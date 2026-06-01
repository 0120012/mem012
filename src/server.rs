use axum;

pub async fn app_run(addr: &str, sweep_interval_minutes: u64) {
    tokio::spawn(trash_cleanup_worker(sweep_interval_minutes));
    let app = crate::api::router_list();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("server listening on {}", addr);
    let _ = axum::serve(listener, app).await;
}

async fn trash_cleanup_worker(sweep_interval_minutes: u64) {
    let delay = std::time::Duration::from_secs(sweep_interval_minutes.saturating_mul(60));
    loop {
        sweep_expired_trash_once().await;
        tokio::time::sleep(delay).await;
    }
}

// What：server 启动时立即扫描所有 profile 的到期回收站项。
// Why：单个 profile 清理失败不能阻断其他 profile，也不能阻断 HTTP 服务启动。
async fn sweep_expired_trash_once() {
    let Ok(config) = crate::config::load_config("config.toml") else {
        eprintln!("trash cleanup skipped: config load failed");
        return;
    };
    let retention_minutes = config.trash_retention_minutes() as i64;
    for (profile, database_url) in config.database_entries() {
        match crate::psql::delete_expired_trash(database_url, retention_minutes).await {
            Ok(deleted) if deleted > 0 => {
                println!("trash cleanup: {profile} deleted {deleted} expired memories")
            }
            Ok(_) => {}
            Err(error) if is_uninitialized_profile_error(error.as_ref()) => {
                eprintln!("trash cleanup skipped for {profile}: database schema is not initialized")
            }
            Err(error) => eprintln!("trash cleanup failed for {profile}: {error}"),
        }
    }
}

fn is_uninitialized_profile_error(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    matches!(
        error.downcast_ref::<sqlx::Error>(),
        Some(sqlx::Error::Database(database_error))
            if database_error.code().as_deref() == Some("42P01")
    )
}
