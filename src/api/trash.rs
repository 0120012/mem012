use axum::{
    Json,
    extract::Path,
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;

use super::utils::{ApiError, api_response, require_project};

// What：返回当前项目回收站列表，包含进入回收站时间和配置派生的自动硬删时间。
// Why：回收站页面只展示当前 active project，跨 profile 自动清理由后台 worker 单独负责。
pub async fn list(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let project = match require_project(&headers) {
        Ok(project) => project,
        Err(error) => return error_response(error, None),
    };
    let config = match crate::config::load_config("config.toml") {
        Ok(config) => config,
        Err(error) => {
            return error_response(
                ApiError {
                    code: "CONFIG_LOAD_FAILED",
                    message: error.to_string(),
                },
                Some(&project),
            );
        }
    };
    let Some(database_url) = config.database_url(&project) else {
        return error_response(
            ApiError {
                code: "PROJECT_NOT_FOUND",
                message: "project is not configured".to_string(),
            },
            Some(&project),
        );
    };
    match crate::psql::list_trash(database_url, config.trash_retention_minutes() as i64).await {
        Ok(data) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Err(error) => error_response(
            ApiError {
                code: "TRASH_LIST_FAILED",
                message: error.to_string(),
            },
            Some(&project),
        ),
    }
}

pub async fn detail(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let (project, database_url, retention_minutes) = match trash_context(&headers) {
        Ok(context) => context,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::get_trash(&database_url, &memory_uuid, retention_minutes).await {
        Ok(Some(data)) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Ok(None) => missing_trash(&project),
        Err(error) => error_response(
            ApiError {
                code: "TRASH_DETAIL_FAILED",
                message: error.to_string(),
            },
            Some(&project),
        ),
    }
}

// What：手动批准当前回收站项立即永久删除。
// Why：HTTP 层不能复用 changes approve，否则恢复/删除规则会和专用回收站语义混在一起。
pub async fn delete(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let (project, database_url, _) = match trash_context(&headers) {
        Ok(context) => context,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::delete_trash(&database_url, &memory_uuid).await {
        Ok(true) => (
            StatusCode::OK,
            api_response(Some(Value::Null), None, Some(&project)),
        ),
        Ok(false) => missing_trash(&project),
        Err(error) => {
            let message = error.to_string();
            let code = if message.starts_with("TRASH_STATE_INVALID") {
                "TRASH_STATE_INVALID"
            } else {
                "TRASH_DELETE_FAILED"
            };
            error_response(ApiError { code, message }, Some(&project))
        }
    }
}

// What：从当前项目回收站恢复一条记忆。
// Why：恢复规则由 psql 层区分 active/pending，HTTP 层只提供专用回收站入口。
pub async fn restore(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let (project, database_url, _) = match trash_context(&headers) {
        Ok(context) => context,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::restore_trash(&database_url, &memory_uuid).await {
        Ok(true) => (
            StatusCode::OK,
            api_response(Some(Value::Null), None, Some(&project)),
        ),
        Ok(false) => missing_trash(&project),
        Err(error) => {
            let message = error.to_string();
            let code = if message.starts_with("DELETE_CHANGE_NOT_FOUND") {
                "DELETE_CHANGE_NOT_FOUND"
            } else {
                "TRASH_RESTORE_FAILED"
            };
            error_response(ApiError { code, message }, Some(&project))
        }
    }
}

fn trash_context(headers: &HeaderMap) -> Result<(String, String, i64), (StatusCode, Json<Value>)> {
    let project = require_project(headers).map_err(|error| error_response(error, None))?;
    let config = crate::config::load_config("config.toml").map_err(|error| {
        error_response(
            ApiError {
                code: "CONFIG_LOAD_FAILED",
                message: error.to_string(),
            },
            Some(&project),
        )
    })?;
    let Some(database_url) = config.database_url(&project) else {
        return Err(error_response(
            ApiError {
                code: "PROJECT_NOT_FOUND",
                message: "project is not configured".to_string(),
            },
            Some(&project),
        ));
    };
    Ok((
        project,
        database_url.to_string(),
        config.trash_retention_minutes() as i64,
    ))
}

fn missing_trash(project: &str) -> (StatusCode, Json<Value>) {
    error_response(
        ApiError {
            code: "TRASH_NOT_FOUND",
            message: "trash item is not found".to_string(),
        },
        Some(project),
    )
}

fn error_response(error: ApiError, project: Option<&str>) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
        "PROJECT_REQUIRED" => StatusCode::BAD_REQUEST,
        "PROJECT_NOT_FOUND" | "TRASH_NOT_FOUND" => StatusCode::NOT_FOUND,
        "DELETE_CHANGE_NOT_FOUND" | "TRASH_STATE_INVALID" => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, api_response(None, Some(error), project))
}
