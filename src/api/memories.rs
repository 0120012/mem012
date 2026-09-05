use axum::{
    Json,
    extract::{
        Path, Query,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;

use super::utils::{ApiError, api_response, require_project};

const DEFAULT_MEMORY_LIMIT: i64 = 12;
const MAX_MEMORY_LIMIT: i64 = 100;

#[derive(serde::Deserialize)]
pub struct MemoryListQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    category: Option<String>,
    filter: Option<String>,
    date_field: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
}

// Why：先让记忆列表入口经过统一门禁，避免后续真实查询绕过 session 和 project 白名单。
pub async fn list(
    query: Result<Query<MemoryListQuery>, QueryRejection>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                api_response(
                    None,
                    Some(ApiError {
                        code: "INVALID_PAGINATION",
                        message: error.to_string(),
                    }),
                    None,
                ),
            );
        }
    };
    let category = memory_list_category(&query);
    let filter = memory_list_text_filter(&query);
    let (date_field, date_from, date_to) = match memory_list_dates(&query) {
        Ok(dates) => dates,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                api_response(None, Some(error), None),
            );
        }
    };
    let (limit, offset) = match memory_list_pagination(&query) {
        Ok(pagination) => pagination,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                api_response(None, Some(error), None),
            );
        }
    };
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

    let data = match load_memory_data(
        &project,
        limit,
        offset,
        category,
        filter.as_deref(),
        date_field,
        date_from,
        date_to,
    )
    .await
    {
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

// What：接收前端的新建记忆请求，并返回待审核记忆的 UUID。
// Why：HTTP 层只负责鉴权、上下文和响应包装，创建规则统一交给现有 create_memory 工具。
pub async fn create(
    headers: HeaderMap,
    payload: Result<Json<Value>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                api_response(
                    None,
                    Some(ApiError {
                        code: "BAD_REQUEST",
                        message: error.to_string(),
                    }),
                    None,
                ),
            );
        }
    };
    let project = match require_project(&headers) {
        Ok(project) => project,
        Err(error) => {
            return (
                memory_error_status(error.code),
                api_response(None, Some(error), None),
            );
        }
    };
    match create_memory_data(&project, payload).await {
        Ok(memory_uuid) => (
            StatusCode::OK,
            api_response(
                Some(serde_json::json!({ "memory_uuid": memory_uuid, "result": "pending" })),
                None,
                Some(&project),
            ),
        ),
        Err(error) => (
            memory_error_status(error.code),
            api_response(None, Some(error), Some(&project)),
        ),
    }
}

// What：按 project 和 category 返回侧栏可展开的关键词列表。
// Why：关键词层级必须由用户点击 category 后懒加载，避免初始侧栏一次性读取完整记忆树。
pub async fn category_keywords(
    Path(category): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let category = category.trim();
    if category.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            api_response(
                None,
                Some(ApiError {
                    code: "CATEGORY_REQUIRED",
                    message: "category is required".to_string(),
                }),
                None,
            ),
        );
    }
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
    match load_memory_category_keywords(&project, category).await {
        Ok(data) => (
            StatusCode::OK,
            api_response(Some(data), None, Some(&project)),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_response(None, Some(error), Some(&project)),
        ),
    }
}

pub async fn update(
    Path(memory_uuid): Path<String>,
    headers: HeaderMap,
    payload: Result<Json<crate::psql::MemoryUpdateInput>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                api_response(
                    None,
                    Some(ApiError {
                        code: "BAD_REQUEST",
                        message: error.to_string(),
                    }),
                    None,
                ),
            );
        }
    };
    if !is_uuid_text(&memory_uuid) {
        return (
            StatusCode::BAD_REQUEST,
            api_response(
                None,
                Some(ApiError {
                    code: "BAD_REQUEST",
                    message: "memory_uuid is invalid".to_string(),
                }),
                None,
            ),
        );
    }
    let project = match require_project(&headers) {
        Ok(project) => project,
        Err(error) => {
            return (
                memory_error_status(error.code),
                api_response(None, Some(error), None),
            );
        }
    };
    match update_memory_data(&project, &memory_uuid, payload).await {
        Ok(true) => (
            StatusCode::OK,
            api_response(Some(Value::Null), None, Some(&project)),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            api_response(
                None,
                Some(ApiError {
                    code: "MEMORY_NOT_EDITABLE",
                    message: "memory is not editable".to_string(),
                }),
                Some(&project),
            ),
        ),
        Err(error) => (
            memory_error_status(error.code),
            api_response(None, Some(error), Some(&project)),
        ),
    }
}

// Why：handler 只按 project 选择数据源，真实列表 SQL 必须留在 psql 层统一维护。
async fn load_memory_data(
    project: &str,
    limit: i64,
    offset: i64,
    category: Option<&str>,
    filter: Option<&str>,
    date_field: Option<&str>,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<Value, ApiError> {
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "CONFIG_LOAD_FAILED",
        message: error.to_string(),
    })?;
    let database_url = config.database_url(project).ok_or(ApiError {
        code: "PROJECT_NOT_FOUND",
        message: "project is not configured".to_string(),
    })?;
    crate::psql::list_memories(
        database_url,
        limit,
        offset,
        category,
        filter,
        date_field,
        date_from,
        date_to,
    )
    .await
    .map_err(|error| ApiError {
        code: "MEMORY_LIST_FAILED",
        message: error.to_string(),
    })
}

fn memory_list_category(query: &MemoryListQuery) -> Option<&str> {
    query
        .category
        .as_deref()
        .map(str::trim)
        .filter(|category| !category.is_empty())
}

fn memory_list_text_filter(query: &MemoryListQuery) -> Option<String> {
    let filter = query
        .filter
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let escaped = filter
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    Some(format!("%{escaped}%"))
}

fn memory_list_dates(
    query: &MemoryListQuery,
) -> Result<(Option<&str>, Option<&str>, Option<&str>), ApiError> {
    let date_field = match query
        .date_field
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some("created_at") => Some("created_at"),
        Some("updated_at") => Some("updated_at"),
        Some(_) => return Err(invalid_date_filter("date_field is invalid")),
    };
    let date_from = validate_memory_date(query.date_from.as_deref(), "date_from")?;
    let date_to = validate_memory_date(query.date_to.as_deref(), "date_to")?;
    if let (Some(date_from), Some(date_to)) = (date_from, date_to) {
        if date_from > date_to {
            return Err(invalid_date_filter(
                "date_from must not be later than date_to",
            ));
        }
    }
    let date_field = if date_from.is_some() || date_to.is_some() {
        Some(date_field.unwrap_or("updated_at"))
    } else {
        date_field
    };
    Ok((date_field, date_from, date_to))
}

fn validate_memory_date<'a>(
    value: Option<&'a str>,
    field: &str,
) -> Result<Option<&'a str>, ApiError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, byte)| matches!(i, 4 | 7) || byte.is_ascii_digit())
    {
        return Err(invalid_date_filter(format!("{field} must use YYYY-MM-DD")));
    }
    let year = value[0..4]
        .parse::<i32>()
        .map_err(|_| invalid_date_filter(format!("{field} is invalid")))?;
    let month = value[5..7]
        .parse::<u32>()
        .map_err(|_| invalid_date_filter(format!("{field} is invalid")))?;
    let day = value[8..10]
        .parse::<u32>()
        .map_err(|_| invalid_date_filter(format!("{field} is invalid")))?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if year < 1 || day == 0 || day > days {
        return Err(invalid_date_filter(format!("{field} is invalid")));
    }
    Ok(Some(value))
}

fn invalid_date_filter(message: impl Into<String>) -> ApiError {
    ApiError {
        code: "INVALID_DATE_FILTER",
        message: message.into(),
    }
}

fn memory_list_pagination(query: &MemoryListQuery) -> Result<(i64, i64), ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_MEMORY_LIMIT);
    let offset = query.offset.unwrap_or(0);
    if !(1..=MAX_MEMORY_LIMIT).contains(&limit) || offset < 0 {
        return Err(ApiError {
            code: "INVALID_PAGINATION",
            message: "limit must be between 1 and 100 and offset must not be negative".to_string(),
        });
    }
    Ok((limit, offset))
}

async fn create_memory_data(project: &str, payload: Value) -> Result<String, ApiError> {
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "CONFIG_LOAD_FAILED",
        message: error.to_string(),
    })?;
    let database_url = config.database_url(project).ok_or(ApiError {
        code: "PROJECT_NOT_FOUND",
        message: "project is not configured".to_string(),
    })?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .map_err(|error| ApiError {
            code: "DATABASE_CONNECT_FAILED",
            message: error.to_string(),
        })?;
    let api_base_url =
        crate::config::local_api_base_url(config.client_base_url().unwrap_or(config.server_addr()))
            .map_err(|error| ApiError {
                code: "CONFIG_LOAD_FAILED",
                message: error.to_string(),
            })?;
    let context = crate::tools::ToolContext {
        profile: project,
        profile_pool: &pool,
        search_default_limit: config.search_default_limit(),
        category_index_list: config.category_index_list(),
        api_base_url: &api_base_url,
        embedding_settings: None,
        embeddings_dimension: config.embeddings_dimension(),
        rerank_settings: None,
        reset_db: false,
    };
    crate::tools::create_memory_for_api(&context, &payload)
        .await
        .map_err(create_memory_error)
}

async fn load_memory_category_keywords(project: &str, category: &str) -> Result<Value, ApiError> {
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "CONFIG_LOAD_FAILED",
        message: error.to_string(),
    })?;
    let database_url = config.database_url(project).ok_or(ApiError {
        code: "PROJECT_NOT_FOUND",
        message: "project is not configured".to_string(),
    })?;
    crate::psql::list_memory_category_keywords(database_url, category)
        .await
        .map_err(|error| ApiError {
            code: "MEMORY_CATEGORY_KEYWORDS_FAILED",
            message: error.to_string(),
        })
}

async fn update_memory_data(
    project: &str,
    memory_uuid: &str,
    input: crate::psql::MemoryUpdateInput,
) -> Result<bool, ApiError> {
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "CONFIG_LOAD_FAILED",
        message: error.to_string(),
    })?;
    let database_url = config.database_url(project).ok_or(ApiError {
        code: "PROJECT_NOT_FOUND",
        message: "project is not configured".to_string(),
    })?;
    crate::psql::update_memory(database_url, memory_uuid, input)
        .await
        .map_err(|error| {
            let message = error.to_string();
            ApiError {
                code: if message.starts_with("MEMORY_UPDATE_INVALID:") {
                    "BAD_REQUEST"
                } else if message.starts_with("MEMORY_UPDATE_CONFLICT:") {
                    "MEMORY_UPDATE_CONFLICT"
                } else {
                    "MEMORY_UPDATE_FAILED"
                },
                message,
            }
        })
}

fn memory_error_status(code: &str) -> StatusCode {
    match code {
        "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
        "PROJECT_REQUIRED" | "BAD_REQUEST" => StatusCode::BAD_REQUEST,
        "PROJECT_NOT_FOUND" => StatusCode::NOT_FOUND,
        "MEMORY_UPDATE_CONFLICT" => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn create_memory_error(error: Box<dyn std::error::Error>) -> ApiError {
    let message = error.to_string();
    let input_error = message.starts_with("DUPLICATE_")
        || message.contains("不能")
        || message.contains("只能")
        || message.contains("必须")
        || message.contains("category 不在 categories.index_list 中")
        || message.contains("invalid type")
        || message.contains("expected")
        || message.contains("missing field")
        || message.contains("unknown field")
        || message.contains("required")
        || message.contains("duplicated");
    ApiError {
        code: if input_error {
            "BAD_REQUEST"
        } else {
            "MEMORY_CREATE_FAILED"
        },
        message,
    }
}

fn is_uuid_text(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use super::{
        MemoryListQuery, create_memory_error, is_uuid_text, memory_list_dates,
        memory_list_pagination, memory_list_text_filter,
    };

    fn empty_memory_list_query() -> MemoryListQuery {
        MemoryListQuery {
            limit: None,
            offset: None,
            category: None,
            filter: None,
            date_field: None,
            date_from: None,
            date_to: None,
        }
    }

    #[test]
    fn memory_list_defaults_to_twelve_items() {
        assert_eq!(
            memory_list_pagination(&empty_memory_list_query()).ok(),
            Some((12, 0))
        );
    }

    #[test]
    fn memory_list_rejects_invalid_pagination() {
        let mut query = empty_memory_list_query();
        query.limit = Some(0);
        assert_eq!(
            memory_list_pagination(&query).unwrap_err().code,
            "INVALID_PAGINATION"
        );
        query.limit = Some(12);
        query.offset = Some(-1);
        assert_eq!(
            memory_list_pagination(&query).unwrap_err().code,
            "INVALID_PAGINATION"
        );
    }

    #[test]
    fn memory_list_text_filter_escapes_like_tokens() {
        let mut query = empty_memory_list_query();
        query.filter = Some(r"  50%_\  ".to_string());
        assert_eq!(
            memory_list_text_filter(&query),
            Some("%50\\%\\_\\\\%".to_string())
        );
    }

    #[test]
    fn memory_list_dates_validates_range_and_field() {
        let mut query = empty_memory_list_query();
        query.date_field = Some("created_at".to_string());
        query.date_from = Some("2024-02-29".to_string());
        query.date_to = Some("2024-03-01".to_string());
        assert_eq!(
            memory_list_dates(&query).ok(),
            Some((Some("created_at"), Some("2024-02-29"), Some("2024-03-01")))
        );
        query.date_to = Some("2024-02-28".to_string());
        assert_eq!(
            memory_list_dates(&query).unwrap_err().code,
            "INVALID_DATE_FILTER"
        );
        query.date_to = Some("2024-02-30".to_string());
        assert_eq!(
            memory_list_dates(&query).unwrap_err().code,
            "INVALID_DATE_FILTER"
        );
    }

    #[test]
    fn unconfigured_category_is_bad_request() {
        let error = create_memory_error(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "category 不在 categories.index_list 中",
        )));
        assert_eq!(error.code, "BAD_REQUEST");
    }

    #[test]
    fn uuid_text_rejects_invalid_path_values() {
        assert!(is_uuid_text("00000000-0000-0000-0000-000000000001"));
        for value in [
            "",
            "not-a-uuid",
            "00000000000000000000000000000001",
            "00000000-0000-0000-0000-00000000000z",
        ] {
            assert!(!is_uuid_text(value), "{value}");
        }
    }
}
