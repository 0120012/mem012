mod api;
mod config;
mod psql;
mod server;

struct CliArgs {
    command: Option<String>,
    profile: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli_args = parse_cli_args()?;

    //load config
    let config = match config::load_config("config.toml") {
        Ok(config) => config,
        Err(error) => {
            eprintln!("读取配置失败: {error}");
            return Err(error);
        }
    };

    if cli_args.command.as_deref() == Some("server") {
        let address = config.server_addr();
        server::app_run(address).await;
        return Ok(());
    }

    //2\ if need init_db()
    let profile = cli_args.profile.ok_or("缺少参数: --profile")?;
    let database_url = config
        .database_url(profile.as_str())
        .ok_or("未找到指定 profile")?;
    let share_database_url = config
        .database_url("share")
        .ok_or("未找到 profile: share")?;
    psql::init_db(database_url, share_database_url).await?;

    //3\ CLI: parse args json

    Ok(())
}

fn parse_cli_args() -> Result<CliArgs, Box<dyn std::error::Error>> {
    // Why：入口只支持 profile + 单个命令，先用最小解析避免把 CLI 合同扩成第二套配置系统。
    let mut command = None;
    let mut profile = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "server" => command = Some(arg),
            "--profile" => profile = args.next(),
            _ => return Err(format!("未知参数: {arg}").into()),
        }
    }

    Ok(CliArgs { command, profile })
}
