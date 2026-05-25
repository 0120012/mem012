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

    // ==== 2. check for init_db()
    let profile = cli_args.profile.ok_or("缺少参数: --profile")?;
    let database_url = config
        .database_url(profile.as_str())
        .ok_or("未找到指定 profile")?;
    let share_database_url = config
        .database_url("share")
        .ok_or("未找到 profile: share")?;
    // Why：main 持有运行期连接池，init_db 和后续工具才能借用同一组数据库连接。
    let profile_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(database_url)
        .await?;
    let share_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(share_database_url)
        .await?;
    psql::init_db(&profile_pool, &share_pool, config.reset_db()).await?;

    // ==== 3. CLI: parse args json
    let args_json = cli_args.args_json.ok_or("缺少参数: --args")?;
    let request_args = parse::parse_args_json(args_json.as_str())?;
    let embedding_settings = config.embedding_settings();
    let rerank_settings = config.rerank_settings();
    let tool_context = tools::ToolContext {
        profile: profile.as_str(),
        profile_pool: &profile_pool,
        search_default_limit: config.search_default_limit(),
        embedding_settings: embedding_settings.as_ref(),
        rerank_settings: rerank_settings.as_ref(),
    };

    // ==== 4. 选择工具, 开始
    tools::dispatch_tool_request(&tool_context, request_args).await?;

    Ok(())
}
