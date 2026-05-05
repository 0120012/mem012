mod api;
mod server;
mod connect_psql;
mod config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //load config
    let config = match config::load_config("config.toml") {
        Ok(config) => config,
        Err(error) => {
            eprintln!("读取配置失败: {error}");
            return Err(error);
        }
    };

    //2\ if need init_db()
    let database_url = config
        .database_url("riko")
        .ok_or("未找到 profile: riko")?;
    connect_psql::init_db(database_url).await?;

    //3\ CLI: parse args json
    //4\ set init prompt memory

    // listen 37777
    let address = config.server_addr();
    server::app_run(address).await;

    Ok(())
}
