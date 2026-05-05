use axum::{http::{HeaderMap, StatusCode}, Json};
use serde_json::{json, Value};

// Why：把认证探针从普通数据接口中拆出来，避免前端把任意请求失败误判为需要重新登录。
pub async fn verify(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    let _ = dotenvy::dotenv();
    let expected = std::env::var("API_TOKEN").ok();
    if let Some(token) = expected {
        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim);
        if provided != Some(token.as_str()) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    Ok(Json(json!({ "ok": true, "authenticated": true })))
}
