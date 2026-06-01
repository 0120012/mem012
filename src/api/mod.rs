mod auth;
mod changes;
mod graph;
mod health;
mod memories;
mod projects;
mod trash;
mod turnstile;
mod utils;

use axum::{
    Router,
    routing::{get, patch, post},
};

// Why：先固定前端真实依赖的路由表，避免后面联调时反复改入口。
pub fn router_list() -> Router {
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
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn trash_routes_are_mounted() {
        for (method, uri) in [
            ("GET", "/api/trash"),
            ("GET", "/api/trash/00000000-0000-0000-0000-000000000001"),
            (
                "POST",
                "/api/trash/00000000-0000-0000-0000-000000000001/delete",
            ),
            (
                "POST",
                "/api/trash/00000000-0000-0000-0000-000000000001/restore",
            ),
        ] {
            let response = router_list()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_ne!(response.status(), StatusCode::NOT_FOUND, "{method} {uri}");
        }
    }
}
