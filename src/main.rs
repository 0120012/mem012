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
    if cli_args.auth_token.is_some() {
        return Err("mem012 --auth grant exchange 尚未实现".into());
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
    let tool_context = tools::ToolContext {
        profile: profile.as_str(),
        profile_pool: &profile_pool,
        search_default_limit: config.search_default_limit(),
        category_index_list: config.category_index_list(),
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
