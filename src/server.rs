use axum;
pub async fn app_run(addr: &str) {
    let app = crate::api::router_list();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("server listening on {}", addr);
    let _ = axum::serve(listener, app).await;
}
