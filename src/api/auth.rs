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
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use super::utils::{ApiError, api_response};

const SESSION_COOKIE: &str = "mem_session";
const INIT_AUTH_TOKEN_TTL: Duration = Duration::from_secs(300);
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

#[derive(Clone)]
struct InitGrant {
    grant_id: String,
    scope: String,
    expires_at: u64,
    consumed: bool,
}

#[derive(Default)]
struct InitGrantStore {
    grant: Option<InitGrant>,
}

struct SignedInitGrant {
    grant: Value,
    grant_id: String,
    expires_at: u64,
}

struct VerifiedInitGrant {
    grant_id: String,
    expires_at: u64,
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    key: String,
}

#[derive(Deserialize)]
pub struct InitTokenRefreshRequest {
    turnstile_token: String,
}

#[derive(Deserialize)]
pub struct InitGrantRequest {
    auth_token: String,
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
pub async fn auth_status(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    match has_valid_session(&headers) {
        Ok(true) => {
            let site_key = crate::config::load_config("config.toml")
                .ok()
                .and_then(|config| config.turnstile_settings())
                .map(|settings| settings.site_key);
            (
                StatusCode::OK,
                api_response(
                    Some(
                        match init_auth_store().lock().unwrap().token_status(now_epoch()) {
                            Some(expires_at) => json!({
                                "valid": true,
                                "expires_at": expires_at,
                                "turnstile_site_key": site_key,
                            }),
                            None => json!({
                                "valid": false,
                                "expires_at": Value::Null,
                                "turnstile_site_key": site_key,
                            }),
                        },
                    ),
                    None,
                    None,
                ),
            )
        }
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
pub async fn auth_refresh(
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
    let _transition_guard = init_authorization_transition_lock().lock().unwrap();
    let token = init_auth_store()
        .lock()
        .unwrap()
        .refresh_token(now_epoch(), INIT_AUTH_TOKEN_TTL);
    init_grant_store().lock().unwrap().clear_grant();
    (
        StatusCode::OK,
        api_response(
            Some(json!({ "auth_token": token.token, "expires_at": token.expires_at })),
            None,
            None,
        ),
    )
}

// What：基于现有未过期授权状态轮换 auth_token，不重新触发 Turnstile。
// Why：用户已完成一次验证码后，需要能主动废弃旧 grant 并生成新 token，而不是反复做人机验证。
pub async fn auth_force_refresh(headers: HeaderMap) -> (StatusCode, Json<Value>) {
    if let Err(error) =
        has_valid_session(&headers).and_then(|ok| ok.then_some(()).ok_or_else(unauthorized_error))
    {
        return (
            StatusCode::UNAUTHORIZED,
            api_response(None, Some(error), None),
        );
    }
    let _transition_guard = init_authorization_transition_lock().lock().unwrap();
    match refresh_existing_init_authorization(now_epoch()) {
        Some(token) => (
            StatusCode::OK,
            api_response(
                Some(json!({ "auth_token": token.token, "expires_at": token.expires_at })),
                None,
                None,
            ),
        ),
        None => (
            StatusCode::UNAUTHORIZED,
            api_response(None, Some(unauthorized_error()), None),
        ),
    }
}

// What：用 300s auth_token 换取 300s Ed25519 init grant。
// Why：CLI 只能落盘一次性 grant，不能把前端 auth_token 写入本机授权文件。
pub async fn auth_grant(
    payload: Result<Json<InitGrantRequest>, JsonRejection>,
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
    let _transition_guard = init_authorization_transition_lock().lock().unwrap();
    let now = now_epoch();
    if !init_auth_store()
        .lock()
        .unwrap()
        .consume_token(&payload.auth_token, now)
    {
        return (
            StatusCode::UNAUTHORIZED,
            api_response(None, Some(unauthorized_error()), None),
        );
    }
    let signed = signed_init_grant(init_grant_signing_key(), now);
    init_grant_store().lock().unwrap().replace_grant(
        signed.grant_id,
        INIT_GRANT_SCOPE.to_string(),
        signed.expires_at,
    );
    (StatusCode::OK, api_response(Some(signed.grant), None, None))
}

// What：消费 auth file 中的 Ed25519 init grant。
// Why：init 写入授权必须一次性使用；无效、过期或重复消费都不能保留旧 grant。
pub async fn auth_grant_consume(
    payload: Result<Json<Value>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Json(grant) = match payload {
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
    let verified = match verify_init_grant(&grant, init_grant_signing_key()) {
        Ok(verified) => verified,
        Err(error) => {
            return (
                StatusCode::UNAUTHORIZED,
                api_response(None, Some(error), None),
            );
        }
    };
    let _transition_guard = init_authorization_transition_lock().lock().unwrap();
    let now = now_epoch();
    let (is_current_grant, consumed) = {
        let mut store = init_grant_store().lock().unwrap();
        let is_current = store.matches_grant(&verified.grant_id, INIT_GRANT_SCOPE);
        (
            is_current,
            store.consume_grant(&verified.grant_id, INIT_GRANT_SCOPE, now),
        )
    };
    if !consumed {
        if is_current_grant {
            init_auth_store().lock().unwrap().clear_token();
        }
        return (
            StatusCode::UNAUTHORIZED,
            api_response(None, Some(unauthorized_error()), None),
        );
    }
    clear_init_authorization_state();
    (
        StatusCode::OK,
        api_response(Some(json!({ "consumed": true })), None, None),
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

fn init_grant_signing_key() -> &'static SigningKey {
    static KEY: OnceLock<SigningKey> = OnceLock::new();
    KEY.get_or_init(|| {
        let mut bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut bytes);
        SigningKey::from_bytes(&bytes)
    })
}

// What：生成带 Ed25519 签名的 init:create grant JSON 票据。
// Why：CLI 后续只保存一次性 grant，不能保存可长期复用的前端 auth_token。
fn signed_init_grant(signing_key: &SigningKey, now: u64) -> SignedInitGrant {
    let grant_id = random_base64_token();
    let expires_at = now.saturating_add(INIT_GRANT_TTL.as_secs());
    let payload = json!({
        "grant_id": grant_id,
        "scope": INIT_GRANT_SCOPE,
        "iat": now,
        "exp": expires_at,
        "nonce": random_base64_token(),
    });
    let payload_bytes = serde_json::to_vec(&payload).expect("grant payload serializes");
    let signature = signing_key.sign(&payload_bytes);
    let grant = json!({
        "version": 1,
        "payload": payload,
        "signature": URL_SAFE_NO_PAD.encode(signature.to_bytes()),
    });
    SignedInitGrant {
        grant,
        grant_id,
        expires_at,
    }
}

// What：验证 Ed25519 grant JSON，并提取消费所需的 grant_id 和过期时间。
// Why：auth file 来自本机文件系统，consume 前必须先证明它确实由当前服务进程签发。
fn verify_init_grant(
    grant: &Value,
    signing_key: &SigningKey,
) -> Result<VerifiedInitGrant, ApiError> {
    let invalid = || ApiError {
        code: "INVALID_INIT_GRANT",
        message: "invalid init grant".to_string(),
    };
    if grant.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(invalid());
    }
    let payload = grant.get("payload").ok_or_else(invalid)?;
    let signature_text = grant
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(signature_text)
        .map_err(|_| invalid())?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|_| invalid())?;
    VerifyingKey::from(signing_key)
        .verify(
            &serde_json::to_vec(payload).map_err(|_| invalid())?,
            &signature,
        )
        .map_err(|_| invalid())?;
    if payload.get("scope").and_then(Value::as_str) != Some(INIT_GRANT_SCOPE) {
        return Err(invalid());
    }
    let issued_at = payload
        .get("iat")
        .and_then(Value::as_u64)
        .ok_or_else(invalid)?;
    let grant_id = payload
        .get("grant_id")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?
        .to_string();
    let expires_at = payload
        .get("exp")
        .and_then(Value::as_u64)
        .ok_or_else(invalid)?;
    if expires_at.saturating_sub(issued_at) != INIT_GRANT_TTL.as_secs() {
        return Err(invalid());
    }
    Ok(VerifiedInitGrant {
        grant_id,
        expires_at,
    })
}

fn init_auth_store() -> &'static Mutex<InitAuthStore> {
    static STORE: OnceLock<Mutex<InitAuthStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(InitAuthStore::default()))
}

fn init_grant_store() -> &'static Mutex<InitGrantStore> {
    static STORE: OnceLock<Mutex<InitGrantStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(InitGrantStore::default()))
}

fn init_authorization_transition_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn clear_init_authorization_state() {
    init_auth_store().lock().unwrap().clear_token();
    init_grant_store().lock().unwrap().clear_grant();
}

fn refresh_existing_init_authorization(now: u64) -> Option<InitAuthToken> {
    let has_token = init_auth_store()
        .lock()
        .unwrap()
        .token_status(now)
        .is_some();
    let has_grant = init_grant_store().lock().unwrap().has_active_grant(now);
    if !has_token && !has_grant {
        return None;
    }
    let token = init_auth_store()
        .lock()
        .unwrap()
        .refresh_token(now, INIT_AUTH_TOKEN_TTL);
    init_grant_store().lock().unwrap().clear_grant();
    Some(token)
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

    fn clear_token(&mut self) {
        self.token = None;
    }
}

impl InitGrantStore {
    fn replace_grant(&mut self, grant_id: String, scope: String, expires_at: u64) {
        self.grant = Some(InitGrant {
            grant_id,
            scope,
            expires_at,
            consumed: false,
        });
    }

    fn clear_grant(&mut self) {
        self.grant = None;
    }

    fn matches_grant(&self, grant_id: &str, scope: &str) -> bool {
        self.grant
            .as_ref()
            .is_some_and(|grant| grant.grant_id == grant_id && grant.scope == scope)
    }

    fn has_active_grant(&self, now: u64) -> bool {
        self.grant
            .as_ref()
            .is_some_and(|grant| !grant.consumed && grant.expires_at > now)
    }

    fn consume_grant(&mut self, grant_id: &str, scope: &str, now: u64) -> bool {
        let Some(grant) = self.grant.as_ref() else {
            return false;
        };
        if grant.grant_id != grant_id || grant.scope != scope {
            return false;
        }
        let valid = !grant.consumed && grant.expires_at > now;
        self.grant = None;
        valid
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
    use std::{
        sync::{Mutex as TestMutex, OnceLock},
        time::Duration,
    };

    use axum::{Json, http::StatusCode};
    use base64::Engine;
    use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};

    use super::{
        INIT_GRANT_SCOPE, InitAuthStore, InitGrantRequest, InitGrantStore, URL_SAFE_NO_PAD,
        auth_grant, auth_grant_consume, clear_init_authorization_state, init_auth_store,
        init_grant_signing_key, init_grant_store, now_epoch, random_base64_token,
        refresh_existing_init_authorization, signed_init_grant, verify_init_grant,
    };

    fn auth_handler_test_lock() -> &'static TestMutex<()> {
        static LOCK: OnceLock<TestMutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| TestMutex::new(()))
    }

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
    fn init_auth_store_clears_current_token() {
        let mut store = InitAuthStore::default();

        store.refresh_token(100, Duration::from_secs(180));
        store.clear_token();

        assert_eq!(store.token_status(120), None);
    }

    #[test]
    fn signed_init_grant_has_verifiable_ed25519_signature() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
        let signed = signed_init_grant(&signing_key, 100);
        let grant = signed.grant;
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
        assert_eq!(
            payload.get("exp").unwrap().as_u64(),
            Some(signed.expires_at)
        );
        assert_eq!(
            payload.get("grant_id").unwrap().as_str(),
            Some(signed.grant_id.as_str())
        );
    }

    #[test]
    fn verify_init_grant_extracts_signed_metadata() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
        let signed = signed_init_grant(&signing_key, 100);

        let verified = match verify_init_grant(&signed.grant, &signing_key) {
            Ok(verified) => verified,
            Err(error) => panic!("{}", error.message),
        };

        assert_eq!(verified.grant_id, signed.grant_id);
        assert_eq!(verified.expires_at, signed.expires_at);
    }

    #[test]
    fn verify_init_grant_rejects_wrong_ttl() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
        let payload = serde_json::json!({
            "grant_id": random_base64_token(),
            "scope": "init:create",
            "iat": 100,
            "exp": 401,
            "nonce": random_base64_token(),
        });
        let signature = signing_key.sign(&serde_json::to_vec(&payload).unwrap());
        let grant = serde_json::json!({
            "version": 1,
            "payload": payload,
            "signature": URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        });

        let error = match verify_init_grant(&grant, &signing_key) {
            Ok(_) => panic!("grant with wrong ttl should fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "INVALID_INIT_GRANT");
    }

    #[test]
    fn init_grant_signing_key_is_process_stable() {
        let first = VerifyingKey::from(init_grant_signing_key()).to_bytes();
        let second = VerifyingKey::from(init_grant_signing_key()).to_bytes();

        assert_eq!(first, second);
    }

    #[test]
    fn init_grant_store_keeps_only_latest_grant() {
        let mut store = InitGrantStore::default();

        store.replace_grant("first".to_string(), INIT_GRANT_SCOPE.to_string(), 400);
        store.replace_grant("second".to_string(), INIT_GRANT_SCOPE.to_string(), 500);

        let grant = store.grant.unwrap();
        assert_eq!(grant.grant_id, "second");
        assert_eq!(grant.scope, "init:create");
        assert_eq!(grant.expires_at, 500);
        assert!(!grant.consumed);
    }

    #[test]
    fn init_grant_store_clears_current_grant() {
        let mut store = InitGrantStore::default();

        store.replace_grant("grant".to_string(), INIT_GRANT_SCOPE.to_string(), 400);
        store.clear_grant();

        assert!(store.grant.is_none());
    }

    #[test]
    fn init_grant_store_consumes_current_grant_once() {
        let mut store = InitGrantStore::default();

        store.replace_grant("grant".to_string(), INIT_GRANT_SCOPE.to_string(), 400);

        assert!(store.consume_grant("grant", INIT_GRANT_SCOPE, 399));
        assert!(!store.consume_grant("grant", INIT_GRANT_SCOPE, 399));
    }

    #[test]
    fn init_grant_store_rejects_and_keeps_mismatched_grant() {
        let mut store = InitGrantStore::default();

        store.replace_grant("grant".to_string(), INIT_GRANT_SCOPE.to_string(), 400);

        assert!(!store.consume_grant("wrong", INIT_GRANT_SCOPE, 399));
        assert!(store.grant.is_some());
    }

    #[test]
    fn init_grant_store_rejects_and_clears_expired_consume() {
        let mut store = InitGrantStore::default();

        store.replace_grant("grant".to_string(), INIT_GRANT_SCOPE.to_string(), 400);

        assert!(!store.consume_grant("grant", INIT_GRANT_SCOPE, 400));
        assert!(store.grant.is_none());
    }

    #[test]
    fn init_grant_store_rejects_and_keeps_wrong_scope_consume() {
        let mut store = InitGrantStore::default();

        store.replace_grant("grant".to_string(), INIT_GRANT_SCOPE.to_string(), 400);

        assert!(!store.consume_grant("grant", "other:scope", 399));
        assert!(store.grant.is_some());
    }

    #[test]
    fn init_grant_store_is_process_stable() {
        let first = init_grant_store() as *const _;
        let second = init_grant_store() as *const _;

        assert_eq!(first, second);
    }

    #[test]
    fn refresh_existing_init_authorization_rotates_from_active_grant() {
        let _guard = auth_handler_test_lock().lock().unwrap();
        clear_init_authorization_state();
        init_grant_store().lock().unwrap().replace_grant(
            "current-grant".to_string(),
            INIT_GRANT_SCOPE.to_string(),
            400,
        );

        let token = refresh_existing_init_authorization(100).unwrap();

        assert_eq!(token.expires_at, 400);
        assert_eq!(
            init_auth_store().lock().unwrap().token_status(101),
            Some(400)
        );
        assert!(init_grant_store().lock().unwrap().grant.is_none());
        clear_init_authorization_state();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn auth_grant_consume_keeps_state_for_unsigned_invalid_grant() {
        let _guard = auth_handler_test_lock().lock().unwrap();
        clear_init_authorization_state();
        init_auth_store()
            .lock()
            .unwrap()
            .refresh_token(100, Duration::from_secs(180));
        init_grant_store().lock().unwrap().replace_grant(
            "current-grant".to_string(),
            INIT_GRANT_SCOPE.to_string(),
            400,
        );

        let (status, _) = auth_grant_consume(Ok(Json(serde_json::json!({ "version": 1 })))).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            init_auth_store().lock().unwrap().token_status(120),
            Some(280)
        );
        assert_eq!(
            init_grant_store()
                .lock()
                .unwrap()
                .grant
                .as_ref()
                .unwrap()
                .grant_id,
            "current-grant"
        );
        clear_init_authorization_state();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn auth_grant_consume_keeps_state_for_stale_signed_grant() {
        let _guard = auth_handler_test_lock().lock().unwrap();
        clear_init_authorization_state();
        let now = now_epoch();
        let stale = signed_init_grant(init_grant_signing_key(), now);
        init_auth_store()
            .lock()
            .unwrap()
            .refresh_token(now, Duration::from_secs(300));
        init_grant_store().lock().unwrap().replace_grant(
            "current-grant".to_string(),
            INIT_GRANT_SCOPE.to_string(),
            now.saturating_add(300),
        );

        let (status, _) = auth_grant_consume(Ok(Json(stale.grant))).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            init_auth_store().lock().unwrap().token_status(now + 1),
            Some(now + 300)
        );
        assert_eq!(
            init_grant_store()
                .lock()
                .unwrap()
                .grant
                .as_ref()
                .unwrap()
                .grant_id,
            "current-grant"
        );
        clear_init_authorization_state();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn auth_grant_consume_clears_state_for_current_expired_grant() {
        let _guard = auth_handler_test_lock().lock().unwrap();
        clear_init_authorization_state();
        let now = now_epoch();
        let expired = signed_init_grant(init_grant_signing_key(), now.saturating_sub(301));
        init_auth_store()
            .lock()
            .unwrap()
            .refresh_token(now, Duration::from_secs(300));
        init_grant_store().lock().unwrap().replace_grant(
            expired.grant_id.clone(),
            INIT_GRANT_SCOPE.to_string(),
            expired.expires_at,
        );

        let (status, _) = auth_grant_consume(Ok(Json(expired.grant))).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            init_auth_store().lock().unwrap().token_status(now + 1),
            None
        );
        assert!(init_grant_store().lock().unwrap().grant.is_none());
        clear_init_authorization_state();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn auth_grant_exchanges_token_for_single_active_grant() {
        let _guard = auth_handler_test_lock().lock().unwrap();
        clear_init_authorization_state();
        let token = init_auth_store()
            .lock()
            .unwrap()
            .refresh_token(now_epoch(), Duration::from_secs(180));

        let (status, body) = auth_grant(Ok(Json(InitGrantRequest {
            auth_token: token.token,
        })))
        .await;

        let grant_id = body["data"]["payload"]["grant_id"].as_str().unwrap();
        let store = init_grant_store().lock().unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(store.grant.as_ref().unwrap().grant_id, grant_id);
        drop(store);
        clear_init_authorization_state();
    }
}
