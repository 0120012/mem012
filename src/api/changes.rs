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
    let embedding = match embedding_for_approve(&url, &memory_uuid, &change).await {
        Ok(embedding) => embedding,
        Err(error) => return error_response(error, Some(&project)),
    };
    match crate::psql::approve_change(&url, &memory_uuid, embedding).await {
        Ok(true) => (
            StatusCode::OK,
            api_response(Some(Value::Null), None, Some(&project)),
        ),
        Ok(false) => missing_change(&project),
        Err(error) => {
            let message = error.to_string();
            let code = if message.starts_with("APPROVE_CONFLICT:") {
                "APPROVE_CONFLICT"
            } else {
                "CHANGE_APPROVE_FAILED"
            };
            error_response(ApiError { code, message }, Some(&project))
        }
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

async fn embedding_for_approve(
    database_url: &str,
    memory_uuid: &str,
    change: &Value,
) -> Result<Option<crate::psql::ApprovedEmbedding>, ApiError> {
    // What：在 approve 提交前从当前 memory_units 工作态生成 embedding。
    // Why：memory_units 是 Agent 可回读的确定状态，embedding 必须绑定这份状态而不是 change 快照。
    let Some((_, action)) = reviewed_change(change) else {
        return Ok(None);
    };
    if action == "delete" {
        return Ok(None);
    }
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "CONFIG_LOAD_FAILED",
        message: error.to_string(),
    })?;
    let Some(settings) = config.embedding_settings() else {
        return Ok(None);
    };
    let source_state = load_current_memory_state(database_url, memory_uuid).await?;
    let input = embedding_input_from_state(&source_state)?;
    let values = crate::provider::embedding::request_embedding(&settings, &input)
        .await
        .map_err(|error| db_error("EMBEDDING_REFRESH_FAILED", error))?;
    Ok(Some(crate::psql::ApprovedEmbedding {
        model: settings.model,
        dimension: settings.dimension as i32,
        values,
        source_state: source_state.to_string(),
    }))
}

async fn load_current_memory_state(
    database_url: &str,
    memory_uuid: &str,
) -> Result<Value, ApiError> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .map_err(|error| db_error("EMBEDDING_INPUT_LOAD_FAILED", error.into()))?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| db_error("EMBEDDING_INPUT_LOAD_FAILED", error.into()))?;
    let state = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| db_error("EMBEDDING_INPUT_LOAD_FAILED", error))?;
    tx.commit()
        .await
        .map_err(|error| db_error("EMBEDDING_INPUT_LOAD_FAILED", error.into()))?;
    serde_json::from_str(&state).map_err(|error| ApiError {
        code: "EMBEDDING_INPUT_LOAD_FAILED",
        message: error.to_string(),
    })
}

fn embedding_input_from_state(state: &Value) -> Result<String, ApiError> {
    let state = state.as_object().ok_or(ApiError {
        code: "EMBEDDING_INPUT_MISSING",
        message: "memory state is invalid".to_string(),
    })?;
    let memory = state.get("memory").ok_or(ApiError {
        code: "EMBEDDING_INPUT_MISSING",
        message: "memory state.memory is missing".to_string(),
    })?;
    let mut parts = ["title_norm", "summary", "content"]
        .into_iter()
        .filter_map(|field| memory.get(field)?.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(keywords) = state.get("keywords").and_then(Value::as_array) {
        let keywords = keywords
            .iter()
            .filter_map(|keyword| keyword.get("keyword_norm")?.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if !keywords.trim().is_empty() {
            parts.push(keywords);
        }
    }
    (!parts.is_empty())
        .then(|| parts.join("\n"))
        .ok_or(ApiError {
            code: "EMBEDDING_INPUT_MISSING",
            message: "embedding input is empty".to_string(),
        })
}

fn reviewed_change(change: &Value) -> Option<(&str, &str)> {
    // Why：审核派生流程只需要动作和目标 uuid，不应让 HTTP 层重复解析完整状态机。
    Some((
        change.get("memory_uuid")?.as_str()?,
        change.get("action")?.as_str()?,
    ))
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
        "APPROVE_CONFLICT" => StatusCode::CONFLICT,
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
