mod auth;
mod health;
mod memory;
mod review;
mod cleanup;
mod utils;

use axum::{
    routing::{delete, get, post},
    Router,
};

// Why：先固定前端真实依赖的路由表，避免后面联调时反复改入口。
pub fn router_list() -> Router {
    Router::new()
        // ====AUTH====
        .route("/api/auth/verify", get(auth::verify))

        // ====MEMORY====
        .route("/api/health", get(health::health))
        .route("/api/health/profiles", get(health::health_profiles))
        .route("/api/memory/domains", get(memory::domains))
        .route("/api/memory/node", get(memory::get_node).put(memory::put_node))
        .route(
            "/api/memory/glossary",
            get(memory::get_glossary)
                .post(memory::create_glossary)
                .delete(memory::delete_glossary),
        )

        // ====REVIEW====
        .route("/api/review/groups", get(review::groups))
        .route("/api/review/groups/{node_uuid}/diff", get(review::group_diff))
        .route(
            "/api/review/groups/{node_uuid}/rollback",
            post(review::rollback_group),
        )
        .route("/api/review/groups/{node_uuid}", delete(review::delete_group))
        .route("/api/review", delete(review::clear_review))

        // ====CLEANUP====
        .route("/api/cleanup/orphans", get(cleanup::orphans))
        .route(
            "/api/cleanup/orphans/{memory_id}",
            get(cleanup::orphan_detail).delete(cleanup::delete_orphan),
        )
}

#[cfg(test)]
mod tests {
    use super::router_list;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    // Why：先用 health 路由做最小可达性测试，尽早发现路由树装配是否断裂。
    #[tokio::test]
    async fn health_route_is_reachable() {
        let response = router_list()
            .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
