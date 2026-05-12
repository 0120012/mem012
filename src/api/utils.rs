use axum::{Json, http::HeaderMap};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

// Why：所有 HTTP handler 必须共享同一响应外壳，避免成功和失败路径各自漂移。
pub fn api_response(
    data: Option<Value>,
    error: Option<ApiError>,
    project: Option<&str>,
) -> Json<Value> {
    let state = if error.is_some() { "failed" } else { "success" };

    Json(json!({
        "state": state,
        "data": data,
        "error": error,
        "meta": {
            "project": project
        }
    }))
}

// Why：非 auth 数据接口必须先统一校验登录和项目白名单，避免各 handler 自行拼接数据库目标。
pub fn require_project(headers: &HeaderMap) -> Result<String, ApiError> {
    if !super::auth::has_valid_session(headers)? {
        return Err(super::auth::unauthorized_error());
    }
    let project = headers
        .get("x-mem-project")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ApiError {
            code: "PROJECT_REQUIRED",
            message: "X-Mem-Project header is required".to_string(),
        })?;
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "CONFIG_LOAD_FAILED",
        message: error.to_string(),
    })?;
    config
        .database_url(project)
        .map(|_| project.to_string())
        .ok_or(ApiError {
            code: "PROJECT_NOT_FOUND",
            message: "project is not configured".to_string(),
        })
}
