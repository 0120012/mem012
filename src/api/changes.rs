use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;

use super::utils::{ApiError, api_response, require_project};

// Why：待确认列表必须和 memories 分流，避免普通记忆页展示未审核工作态。
pub async fn list(
    State(state): State<super::AppState>,
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
    let Some(pool) = state.pool_for(&project, &url) else {
        return error_response(
            ApiError {
                code: "DATABASE_POOL_UNAVAILABLE",
                message: "database pool is unavailable; restart the server after changing database configuration".to_string(),
            },
            Some(&project),
        );
    };
    let data = match crate::psql::list_changes(pool).await {
        Ok(data) => data,
        Err(error) => return error_response(db_error("CHANGE_LIST_FAILED", error), Some(&project)),
    };
    (
        StatusCode::OK,
        api_response(Some(data), None, Some(&project)),
    )
}

// What：返回当前项目待确认变更数量。
// Why：Layout 只需显示徽标，不应为此加载完整变更摘要。
pub async fn count(
    State(state): State<super::AppState>,
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
    let Some(pool) = state.pool_for(&project, &url) else {
        return error_response(
            ApiError {
                code: "DATABASE_POOL_UNAVAILABLE",
                message: "database pool is unavailable; restart the server after changing database configuration".to_string(),
            },
            Some(&project),
        );
    };
    let count = match crate::psql::count_changes(pool).await {
        Ok(count) => count,
        Err(error) => {
            return error_response(db_error("CHANGE_COUNT_FAILED", error), Some(&project));
        }
    };
    (
        StatusCode::OK,
        api_response(
            Some(serde_json::json!({ "count": count })),
            None,
            Some(&project),
        ),
    )
}

// Why：详情接口只根据 memory_uuid 读取 before/after，不让前端提交回滚状态。
pub async fn detail(
    Path(memory_uuid): Path<String>,
    State(state): State<super::AppState>,
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
    let Some(pool) = state.pool_for(&project, &url) else {
        return error_response(
            ApiError {
                code: "DATABASE_POOL_UNAVAILABLE",
                message: "database pool is unavailable; restart the server after changing database configuration".to_string(),
            },
            Some(&project),
        );
    };
    let data = match load_change_detail(pool, &memory_uuid).await {
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
    State(state): State<super::AppState>,
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
    let Some(pool) = state.pool_for(&project, &url) else {
        return error_response(
            ApiError {
                code: "DATABASE_POOL_UNAVAILABLE",
                message: "database pool is unavailable; restart the server after changing database configuration".to_string(),
            },
            Some(&project),
        );
    };
    let change = match crate::psql::get_change(pool, &memory_uuid).await {
        Ok(Some(change)) => change,
        Ok(None) => return missing_change(&project),
        Err(error) => {
            return error_response(db_error("CHANGE_DETAIL_FAILED", error), Some(&project));
        }
    };
    match crate::psql::approve_change(pool, &memory_uuid, None).await {
        Ok(true) => {
            spawn_embedding_refresh(pool.clone(), memory_uuid, change);
            (
                StatusCode::OK,
                api_response(Some(Value::Null), None, Some(&project)),
            )
        }
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

// What：approve 提交后在后台任务里生成并写入 embedding，不阻塞审批响应。
// Why：分块 embedding 在慢机器上需数分钟，同步等待会超过网关与 Cloudflare 的超时上限，
// 断连还会连带取消审批本身；向量短暂滞后只影响保底语义召回，可接受。
fn spawn_embedding_refresh(pool: sqlx::PgPool, memory_uuid: String, change: Value) {
    tokio::spawn(async move {
        let embedding = match embedding_for_approve(&pool, &memory_uuid, &change).await {
            Ok(Some(embedding)) => embedding,
            Ok(None) => return,
            Err(error) => {
                eprintln!(
                    "embedding refresh skipped for {memory_uuid}: {} {}",
                    error.code, error.message
                );
                return;
            }
        };
        if let Err(error) = crate::psql::refresh_memory_embedding(&pool, &memory_uuid, embedding).await {
            eprintln!("embedding refresh write failed for {memory_uuid}: {error}");
        }
    });
}

// Why：reject 只接受 memory_uuid，回滚依据必须来自后端锁定的 before_state。
pub async fn reject(
    Path(memory_uuid): Path<String>,
    State(state): State<super::AppState>,
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
    let Some(pool) = state.pool_for(&project, &url) else {
        return error_response(
            ApiError {
                code: "DATABASE_POOL_UNAVAILABLE",
                message: "database pool is unavailable; restart the server after changing database configuration".to_string(),
            },
            Some(&project),
        );
    };
    match crate::psql::reject_change(pool, &memory_uuid).await {
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
    pool: &sqlx::PgPool,
    memory_uuid: &str,
    change: &Value,
) -> Result<Option<crate::psql::ApprovedEmbedding>, ApiError> {
    // What：在 approve 提交后从当前 memory_units 工作态生成 embedding。
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
    Ok(Some(
        generate_embedding_for_memory(pool, memory_uuid, &settings).await?,
    ))
}

// What：按本库参数登记生成目标，生成完整块集合并携带起始表 OID。
// Why：发布端须识别生成期间的换表；目标标记仍阻止同一代内的旧任务覆盖。
pub(crate) async fn generate_embedding_for_memory(
    pool: &sqlx::PgPool,
    memory_uuid: &str,
    settings: &crate::config::EmbeddingSettings,
) -> Result<crate::psql::ApprovedEmbedding, ApiError> {
    static RECENT: tokio::sync::Mutex<std::collections::VecDeque<(std::time::Instant, u32)>> =
        tokio::sync::Mutex::const_new(std::collections::VecDeque::new());
    let (table_oid, model, dimension, fallback_max_distance) = crate::psql::read_current_embedding_config(pool).await
        .map_err(|error| db_error("EMBEDDING_REFRESH_FAILED", error.into()))?;
    // Why：模型或维度已变化时，不能将文件新参数与调用方旧 API 参数拼成请求。
    if settings.model != model || i32::try_from(settings.dimension).ok() != Some(dimension) {
        return Err(ApiError { code: "EMBEDDING_REFRESH_FAILED", message: "embedding 调用配置与文件配置不一致，请重新读取配置后重试".into() });
    }
    let settings = crate::config::EmbeddingSettings {
        api: settings.api.clone(),
        api_type: settings.api_type.clone(),
        key: settings.key.clone(),
        model,
        dimension: usize::try_from(dimension).map_err(|error| db_error("EMBEDDING_REFRESH_FAILED", error.into()))?,
        fallback_max_distance,
        proxy: settings.proxy.clone(),
    };
    if settings.model != "BAAI/bge-m3" || !(1..=2000).contains(&settings.dimension) {
        return Err(ApiError { code: "EMBEDDING_REFRESH_FAILED", message: "正常分块仅支持 BAAI/bge-m3，维度须为 1..=2000".into() });
    }
    let tokenizer = crate::provider::embedding::load_bge_m3_tokenizer(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models/bge-m3/tokenizer.json"),
    ).map_err(|error| db_error("EMBEDDING_REFRESH_FAILED", error))?;
    let mut source_state = load_current_memory_state(pool, memory_uuid).await?;
    let (content, metadata) = embedding_input_from_state(&source_state)?;
    let revision = source_state.pointer("/memory/revision").and_then(Value::as_i64)
        .filter(|revision| *revision > 0)
        .ok_or_else(|| ApiError { code: "EMBEDDING_REFRESH_FAILED", message: "源快照缺少有效版本".into() })?;
    let policy = "metadata-v1:heading-path-v1:full-input-8192:tokenizer-21106b6d7dab2952c1d496fb21d5dc9db75c28ed361a05f5020bbba27810dd08";
    let generation = crate::psql::register_embedding_target(
        pool, memory_uuid, revision, &settings.model, settings.dimension as i32, policy,
    ).await.map_err(|error| db_error("EMBEDDING_REFRESH_FAILED", error.into()))?;
    let mut recent = RECENT.lock().await;
    let chunks = crate::provider::embedding::generate_chunks(
        content, &metadata, &settings, &tokenizer, &mut recent, 2000, 500000,
    ).await
        .map_err(|error| db_error("EMBEDDING_REFRESH_FAILED", error))?;
    source_state["embedding_table_oid"] = serde_json::json!(table_oid);
    source_state["embedding_target"] = serde_json::json!({
        "generation": generation, "model": settings.model, "dimension": settings.dimension, "policy": policy,
    });
    Ok(crate::psql::ApprovedEmbedding {
        model: settings.model.clone(),
        dimension: settings.dimension as i32,
        values: crate::provider::embedding::EmbeddingValues::Chunks(chunks),
        source_state: source_state.to_string(),
    })
}

async fn load_current_memory_state(
    pool: &sqlx::PgPool,
    memory_uuid: &str,
) -> Result<Value, ApiError> {
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

fn embedding_input_from_state(state: &Value) -> Result<(&str, String), ApiError> {
    // What：从同一源快照提取原始正文，以及标题、摘要、召回场景和关键词元信息。
    // Why：正文必须独立保留原始字节，逐块生成的范围才能准确指向源文而不混入元信息。
    let state = state.as_object().ok_or(ApiError {
        code: "EMBEDDING_INPUT_MISSING",
        message: "memory state is invalid".to_string(),
    })?;
    let memory = state.get("memory").ok_or(ApiError {
        code: "EMBEDDING_INPUT_MISSING",
        message: "memory state.memory is missing".to_string(),
    })?;
    let content = memory.get("content").and_then(Value::as_str).ok_or(ApiError {
        code: "EMBEDDING_INPUT_MISSING",
        message: "memory state.memory.content is missing or invalid".to_string(),
    })?;
    let mut parts = ["title_norm", "summary", "recall_when"]
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
    Ok((content, parts.join("\n")))
}

fn reviewed_change(change: &Value) -> Option<(&str, &str)> {
    // Why：审核派生流程只需要动作和目标 uuid，不应让 HTTP 层重复解析完整状态机。
    Some((
        change.get("memory_uuid")?.as_str()?,
        change.get("action")?.as_str()?,
    ))
}

// Why：详情读取需要区分不存在和数据库失败，前端才能展示正确状态。
async fn load_change_detail(
    pool: &sqlx::PgPool,
    uuid: &str,
) -> Result<Option<Value>, ApiError> {
    crate::psql::get_change(pool, uuid)
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
