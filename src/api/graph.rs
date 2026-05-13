use axum::{
    Json,
    extract::{Path, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::Value;

use super::utils::{ApiError, api_response, require_project};

#[derive(Deserialize)]
pub struct AddRelationRequest {
    from_memory_uuid: String,
    to_memory_uuid: String,
    relation_type: String,
    weight: Option<i32>,
    note: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateRelationRequest {
    relation_type: Option<String>,
    weight: Option<i32>,
    note: Option<String>,
}

// Why：graph status 是 rebuild 前置判断，HTTP 入口必须和 CLI 返回同一数据来源。
pub async fn status(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let (project, pool) = match project_pool(headers).await {
        Ok(value) => value,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::get_memory_graph_status(&pool).await {
        Ok(data) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Err(error) => error_response(db_error("GRAPH_STATUS_FAILED", error), Some(&project)),
    }
}

// Why：rebuild 是显式操作，避免 approve 时把 AGE 失败混入用户确认流程。
pub async fn rebuild(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let (project, pool) = match project_pool(headers).await {
        Ok(value) => value,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::rebuild_memory_graph(&pool).await {
        Ok(()) => (
            StatusCode::OK,
            api_response(
                Some(serde_json::json!({ "graph": "memory_graph" })),
                None,
                Some(&project),
            ),
        ),
        Err(error) => error_response(
            ApiError {
                code: "GRAPH_REBUILD_FAILED",
                message: error.to_string(),
            },
            Some(&project),
        ),
    }
}

// Why：一跳邻居来自 SQL 主数据，dirty 状态下仍可作为图页的稳定降级查询。
pub async fn neighbors(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let (project, pool) = match project_pool(headers).await {
        Ok(value) => value,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::list_memory_neighbors(&pool, &memory_uuid).await {
        Ok(data) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Err(error) => error_response(db_error("GRAPH_NEIGHBORS_FAILED", error), Some(&project)),
    }
}

// Why：图谱页默认打开时需要稳定数据源，不能依赖用户先输入某个 memory_uuid。
pub async fn overview(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    let (project, pool) = match project_pool(headers).await {
        Ok(value) => value,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::list_memory_graph_overview(&pool).await {
        Ok(data) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Err(error) => error_response(db_error("GRAPH_OVERVIEW_FAILED", error), Some(&project)),
    }
}

// Why：HTTP relation 写入给前端使用，但仍复用 CLI 的 psql 工作态和 pending change 规则。
pub async fn add_relation(
    headers: HeaderMap,
    payload: Result<Json<AddRelationRequest>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            return error_response(
                ApiError {
                    code: "BAD_REQUEST",
                    message: error.to_string(),
                },
                None,
            );
        }
    };
    let (project, pool) = match project_pool(headers).await {
        Ok(value) => value,
        Err((status, body)) => return (status, body),
    };
    let input = crate::psql::RelationCreate {
        from_memory_uuid: payload.from_memory_uuid,
        to_memory_uuid: payload.to_memory_uuid,
        relation_type: payload.relation_type,
        weight: payload.weight,
        note: payload.note,
    };
    match crate::psql::add_memory_relation(&pool, input).await {
        Ok(data) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Err(error) => error_response(db_error("RELATION_ADD_FAILED", error), Some(&project)),
    }
}

// Why：relation 修正必须由 relation_uuid 定位，避免前端重新提交两端 memory 造成歧义。
pub async fn update_relation(
    Path(relation_uuid): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<UpdateRelationRequest>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => return bad_request(error),
    };
    let (project, pool) = match project_pool(headers).await {
        Ok(value) => value,
        Err((status, body)) => return (status, body),
    };
    let patch = crate::psql::RelationUpdate {
        relation_type: payload.relation_type,
        weight: payload.weight,
        note: payload.note,
    };
    match crate::psql::update_memory_relation(&pool, &relation_uuid, patch).await {
        Ok(Some(data)) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Ok(None) => relation_not_found(&project),
        Err(error) => error_response(db_error("RELATION_UPDATE_FAILED", error), Some(&project)),
    }
}

// Why：删除 relation 只接收 uuid，实际回滚基线必须从数据库锁定后的 before_state 得到。
pub async fn delete_relation(
    Path(relation_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let (project, pool) = match project_pool(headers).await {
        Ok(value) => value,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::delete_memory_relation(&pool, &relation_uuid).await {
        Ok(true) => (
            StatusCode::OK,
            api_response(
                Some(serde_json::json!({ "deleted": true })),
                None,
                Some(&project),
            ),
        ),
        Ok(false) => relation_not_found(&project),
        Err(error) => error_response(db_error("RELATION_DELETE_FAILED", error), Some(&project)),
    }
}

// Why：候选接口只读，不替用户建边，前端可以把候选再交给用户确认。
pub async fn suggest_relations(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let (project, pool) = match project_pool(headers).await {
        Ok(value) => value,
        Err((status, body)) => return (status, body),
    };
    match crate::psql::suggest_memory_relations(&pool, &memory_uuid).await {
        Ok(data) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Err(error) => error_response(db_error("RELATION_SUGGEST_FAILED", error), Some(&project)),
    }
}

async fn project_pool(
    headers: HeaderMap,
) -> Result<(String, sqlx::Pool<sqlx::Postgres>), (StatusCode, Json<Value>)> {
    let project = require_project(&headers).map_err(|error| error_response(error, None))?;
    let url = database_url(&project).map_err(|error| error_response(error, Some(&project)))?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(|error| {
            error_response(
                ApiError {
                    code: "DATABASE_CONNECT_FAILED",
                    message: error.to_string(),
                },
                Some(&project),
            )
        })?;
    Ok((project, pool))
}

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

fn error_response(error: ApiError, project: Option<&str>) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
        "PROJECT_REQUIRED" => StatusCode::BAD_REQUEST,
        "PROJECT_NOT_FOUND" | "RELATION_NOT_FOUND" => StatusCode::NOT_FOUND,
        "BAD_REQUEST" => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, api_response(None, Some(error), project))
}

fn bad_request(error: JsonRejection) -> (StatusCode, Json<Value>) {
    error_response(
        ApiError {
            code: "BAD_REQUEST",
            message: error.to_string(),
        },
        None,
    )
}

fn relation_not_found(project: &str) -> (StatusCode, Json<Value>) {
    error_response(
        ApiError {
            code: "RELATION_NOT_FOUND",
            message: "relation is not found".to_string(),
        },
        Some(project),
    )
}

fn db_error(code: &'static str, error: Box<dyn std::error::Error + Send + Sync>) -> ApiError {
    ApiError {
        code,
        message: error.to_string(),
    }
}
