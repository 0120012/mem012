mod api;
mod config;
mod parse;
mod provider;
mod psql;
mod server;
mod tools;

struct CliArgs {
    command: Option<String>,
    profile: Option<String>,
    args_json: Option<String>,
    auth_token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli_args = parse::parse_cli_args()?;

    // ==== 1. load config
    let config = match config::load_config("config.toml") {
        Ok(config) => config,
        Err(error) => {
            eprintln!("读取配置失败: {error}");
            return Err(error);
        }
    };

    // Why：server 是常驻 HTTP 模式，不应继续要求 profile 和 --args 这些单次 CLI 参数。
    if cli_args.command.as_deref() == Some("server") {
        let address = config.server_addr();
        server::app_run(address).await;
        return Ok(());
    }
    if let Some(auth_token) = cli_args.auth_token {
        tools::auth::run(config.server_addr(), &auth_token).await?;
        return Ok(());
    }

    // ==== 2. check for init_db()
    let profile = cli_args.profile.ok_or("缺少参数: --profile")?;
    let database_url = config
        .database_url(profile.as_str())
        .ok_or("未找到指定 profile")?;
    // Why：main 持有运行期连接池，init_db 和后续工具才能借用同一组数据库连接。
    let profile_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(database_url)
        .await?;
    if cli_args.command.as_deref() == Some("init") {
        print_init_memories(&profile_pool).await?;
        return Ok(());
    }

    let share_database_url = config
        .database_url("share")
        .ok_or("未找到 profile: share")?;
    let share_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(share_database_url)
        .await?;
    psql::init_db(&profile_pool, &share_pool, config.reset_db()).await?;

    // ==== 3. CLI: parse args json
    let args_json = cli_args.args_json.ok_or("缺少参数: --args")?;
    let request_args = parse::parse_args_json(args_json.as_str())?;
    let embedding_settings = config.embedding_settings();
    let rerank_settings = config.rerank_settings();
    let api_base_url = local_api_base_url(config.server_addr())?;
    let tool_context = tools::ToolContext {
        profile: profile.as_str(),
        profile_pool: &profile_pool,
        search_default_limit: config.search_default_limit(),
        category_index_list: config.category_index_list(),
        api_base_url: api_base_url.as_str(),
        embedding_settings: embedding_settings.as_ref(),
        rerank_settings: rerank_settings.as_ref(),
    };

    // ==== 4. 选择工具, 开始
    tools::dispatch_tool_request(&tool_context, request_args).await?;

    Ok(())
}

async fn print_init_memories(
    pool: &sqlx::Pool<sqlx::Postgres>,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：读取当前 profile 中用于 CLI init 的记忆内容。
    // Why：init 只服务 Agent 启动上下文，应固定读取 init 类并避免输出 uuid/status 等普通记忆元数据。
    let rows = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT title_norm, content
        FROM memory_units
        WHERE category = 'init' AND status <> 'trashed'
        ORDER BY title_norm ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    let results = rows
        .into_iter()
        .map(|(title_norm, content)| serde_json::json!({ "title_norm": title_norm, "content": content }))
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string(&results)?);
    Ok(())
}

fn local_api_base_url(server_addr: &str) -> Result<String, Box<dyn std::error::Error>> {
    // What：把 server 监听地址转换成 CLI 可请求的本机 API 地址。
    // Why：0.0.0.0 是监听地址，不是可靠的客户端目标地址，CLI 必须改用 loopback 访问本机服务。
    let trimmed = server_addr.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("server.addr 不能为空".into());
    }
    let (scheme, target) = if let Some(target) = trimmed.strip_prefix("http://") {
        ("http://", target)
    } else if let Some(target) = trimmed.strip_prefix("https://") {
        ("https://", target)
    } else {
        ("http://", trimmed)
    };
    let target = target
        .strip_prefix("0.0.0.0:")
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or_else(|| target.to_string());
    Ok(format!("{scheme}{target}"))
}

fn init_auth_file_path() -> Result<std::path::PathBuf, String> {
    // What：返回 init grant 在本机的固定授权文件路径。
    // Why：CLI 写入和 create_memory 消费必须使用同一位置，避免授权文件被写到一个路径、读取却去另一个路径。
    let home = std::env::var_os("HOME").ok_or("HOME 未设置，无法定位 init auth file")?;
    Ok(std::path::PathBuf::from(home)
        .join(".auth")
        .join("auth_file.mem"))
}

fn write_init_auth_file(
    path: &std::path::Path,
    grant: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let Some(parent) = path.parent() else {
        return Err("init auth file 路径缺少父目录".into());
    };
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;

    let mut bytes = serde_json::to_vec(grant)?;
    bytes.push(b'\n');
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::{
        exchange_init_auth_grant, init_auth_file_path, local_api_base_url, write_init_auth_file,
    };

    #[test]
    fn local_api_base_url_maps_wildcard_listener_to_loopback() {
        assert_eq!(
            local_api_base_url("0.0.0.0:37777").unwrap(),
            "http://127.0.0.1:37777"
        );
        assert_eq!(
            local_api_base_url("http://0.0.0.0:37777/").unwrap(),
            "http://127.0.0.1:37777"
        );
    }

    #[test]
    fn local_api_base_url_preserves_client_addresses() {
        assert_eq!(
            local_api_base_url("127.0.0.1:37777").unwrap(),
            "http://127.0.0.1:37777"
        );
        assert_eq!(
            local_api_base_url("https://example.com").unwrap(),
            "https://example.com"
        );
    }

    #[tokio::test]
    async fn exchange_init_auth_grant_rejects_empty_token() {
        let error = exchange_init_auth_grant("http://127.0.0.1:37777", " ")
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("--auth token 不能为空"));
    }

    #[test]
    fn init_auth_file_path_uses_fixed_auth_location() {
        let path = init_auth_file_path().unwrap();

        assert!(path.ends_with(".auth/auth_file.mem"));
    }

    #[test]
    fn write_init_auth_file_writes_grant_with_private_permissions() {
        let root =
            std::env::temp_dir().join(format!("mem012_auth_write_test_{}", std::process::id()));
        let path = root.join(".auth").join("auth_file.mem");
        let grant = serde_json::json!({
            "version": 1,
            "payload": {
                "grant_id": "grant",
                "scope": "init:create",
                "iat": 100,
                "exp": 400,
                "nonce": "nonce"
            },
            "signature": "signature"
        });

        write_init_auth_file(&path, &grant).unwrap();

        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&saved).unwrap(),
            grant
        );
        #[cfg(unix)]
        {
            assert_eq!(
                std::fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
