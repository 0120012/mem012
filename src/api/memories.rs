use axum::{
    Json,
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;

use super::utils::{ApiError, api_response, require_project};

// Why：先让记忆列表入口经过统一门禁，避免后续真实查询绕过 session 和 project 白名单。
pub async fn list(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let project = match require_project(&headers) {
        Ok(project) => project,
        Err(error) => {
            let status = match error.code {
                "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
                "PROJECT_REQUIRED" => StatusCode::BAD_REQUEST,
                "PROJECT_NOT_FOUND" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            return (status, api_response(None, Some(error), None));
        }
    };

    let data = match load_memory_data(&project).await {
        Ok(data) => data,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_response(None, Some(error), Some(&project)),
            );
        }
    };

    (
        StatusCode::OK,
        api_response(Some(data), None, Some(&project)),
    )
}

// Why：handler 只按 project 选择数据源，真实列表 SQL 必须留在 psql 层统一维护。
async fn load_memory_data(project: &str) -> Result<Value, ApiError> {
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "CONFIG_LOAD_FAILED",
        message: error.to_string(),
    })?;
    let database_url = config.database_url(project).ok_or(ApiError {
        code: "PROJECT_NOT_FOUND",
        message: "project is not configured".to_string(),
    })?;
    crate::psql::list_memories(database_url)
        .await
        .map_err(|error| ApiError {
            code: "MEMORY_LIST_FAILED",
            message: error.to_string(),
        })
}
