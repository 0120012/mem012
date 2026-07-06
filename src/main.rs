mod api;
mod config;
mod parse;
mod provider;
mod psql;
mod server;
mod tools;

const PROFILE_NAME_MAX_LEN: usize = 30;

struct CliArgs {
    command: Option<String>,
    profile: Option<String>,
    create_profile: Option<String>,
    args_json: Option<String>,
    auth_token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_args = std::env::args().skip(1).collect::<Vec<_>>();
    let help_requested = tools::cli_help::agent_help_requested(&raw_args);

    // ==== 1. load config
    let config = match config::load_config("config.toml") {
        Ok(config) => config,
        Err(error) => {
            eprintln!("读取配置失败: {error}");
            return Err(error);
        }
    };
    if help_requested {
        tools::cli_help::print_agent_help(&config)?;
        return Ok(());
    }

    let cli_args = parse::parse_cli_args()?;

    // Why：server 是常驻 HTTP 模式，不应继续要求 profile 和 --args 这些单次 CLI 参数。
    if cli_args.command.as_deref() == Some("server") {
        let address = config.server_addr();
        server::app_run(address, config.cleanup_sweep_interval_minutes()).await;
        return Ok(());
    }
    if let Some(create_profile) = cli_args.create_profile.as_deref() {
        tools::dispatch_create_profile_command(&config, create_profile).await?;
        return Ok(());
    }
    if let Some(auth_token) = cli_args.auth_token {
        cli_args
            .profile
            .as_deref()
            .ok_or("缺少参数: --profile；--auth 需要指定 profile，例如 mem012 --profile maccodex --auth <auth_token>")?;
        let api_base_url =
            config::local_api_base_url(config.client_base_url().unwrap_or(config.server_addr()))?;
        tools::dispatch_auth_command(&api_base_url, &auth_token).await?;
        return Ok(());
    }

    // ==== 2. connect profile database for CLI commands
    let profile = cli_args.profile.ok_or("缺少参数: --profile")?;
    let database_url = config
        .database_url(profile.as_str())
        .ok_or("未找到指定 profile")?;
    // Why：main 持有运行期连接池，init 和后续工具才能借用同一组数据库连接。
    let profile_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(database_url)
        .await?;
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
        config::local_api_base_url(config.client_base_url().unwrap_or(config.server_addr()))?;
    let tool_context = tools::ToolContext {
        profile: profile.as_str(),
        profile_pool: &profile_pool,
        search_default_limit: config.search_default_limit(),
        category_index_list: config.category_index_list(),
        api_base_url: api_base_url.as_str(),
        embedding_settings: embedding_settings.as_ref(),
        rerank_settings: rerank_settings.as_ref(),
        reset_db: config.reset_db(),
    };

    // ==== 4. 选择工具, 开始
    tools::dispatch_tool_request(&tool_context, request_args).await?;

    Ok(())
}
