use sqlx::postgres::PgPoolOptions;
use sqlx::Executor;


// Why：初始化入口必须独立于服务启动，避免运行态自动修改 profile 私库或共享库。
pub async fn init_db(database_url: &str) -> Result<(), sqlx::Error> {

    let pool = PgPoolOptions::new()
        .connect(database_url).await?;

    sqlx::query("SELECT 1").execute(&pool).await?;

    Ok(())
}
