#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportMemoryArgs {
    input_path: String,
}

pub async fn run(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：从 mem012 备份 JSON 导入 active memory 主体和 keywords。
    // Why：先固定无关系导入，避免把冲突合并和 relation 恢复混在一起。
    let import_args = serde_json::from_value::<ImportMemoryArgs>(args.clone())?;
    let input_path = import_args.input_path.trim();
    if input_path.is_empty() {
        return Err("import_memory input_path 不能为空".into());
    }
    let backup = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(input_path)?)?;
    if backup.get("format").and_then(serde_json::Value::as_str) != Some("mem012.memory_backup.v1") {
        return Err("import_memory 只支持 mem012.memory_backup.v1".into());
    }
    let memories = backup
        .get("memories")
        .and_then(serde_json::Value::as_array)
        .ok_or("import_memory memories 缺失或不是数组")?;
    let mut memory_uuids = Vec::with_capacity(memories.len());
    for state in memories {
        let memory = state
            .get("memory")
            .and_then(serde_json::Value::as_object)
            .ok_or("import_memory memory 缺失或不是对象")?;
        if memory.get("status").and_then(serde_json::Value::as_str) != Some("active") {
            return Err("import_memory 当前只支持 active memory".into());
        }
        if state
            .get("keywords")
            .and_then(serde_json::Value::as_array)
            .is_none()
        {
            return Err("import_memory keywords 缺失或不是数组".into());
        }
        if state
            .get("relations")
            .and_then(serde_json::Value::as_array)
            .is_none()
        {
            return Err("import_memory relations 缺失或不是数组".into());
        }
        memory_uuids.push(
            memory
                .get("uuid")
                .and_then(serde_json::Value::as_str)
                .ok_or("import_memory memory.uuid 缺失或不是字符串")?,
        );
    }
    let mut tx = context.profile_pool.begin().await?;
    sqlx::query(r#"INSERT INTO memory_units (uuid, category, title_norm, content, summary, revision, status, recall_when, trashed_at, created_at, updated_at) SELECT (state #>> '{memory,uuid}')::uuid, state #>> '{memory,category}', state #>> '{memory,title_norm}', state #>> '{memory,content}', state #>> '{memory,summary}', (state #>> '{memory,revision}')::bigint, state #>> '{memory,status}', state #>> '{memory,recall_when}', (state #>> '{memory,trashed_at}')::timestamptz, now(), now() FROM jsonb_array_elements($1::jsonb -> 'memories') AS memories(state)"#)
        .bind(backup.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memory_keywords (uuid, memory_uuid, keyword_norm, weight, created_at) SELECT gen_random_uuid(), (state #>> '{memory,uuid}')::uuid, keyword ->> 'keyword_norm', (keyword ->> 'weight')::int, now() FROM jsonb_array_elements($1::jsonb -> 'memories') AS memories(state) CROSS JOIN LATERAL jsonb_array_elements(state -> 'keywords') AS keywords(keyword)")
        .bind(backup.to_string())
        .execute(&mut *tx)
        .await?;
    for memory_uuid in &memory_uuids {
        crate::psql::search_index::refresh_memory_search_index(&mut tx, memory_uuid).await?;
    }
    if !memory_uuids.is_empty() {
        crate::psql::mark_memory_graph_dirty(&mut tx).await?;
    }
    tx.commit().await?;
    println!(
        "{}",
        serde_json::json!({"state":"success","tool":"import_memory","data":{"input_path":input_path,"source_profile":backup.get("profile").and_then(serde_json::Value::as_str),"target_profile":context.profile,"memory_uuids":memory_uuids,"count":memory_uuids.len(),"relations_imported":0,"result":"imported"},"error":null,"profile":context.profile})
    );
    Ok(())
}
