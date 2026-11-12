use std::net::SocketAddr;

pub async fn app_run(
    addr: &str,
    config: &crate::config::Config,
    sweep_interval_minutes: u64,
) {
    let state = crate::api::AppState::from_config(config);
    tokio::spawn(trash_cleanup_worker(sweep_interval_minutes, state.clone()));
    tokio::spawn(backfill_embedding_worker(sweep_interval_minutes, state.clone()));
    let app = crate::api::router_list(state);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("server listening on {}", addr);
    let _ = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await;
}

async fn trash_cleanup_worker(
    sweep_interval_minutes: u64,
    state: crate::api::AppState,
) {
    let delay = std::time::Duration::from_secs(sweep_interval_minutes.saturating_mul(60));
    loop {
        sweep_expired_trash_once(&state).await;
        tokio::time::sleep(delay).await;
    }
}

// What：按清理周期运行 embedding 缺口补齐循环。
// Why：补齐任务必须在 HTTP 服务后台运行，不能阻塞服务启动或请求处理。
async fn backfill_embedding_worker(
    sweep_interval_minutes: u64,
    state: crate::api::AppState,
) {
    let delay = std::time::Duration::from_secs(sweep_interval_minutes.saturating_mul(60));
    loop {
        backfill_embeddings_once(&state).await;
        tokio::time::sleep(delay).await;
    }
}

// What：server 启动时立即扫描所有 profile 的到期回收站项。
// Why：单个 profile 清理失败不能阻断其他 profile，也不能阻断 HTTP 服务启动。
async fn sweep_expired_trash_once(state: &crate::api::AppState) {
    let Ok(config) = crate::config::load_config("config.toml") else {
        eprintln!("trash cleanup skipped: config load failed");
        return;
    };
    let retention_minutes = config.trash_retention_minutes() as i64;
    for (profile, database_url) in config.database_entries() {
        let Some(pool) = state.pool_for(profile, database_url) else {
            eprintln!("trash cleanup skipped for {profile}: database pool is unavailable");
            continue;
        };
        match crate::psql::delete_expired_trash(pool, retention_minutes).await {
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

// What：单轮扫描所有 profile，并逐条串行补齐待办 embedding。
// Why：将失败隔离在 profile 和 UUID 边界，避免一次数据库或 provider 故障终止整个后台循环。
async fn backfill_embeddings_once(state: &crate::api::AppState) {
    let Ok(config) = crate::config::load_config("config.toml") else {
        eprintln!("embedding backfill skipped: config load failed");
        return;
    };
    // Why：embedding 配置是全局的而非 per-profile，未配置时整轮无事可做，不必遍历 profile。
    let Some(settings) = config.embedding_settings() else {
        return;
    };
    for (profile, database_url) in config.database_entries() {
        let Some(pool) = state.pool_for(profile, database_url) else {
            eprintln!("embedding backfill skipped for {profile}: database pool is unavailable");
            continue;
        };
        let uuids = match crate::psql::list_embedding_backfill_memory_uuids(
            pool,
        )
        .await
        {
            Ok(uuids) => uuids,
            Err(error) if is_uninitialized_profile_error(error.as_ref()) => {
                eprintln!(
                    "embedding backfill skipped for {profile}: database schema is not initialized"
                );
                continue;
            }
            Err(error) => {
                eprintln!("embedding backfill failed for {profile}: {error}");
                continue;
            }
        };
        for memory_uuid in uuids {
            let embedding = match crate::api::generate_embedding_for_memory(
                pool,
                &memory_uuid,
                &settings,
            )
            .await
            {
                Ok(embedding) => embedding,
                Err(error) => {
                    eprintln!(
                        "embedding backfill skipped for {profile}/{memory_uuid}: {} {}",
                        error.code, error.message
                    );
                    continue;
                }
            };
            if let Err(error) = crate::psql::refresh_memory_embedding(pool, &memory_uuid, embedding).await {
                eprintln!("embedding backfill write failed for {profile}/{memory_uuid}: {error}");
            }
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
