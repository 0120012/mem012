use axum::Json;
use serde_json::{Value, json};

// Why：先让前端健康检查接口有稳定占位结构，页面联调不依赖数据库。
pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "database": "unconfigured"
    }))
}
