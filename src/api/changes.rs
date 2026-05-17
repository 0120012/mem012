use axum::{
    Json,
    extract::Path,
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;

use super::utils::{ApiError, api_response, require_project};

// Why：待确认列表必须和 memories 分流，避免普通记忆页展示未审核工作态。
pub async fn list(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let project = match require_project(&headers) {
        Ok(project) => project,
        Err(error) => return error_response(error, None),
    };
    let url = match database_url(&project) {
        Ok(url) => url,
        Err(error) => return error_response(error, Some(&project)),
    };
    let data = match crate::psql::list_changes(&url).await {
        Ok(data) => data,
        Err(error) => return error_response(db_error("CHANGE_LIST_FAILED", error), Some(&project)),
    };
    (
        StatusCode::OK,
        api_response(Some(data), None, Some(&project)),
    )
}

// Why：详情接口只根据 memory_uuid 读取 before/after，不让前端提交回滚状态。
pub async fn detail(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let project = match require_project(&headers) {
        Ok(project) => project,
        Err(error) => return error_response(error, None),
    };
    let data = match load_change_detail(&project, &memory_uuid).await {
        Ok(Some(data)) => data,
        Ok(None) => return missing_change(&project),
        Err(error) => return error_response(error, Some(&project)),
    };
    (
        StatusCode::OK,
        api_response(Some(data), None, Some(&project)),
    )
}

// Why：approve 的状态机在 psql 层统一执行，HTTP 层只负责鉴权和定位 change。
pub async fn approve(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let project = match require_project(&headers) {
        Ok(project) => project,
        Err(error) => return error_response(error, None),
    };
    let url = match database_url(&project) {
        Ok(url) => url,
        Err(error) => return error_response(error, Some(&project)),
    };
    let change = match crate::psql::get_change(&url, &memory_uuid).await {
        Ok(Some(change)) => change,
        Ok(None) => return missing_change(&project),
        Err(error) => {
            return error_response(db_error("CHANGE_DETAIL_FAILED", error), Some(&project));
        }
    };
    match crate::psql::approve_change(&url, &memory_uuid).await {
        Ok(true) => {
            refresh_embedding_after_approve(&project, &url, &change).await;
            (
                StatusCode::OK,
                api_response(Some(Value::Null), None, Some(&project)),
            )
        }
        Ok(false) => missing_change(&project),
        Err(error) => error_response(db_error("CHANGE_APPROVE_FAILED", error), Some(&project)),
    }
}

// Why：reject 只接受 memory_uuid，回滚依据必须来自后端锁定的 before_state。
pub async fn reject(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let project = match require_project(&headers) {
        Ok(project) => project,
        Err(error) => return error_response(error, None),
    };
    let url = match database_url(&project) {
        Ok(url) => url,
        Err(error) => return error_response(error, Some(&project)),
    };
    match crate::psql::reject_change(&url, &memory_uuid).await {
        Ok(true) => (
            StatusCode::OK,
            api_response(Some(Value::Null), None, Some(&project)),
        ),
        Ok(false) => missing_change(&project),
        Err(error) => error_response(db_error("CHANGE_REJECT_FAILED", error), Some(&project)),
    }
}

// Why：project 到 database_url 的映射只能来自配置，不能由请求参数直接决定连接串。
fn database_url(project: &str) -> Result<String, ApiError> {
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "CONFIG_LOAD_FAILED",
        message: error.to_string(),
    })?;
    config
        .database_url(project)
        .map(str::to_string)
        .ok_or(ApiError {
            code: "PROJECT_NOT_FOUND",
            message: "project is not configured".to_string(),
        })
}

async fn refresh_embedding_after_approve(project: &str, database_url: &str, change: &Value) {
    // Why：embedding 是 approve create 的派生索引，失败只降级语义召回，不撤销批准。
    let Some(memory_uuid) = approved_create_memory_uuid(change) else {
        return;
    };
    let Ok(config) = crate::config::load_config("config.toml") else {
        return;
    };
    let Some(settings) = config.embedding_settings() else {
        return;
    };
    if let Err(error) =
        crate::embeddings::refresh_memory_embedding(database_url, settings, memory_uuid).await
    {
        eprintln!("{project}: embedding 生成失败: {error}");
    }
}

fn approved_create_memory_uuid(change: &Value) -> Option<&str> {
    // Why：只有 create 批准会让 pending 变 active，update/delete 不应在这里重算 embedding。
    if change.get("action")?.as_str()? != "create" {
        return None;
    }
    change.get("memory_uuid")?.as_str()
}

// Why：详情读取需要区分不存在和数据库失败，前端才能展示正确状态。
async fn load_change_detail(project: &str, uuid: &str) -> Result<Option<Value>, ApiError> {
    let url = database_url(project)?;
    crate::psql::get_change(&url, uuid)
        .await
        .map_err(|error| db_error("CHANGE_DETAIL_FAILED", error))
}

// Why：不同 handler 必须共享状态码映射，避免同一错误在各接口表现不一致。
fn error_response(error: ApiError, project: Option<&str>) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
        "PROJECT_REQUIRED" => StatusCode::BAD_REQUEST,
        "PROJECT_NOT_FOUND" | "CHANGE_NOT_FOUND" => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, api_response(None, Some(error), project))
}

// Why：不存在的 change 要稳定返回同一错误码，方便前端从列表刷新掉过期项。
fn missing_change(project: &str) -> (StatusCode, Json<Value>) {
    error_response(
        ApiError {
            code: "CHANGE_NOT_FOUND",
            message: "change is not found".to_string(),
        },
        Some(project),
    )
}

// Why：数据库错误包装成固定 API code，避免把底层错误类型泄漏给前端分支。
fn db_error(code: &'static str, error: Box<dyn std::error::Error + Send + Sync>) -> ApiError {
    ApiError {
        code,
        message: error.to_string(),
    }
}
