use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey};
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use super::utils::{ApiError, api_response};

const SESSION_COOKIE: &str = "mem_session";
const INIT_AUTH_TOKEN_TTL: Duration = Duration::from_secs(180);
const INIT_GRANT_TTL: Duration = Duration::from_secs(300);
const INIT_GRANT_SCOPE: &str = "init:create";
type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
struct InitAuthToken {
    token: String,
    expires_at: u64,
}

#[derive(Default)]
struct InitAuthStore {
    token: Option<InitAuthToken>,
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    key: String,
}

#[derive(Deserialize)]
pub struct InitTokenRefreshRequest {
    turnstile_token: String,
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

// What：返回 init 授权 token 的当前状态，不生成新 token，也不返回 token 明文。
// Why：/auth 页面轮询只能判断旧 token 是否仍有效，真正展示 token 必须先通过 Turnstile refresh。
pub async fn init_token_status(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    match has_valid_session(&headers) {
        Ok(true) => (
            StatusCode::OK,
            api_response(
                Some(
                    match init_auth_store().lock().unwrap().token_status(now_epoch()) {
                        Some(expires_at) => json!({ "valid": true, "expires_at": expires_at }),
                        None => json!({ "valid": false, "expires_at": Value::Null }),
                    },
                ),
                None,
                None,
            ),
        ),
        Ok(false) => (
            StatusCode::UNAUTHORIZED,
            api_response(None, Some(unauthorized_error()), None),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_response(None, Some(error), None),
        ),
    }
}

// What：接收 /auth 页面提交的 Turnstile token，并按官方 siteverify 完成后端校验。
// Why：init auth_token 只能在验证码通过后生成；这一层先锁住验证码边界，再接入 token 状态。
pub async fn init_token_refresh(
    headers: HeaderMap,
    payload: Result<Json<InitTokenRefreshRequest>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    if let Err(error) =
        has_valid_session(&headers).and_then(|ok| ok.then_some(()).ok_or_else(unauthorized_error))
    {
        return (
            StatusCode::UNAUTHORIZED,
            api_response(None, Some(error), None),
        );
    }
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
    let settings = match crate::config::load_config("config.toml")
        .ok()
        .and_then(|config| config.turnstile_settings())
    {
        Some(settings) => settings,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_response(
                    None,
                    Some(ApiError {
                        code: "TURNSTILE_CONFIG_MISSING",
                        message: "turnstile is not configured".to_string(),
                    }),
                    None,
                ),
            );
        }
    };
    if let Err(error) =
        super::turnstile::verify_token(&settings, &payload.turnstile_token, None).await
    {
        return (
            StatusCode::BAD_REQUEST,
            api_response(None, Some(error), None),
        );
    }
    let token = init_auth_store()
        .lock()
        .unwrap()
        .refresh_token(now_epoch(), INIT_AUTH_TOKEN_TTL);
    (
        StatusCode::OK,
        api_response(
            Some(json!({ "auth_token": token.token, "expires_at": token.expires_at })),
            None,
            None,
        ),
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

// Why：当前服务没有 TLS 终止信息，强制 Secure 会让 HTTP/VPS 登录后浏览器丢弃 session。
fn session_headers(secret: &str) -> HeaderMap {
    let cookie = format!(
        "{SESSION_COOKIE}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=604800",
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

fn random_base64_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

// What：生成带 Ed25519 签名的 init:create grant JSON 票据。
// Why：CLI 后续只保存一次性 grant，不能保存可长期复用的前端 auth_token。
fn signed_init_grant(signing_key: &SigningKey, now: u64) -> Value {
    let payload = json!({
        "grant_id": random_base64_token(),
        "scope": INIT_GRANT_SCOPE,
        "iat": now,
        "exp": now.saturating_add(INIT_GRANT_TTL.as_secs()),
        "nonce": random_base64_token(),
    });
    let payload_bytes = serde_json::to_vec(&payload).expect("grant payload serializes");
    let signature = signing_key.sign(&payload_bytes);
    json!({
        "version": 1,
        "payload": payload,
        "signature": URL_SAFE_NO_PAD.encode(signature.to_bytes()),
    })
}

fn init_auth_store() -> &'static Mutex<InitAuthStore> {
    static STORE: OnceLock<Mutex<InitAuthStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(InitAuthStore::default()))
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl InitAuthStore {
    fn refresh_token(&mut self, now: u64, ttl: Duration) -> InitAuthToken {
        let token = InitAuthToken {
            token: random_base64_token(),
            expires_at: now.saturating_add(ttl.as_secs()),
        };
        self.token = Some(token.clone());
        token
    }

    fn token_status(&mut self, now: u64) -> Option<u64> {
        let expires_at = self.token.as_ref()?.expires_at;
        if expires_at <= now {
            self.token = None;
            return None;
        }
        Some(expires_at)
    }

    fn consume_token(&mut self, presented_token: &str, now: u64) -> bool {
        let Some(token) = self.token.as_ref() else {
            return false;
        };
        if token.expires_at <= now {
            self.token = None;
            return false;
        }
        if token.token != presented_token {
            return false;
        }
        self.token = None;
        true
    }
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    use super::{InitAuthStore, URL_SAFE_NO_PAD, random_base64_token, signed_init_grant};

    #[test]
    fn random_base64_token_is_256_bit_url_safe() {
        let first = random_base64_token();
        let second = random_base64_token();

        assert_eq!(first.len(), 43);
        assert_ne!(first, second);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
    }

    #[test]
    fn init_auth_store_refreshes_and_expires_token() {
        let mut store = InitAuthStore::default();
        let first = store.refresh_token(100, Duration::from_secs(180));

        assert_eq!(store.token_status(100), Some(280));
        assert_eq!(first.expires_at, 280);

        let second = store.refresh_token(120, Duration::from_secs(180));
        assert_ne!(first.token, second.token);
        assert_eq!(store.token_status(299), Some(300));
        assert_eq!(store.token_status(300), None);
    }

    #[test]
    fn init_auth_store_consumes_only_valid_token_once() {
        let mut store = InitAuthStore::default();
        let token = store.refresh_token(100, Duration::from_secs(180)).token;

        assert!(!store.consume_token("wrong-token", 120));
        assert_eq!(store.token_status(120), Some(280));
        assert!(store.consume_token(&token, 120));
        assert!(!store.consume_token(&token, 120));
        assert_eq!(store.token_status(120), None);
    }

    #[test]
    fn signed_init_grant_has_verifiable_ed25519_signature() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
        let grant = signed_init_grant(&signing_key, 100);
        let payload = grant.get("payload").unwrap();
        let signature_bytes = URL_SAFE_NO_PAD
            .decode(grant.get("signature").unwrap().as_str().unwrap())
            .unwrap();
        let signature = Signature::from_slice(&signature_bytes).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);

        verifying_key
            .verify(&serde_json::to_vec(payload).unwrap(), &signature)
            .unwrap();
        assert_eq!(grant.get("version").unwrap().as_u64(), Some(1));
        assert_eq!(payload.get("scope").unwrap().as_str(), Some("init:create"));
        assert_eq!(payload.get("exp").unwrap().as_u64(), Some(400));
    }
}
