use axum;
pub async fn app_run(addr: &str) {
    let app = crate::api::router_list();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await;
}