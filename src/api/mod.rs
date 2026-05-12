mod auth;
mod health;
mod projects;
mod utils;

use axum::{
    Router,
    routing::{get, post},
};

// Why：先固定前端真实依赖的路由表，避免后面联调时反复改入口。
pub fn router_list() -> Router {
    Router::new()
        .route("/api/health", get(health::health))
        .route("/api/auth/verify", post(auth::verify))
        .route("/api/auth/session", get(auth::session))
        .route("/api/projects", get(projects::list))
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
}
