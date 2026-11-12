use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};

use super::auth::{has_valid_session, unauthorized_error};
use super::utils::{ApiError, api_response};

// Why：项目列表必须来自配置白名单，避免前端枚举或伪造任意数据库名。
pub async fn list(
    State(state): State<super::AppState>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    match has_valid_session(&headers) {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::UNAUTHORIZED,
                api_response(None, Some(unauthorized_error()), None),
            );
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_response(None, Some(error), None),
            );
        }
    }
    let config = match crate::config::load_config("config.toml") {
        Ok(config) => config,
        Err(error) => {
            let response = api_response(
                None,
                Some(ApiError {
                    code: "CONFIG_LOAD_FAILED",
                    message: error.to_string(),
                }),
                None,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, response);
        }
    };
    let projects = config
        .database_entries()
        .filter_map(|(project_id, database_url)| {
            state.pool_for(project_id, database_url)?;
            let categories = if project_id == "share" {
                vec!["share".to_string()]
            } else {
                config.category_index_list().to_vec()
            };
            Some(json!({
                "project_id": project_id,
                "display_name": project_id,
                "db_scope": if project_id == "share" { "share" } else { "profile" },
                "is_share": project_id == "share",
                "categories": categories
            }))
        })
        .collect::<Vec<_>>();

    let response = api_response(Some(json!(projects)), None, None);
    (StatusCode::OK, response)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::super::AppState;
    use super::list;

    #[tokio::test]
    async fn list_rejects_anonymous_request() {
        let (status, body) = list(axum::extract::State(AppState::default()), HeaderMap::new()).await;

        assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
        assert!(body["data"].is_null());
    }
}
