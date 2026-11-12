mod auth;
mod changes;
mod graph;
mod health;
mod memories;
mod projects;
mod trash;
mod utils;

pub(crate) use changes::generate_embedding_for_memory;

#[derive(Clone, Default)]
pub(crate) struct AppState {
    pools: std::sync::Arc<std::collections::HashMap<String, ProfilePool>>,
}

#[derive(Clone)]
struct ProfilePool {
    database_url_hash: [u8; 32],
    pool: sqlx::PgPool,
}

impl AppState {
    // What：按启动配置为格式有效的 profile 建立惰性连接池。
    // Why：单个错误 profile 不应阻断其他 profile 或无数据库依赖的 HTTP 路由。
    pub(crate) fn from_config(config: &crate::config::Config) -> Self {
        Self::from_database_entries(config.database_entries())
    }

    // What：从 profile/URL 项构建 Server 的连接池集合。
    // Why：集中验证池的缓存键、profile 隔离和坏 URL 跳过行为。
    fn from_database_entries<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut pools = std::collections::HashMap::new();
        for (profile, database_url) in entries {
            let pool = match sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect_lazy(database_url)
            {
                Ok(pool) => pool,
                Err(_) => {
                    eprintln!("database pool skipped for profile {profile}: invalid URL");
                    continue;
                }
            };
            let database_url_hash =
                <sha2::Sha256 as sha2::Digest>::digest(database_url.as_bytes()).into();
            pools.insert(
                profile.to_owned(),
                ProfilePool {
                    database_url_hash,
                    pool,
                },
            );
        }
        Self {
            pools: std::sync::Arc::new(pools),
        }
    }

    pub(crate) fn pool_for(
        &self,
        profile: &str,
        database_url: &str,
    ) -> Option<&sqlx::PgPool> {
        let profile_pool = self.pools.get(profile)?;
        let database_url_hash =
            <sha2::Sha256 as sha2::Digest>::digest(database_url.as_bytes());
        (profile_pool.database_url_hash.as_slice() == database_url_hash.as_slice())
            .then_some(&profile_pool.pool)
    }
}

use axum::{
    Router,
    routing::{get, patch, post},
};

// Why：先固定前端真实依赖的路由表，避免后面联调时反复改入口。
pub fn router_list(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health::health))
        .route("/api/auth/verify", post(auth::verify))
        .route("/api/auth/session", get(auth::session))
        .route("/api/auth/status", get(auth::auth_status))
        .route("/api/auth/refresh", post(auth::auth_refresh))
        .route("/api/auth/refresh/force", post(auth::auth_force_refresh))
        .route("/api/auth/grant", post(auth::auth_grant))
        .route("/api/auth/grant/consume", post(auth::auth_grant_consume))
        .route("/api/projects", get(projects::list))
        .route("/api/memories", get(memories::list))
        .route("/api/create_memory", post(memories::create))
        .route("/api/memories/{memory_uuid}", patch(memories::update))
        .route(
            "/api/memories/categories/{category}/keywords",
            get(memories::category_keywords),
        )
        .route("/api/trash", get(trash::list))
        .route("/api/trash/{memory_uuid}", get(trash::detail))
        .route("/api/trash/{memory_uuid}/delete", post(trash::delete))
        .route("/api/trash/{memory_uuid}/restore", post(trash::restore))
        .route("/api/changes", get(changes::list))
        .route("/api/changes/count", get(changes::count))
        .route("/api/changes/{memory_uuid}", get(changes::detail))
        .route("/api/changes/{memory_uuid}/approve", post(changes::approve))
        .route("/api/changes/{memory_uuid}/reject", post(changes::reject))
        .route("/api/graph/status", get(graph::status))
        .route("/api/graph/overview", get(graph::overview))
        .route("/api/graph/rebuild", post(graph::rebuild))
        .route("/api/graph/neighbors/{memory_uuid}", get(graph::neighbors))
        .route("/api/graph/relations", post(graph::add_relation))
        .route(
            "/api/graph/relations/{relation_uuid}",
            patch(graph::update_relation).delete(graph::delete_relation),
        )
        .route(
            "/api/graph/relations/suggest/{memory_uuid}",
            get(graph::suggest_relations),
        )
        .with_state(state)
}

#[cfg(test)]
mod pool_state_tests {
    use super::AppState;

    #[tokio::test(flavor = "current_thread")]
    async fn pool_lookup_reuses_profile_pool_and_rejects_changed_url() {
        let state = AppState::from_database_entries([
            ("codex", "postgres://user:pass@localhost/codex"),
            ("claude", "postgres://user:pass@localhost/claude"),
        ]);
        let codex = state
            .pool_for("codex", "postgres://user:pass@localhost/codex")
            .unwrap();
        let codex_again = state
            .pool_for("codex", "postgres://user:pass@localhost/codex")
            .unwrap();
        let claude = state
            .pool_for("claude", "postgres://user:pass@localhost/claude")
            .unwrap();

        assert!(std::ptr::eq(codex, codex_again));
        assert!(!std::ptr::eq(codex, claude));
        assert_eq!(codex.options().get_max_connections(), 5);
        assert!(state
            .pool_for("codex", "postgres://user:pass@localhost/changed")
            .is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_profile_url_is_skipped_without_blocking_valid_profiles() {
        let state = AppState::from_database_entries([
            ("bad", "not a database URL"),
            ("good", "postgres://user:pass@localhost/good"),
        ]);

        assert!(state.pool_for("bad", "not a database URL").is_none());
        assert!(state
            .pool_for("good", "postgres://user:pass@localhost/good")
            .is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires the configured codex PostgreSQL database"]
    async fn live_detail_query_reuses_server_profile_connection() {
        let config = crate::config::load_config("config.toml").unwrap();
        let state = AppState::from_config(&config);
        let database_url = config.database_url("codex").unwrap();
        let pool = state.pool_for("codex", database_url).unwrap();
        let before: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(pool)
            .await
            .unwrap();
        let _ = crate::psql::get_change(
            pool,
            "aa7dd487-48ee-4e98-ba00-601c236dd620",
        )
        .await
        .unwrap();
        let after: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(pool)
            .await
            .unwrap();

        assert_eq!(before, after);
        assert!(pool.size() <= 5);
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    use super::{AppState, router_list};

    #[tokio::test]
    async fn projects_route_rejects_anonymous_request() {
        let response = router_list(AppState::default())
            .oneshot(
                Request::builder()
                    .uri("/api/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
