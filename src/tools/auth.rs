pub(crate) async fn run(
    server_addr: &str,
    auth_token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：执行 `mem012 --auth` 的本机 grant 换取和授权文件落盘流程。
    // Why：main 只保留 CLI 分发，授权副作用集中在 tools/auth.rs，便于后续独立审查。
    let api_base_url = crate::local_api_base_url(server_addr)?;
    let auth_file_path = crate::init_auth_file_path()?;
    remove_init_auth_file(&auth_file_path)?;
    let grant = crate::exchange_init_auth_grant(&api_base_url, auth_token).await?;
    crate::write_init_auth_file(&auth_file_path, &grant)?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "auth",
            "data": {
                "auth_file": "~/.auth/auth_file.mem"
            },
            "error": null
        })
    );
    Ok(())
}

pub(crate) fn remove_init_auth_file(
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::remove_init_auth_file;

    #[test]
    fn remove_init_auth_file_ignores_missing_file() {
        let path = std::env::temp_dir().join("mem012_missing_tool_auth_file_for_test.mem");

        assert!(remove_init_auth_file(&path).is_ok());
    }
}
