use axum::{Json, http::StatusCode};
use serde_json::{Value, json};

use super::utils::{ApiError, api_response};

// Why：项目列表必须来自配置白名单，避免前端枚举或伪造任意数据库名。
pub async fn list() -> (StatusCode, Json<Value>) {
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
    let projects = ["riko", "herm", "doge", "share"]
        .into_iter()
        .filter_map(|project_id| {
            let database_url = config.database_url(project_id)?;
            let database_name = database_url.split('?').next()?.rsplit('/').next()?;
            Some(json!({
                "project_id": project_id,
                "display_name": project_id,
                "database_name": database_name,
                "db_scope": if project_id == "share" { "share" } else { "profile" },
                "is_share": project_id == "share"
            }))
        })
        .collect::<Vec<_>>();

    let response = api_response(Some(json!(projects)), None, None);
    (StatusCode::OK, response)
}
