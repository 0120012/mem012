use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use super::utils::{ApiError, api_response};

const SESSION_COOKIE: &str = "mem_session";
type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
pub struct VerifyRequest {
    key: String,
}

// Why：登录入口必须接收用户输入的密钥，并用 HttpOnly cookie 隔离前端脚本和长期凭证。
pub async fn verify(
    payload: Result<Json<VerifyRequest>, JsonRejection>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            return auth_response(StatusCode::BAD_REQUEST, "BAD_REQUEST", error.to_string());
        }
    };
    let expected = match expected_token() {
        Ok(expected) => expected,
        Err(error) => {
            return auth_response(StatusCode::INTERNAL_SERVER_ERROR, error.code, error.message);
        }
    };

    if payload.key != expected {
        return auth_response(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "invalid API token".to_string(),
        );
    }

    let headers = session_headers(&expected);
    (
        StatusCode::OK,
        headers,
        api_response(Some(json!({ "authenticated": true })), None, None),
    )
}

// Why：session 检查只能信任后端签发的 cookie，不能继续要求前端携带原始密钥。
pub async fn session(headers: HeaderMap) -> (StatusCode, Json<Value>) {
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

    (
        StatusCode::OK,
        api_response(Some(json!({ "authenticated": true })), None, None),
    )
}

// Why：非 auth API 只需要布尔认证结果，不能复制 cookie 和签名校验细节。
pub fn has_valid_session(headers: &HeaderMap) -> Result<bool, ApiError> {
    let expected = expected_token()?;
    Ok(
        cookie_value(headers, SESSION_COOKIE)
            .is_some_and(|value| value == session_token(&expected)),
    )
}

// Why：session 和 verify 必须共享同一个失败编码，前端才能稳定判断认证失败。
pub fn unauthorized_error() -> ApiError {
    ApiError {
        code: "UNAUTHORIZED",
        message: "invalid API token".to_string(),
    }
}

// Why：认证失败需要和成功路径共用响应壳，同时 verify 还要能附加 Set-Cookie。
fn auth_response(
    status: StatusCode,
    code: &'static str,
    message: String,
) -> (StatusCode, HeaderMap, Json<Value>) {
    let error = ApiError { code, message };
    (
        status,
        HeaderMap::new(),
        api_response(None, Some(error), None),
    )
}

// Why：cookie 属性集中生成，避免登录成功路径散落浏览器安全策略细节。
fn session_headers(secret: &str) -> HeaderMap {
    let cookie = format!(
        "{SESSION_COOKIE}={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=604800",
        session_token(secret)
    );
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    headers
}

// Why：缺失或空密钥是服务端配置错误，不能降级成无认证模式。
fn expected_token() -> Result<String, ApiError> {
    let config = crate::config::load_config("config.toml").map_err(|error| ApiError {
        code: "AUTH_CONFIG_MISSING",
        message: error.to_string(),
    })?;
    config.api_token().map(str::to_string).ok_or(ApiError {
        code: "AUTH_CONFIG_EMPTY",
        message: "server.api_token is empty".to_string(),
    })
}

// Why：cookie 内只放签名后的 session 标记，避免把 API_TOKEN 本身发回浏览器。
fn session_token(secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(b"mem012-session-v1");
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

// Why：Cookie 头是分号分隔的扁平文本，集中解析可以避免各 handler 重复拆字符串。
fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| part.trim().split_once('='))
        .and_then(|(key, value)| (key == name).then_some(value))
}
