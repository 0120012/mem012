pub(crate) async fn run(
    api_base_url: &str,
    auth_token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：执行 `mem012 --auth` 的本机 grant 换取和授权文件落盘流程。
    // Why：main 只保留 CLI 分发，授权副作用集中在 tools/auth.rs，便于后续独立审查。
    let auth_file_path = super::auth_file::init_auth_file_path()?;
    remove_init_auth_file(&auth_file_path)?;
    let grant = exchange_init_auth_grant(api_base_url, auth_token).await?;
    super::auth_file::write_init_auth_file(&auth_file_path, &grant)?;
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

async fn exchange_init_auth_grant(
    api_base_url: &str,
    auth_token: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // What：向本机 HTTP API 提交前端 auth_token，并换取 init:create grant。
    // Why：CLI 不能保存前端 token，只能保存后端签发的一次性 Ed25519 grant。
    let auth_token = auth_token.trim();
    if auth_token.is_empty() {
        return Err("--auth token 不能为空".into());
    }
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/auth/grant",
            api_base_url.trim_end_matches('/')
        ))
        .json(&serde_json::json!({ "auth_token": auth_token }))
        .send()
        .await?;
    let status = response.status();
    let body = response.json::<serde_json::Value>().await?;
    if !status.is_success() {
        let message = body
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| status.to_string());
        return Err(format!("换取 init grant 失败: {message}").into());
    }
    body.get("data")
        .cloned()
        .ok_or_else(|| "init grant 响应缺少 data".into())
}

#[cfg(test)]
mod tests {
    use super::{exchange_init_auth_grant, remove_init_auth_file};

    #[tokio::test]
    async fn exchange_init_auth_grant_rejects_empty_token() {
        let error = exchange_init_auth_grant("http://127.0.0.1:37777", " ")
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("--auth token 不能为空"));
    }

    #[test]
    fn remove_init_auth_file_ignores_missing_file() {
        let path = std::env::temp_dir().join("mem012_missing_tool_auth_file_for_test.mem");

        assert!(remove_init_auth_file(&path).is_ok());
    }
}
