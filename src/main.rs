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
    let raw_args = std::env::args().skip(1).collect::<Vec<_>>();
    let help_requested = agent_help_requested(&raw_args);

    // ==== 1. load config
    let config = match config::load_config("config.toml") {
        Ok(config) => config,
        Err(error) => {
            eprintln!("读取配置失败: {error}");
            return Err(error);
        }
    };
    if help_requested {
        print_agent_help(&config)?;
        return Ok(());
    }

    let cli_args = parse::parse_cli_args()?;

    // Why：server 是常驻 HTTP 模式，不应继续要求 profile 和 --args 这些单次 CLI 参数。
    if cli_args.command.as_deref() == Some("server") {
        let address = config.server_addr();
        server::app_run(address, config.cleanup_sweep_interval_minutes()).await;
        return Ok(());
    }
    if let Some(auth_token) = cli_args.auth_token {
        cli_args
            .profile
            .as_deref()
            .ok_or("缺少参数: --profile；--auth 需要指定 profile，例如 mem012 --profile maccodex --auth <auth_token>")?;
        tools::dispatch_auth_command(
            config.client_base_url().unwrap_or(config.server_addr()),
            &auth_token,
        )
        .await?;
        return Ok(());
    }

    // ==== 2. connect profile database for CLI commands
    let profile = cli_args.profile.ok_or("缺少参数: --profile")?;
    let database_url = config
        .database_url(profile.as_str())
        .ok_or("未找到指定 profile")?;
    // Why：main 持有运行期连接池，dbsetup、init 和后续工具才能借用同一组数据库连接。
    let profile_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(database_url)
        .await?;
    if cli_args.command.as_deref() == Some("dbsetup") {
        // What：只在显式 dbsetup 命令中执行数据库 schema 初始化/迁移。
        // Why：远程 PostgreSQL 下普通工具调用反复跑 DDL/schema check 会显著增加延迟。
        psql::init_db(&profile_pool, profile.as_str(), config.reset_db()).await?;
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "state": "success",
                "tool": "dbsetup",
                "data": { "initialized": true },
                "error": null,
                "profile": profile
            }))?
        );
        return Ok(());
    }
    if cli_args.command.as_deref() == Some("init") {
        tools::dispatch_init_command(&profile_pool).await?;
        return Ok(());
    }

    // ==== 3. CLI: parse args json
    let args_json = cli_args.args_json.ok_or("缺少参数: --args")?;
    let request_args = parse::parse_args_json(args_json.as_str())?;
    let embedding_settings = config.embedding_settings();
    let rerank_settings = config.rerank_settings();
    let api_base_url =
        local_api_base_url(config.client_base_url().unwrap_or(config.server_addr()))?;
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

fn print_agent_help(config: &config::Config) -> Result<(), Box<dyn std::error::Error>> {
    // What：输出给 Agent 读取的 CLI 能力入口元数据。
    // Why：category 白名单来自运行时配置，避免 help 和实际写入校验出现两套来源。
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "state": "success",
            "tool": "help",
            "data": {
                "skill": {
                    "name": "mem012-memory-skill"
                },
                "categories": {
                    "cateory_list": config.category_index_list()
                },
                "failure_instruction": "任一 mem012 命令失败后，禁止猜测或重复尝试其他 mem012/file/strings/grep 探测命令；立即停止，并向用户报告失败命令、退出码和错误输出。"
            },
            "error": null
        }))?
    );
    Ok(())
}

fn agent_help_requested(args: &[String]) -> bool {
    // What：识别 Agent 常见的 help 误调用形态。
    // Why：help 必须在严格 CLI 解析前短路，避免 Agent 失败后继续枚举命令探测二进制。
    let mut skip_value = false;
    for (index, arg) in args.iter().enumerate() {
        if skip_value {
            skip_value = false;
            continue;
        }
        match arg.as_str() {
            "--help" | "help" | "--tool=help" => return true,
            "--tool" if args.get(index + 1).is_some_and(|value| value == "help") => return true,
            "--profile" | "--args" | "--auth" => skip_value = true,
            _ => {}
        }
    }
    false
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

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::{init_auth_file_path, local_api_base_url, write_init_auth_file};

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
