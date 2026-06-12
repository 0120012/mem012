#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BackupMemoryArgs {
    output_path: String,
}

pub async fn run(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：导出当前 profile 的非回收站 memory_state 快照集合。
    // Why：备份只读路径必须复用现有快照结构，避免和 read_memory 出现字段漂移。
    let backup_args = serde_json::from_value::<BackupMemoryArgs>(args.clone())?;
    let output_path = backup_args.output_path.trim();
    if output_path.is_empty() {
        return Err("backup_memory output_path 不能为空".into());
    }
    let mut tx = context.profile_pool.begin().await?;
    let memory_uuids = sqlx::query_scalar::<_, String>(
        "SELECT uuid::text FROM memory_units WHERE status = 'active' ORDER BY updated_at ASC, uuid",
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut memories = Vec::with_capacity(memory_uuids.len());
    for memory_uuid in memory_uuids {
        let state_text = crate::psql::memory_state(&mut tx, &memory_uuid)
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        memories.push(serde_json::from_str::<serde_json::Value>(&state_text)?);
    }
    tx.commit().await?;
    let backup = serde_json::json!({"format": "mem012.memory_backup.v1", "profile": context.profile, "memories": memories});
    let mut path = std::path::PathBuf::from(output_path);
    if path.is_dir() {
        path.push("backup.json");
    }
    write_private_backup_file(&path, &serde_json::to_string_pretty(&backup)?)?;
    let path = path.display().to_string();
    println!(
        "{}",
        serde_json::json!({"state":"success","tool":"backup_memory","data":{"path":path,"count":backup["memories"].as_array().map_or(0, Vec::len)},"error":null,"profile":context.profile})
    );
    Ok(())
}

fn write_private_backup_file(
    path: &std::path::Path,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：以私有权限写入 memory 备份文件。
    // Why：备份包含完整 memory 内容，不能依赖进程 umask 决定本机其他用户是否可读。
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}
