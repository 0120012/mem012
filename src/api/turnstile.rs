use serde::Deserialize;

use super::utils::ApiError;

#[derive(Deserialize)]
struct SiteverifyResponse {
    success: bool,
    #[serde(default)]
    #[allow(dead_code)]
    challenge_ts: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    hostname: Option<String>,
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

// What：调用 Cloudflare Turnstile 官方 siteverify 接口验证前端 challenge token。
// Why：客户端 widget 只证明用户拿到了 token，真正的授权边界必须由后端用 secret 校验。
#[allow(dead_code)]
pub async fn verify_token(
    settings: &crate::config::TurnstileSettings,
    token: &str,
    remote_ip: Option<&str>,
) -> Result<(), ApiError> {
    if token.trim().is_empty() {
        return Err(turnstile_error("turnstile token is required"));
    }
    let mut form = vec![
        ("secret", settings.secret_key.as_str()),
        ("response", token.trim()),
    ];
    if let Some(ip) = remote_ip.map(str::trim).filter(|ip| !ip.is_empty()) {
        form.push(("remoteip", ip));
    }
    let response = reqwest::Client::new()
        .post(&settings.verify_url)
        .form(&form)
        .send()
        .await
        .map_err(|error| turnstile_error(error.to_string()))?
        .json::<SiteverifyResponse>()
        .await
        .map_err(|error| turnstile_error(error.to_string()))?;
    if response.success {
        return Ok(());
    }
    let message = if response.error_codes.is_empty() {
        "turnstile verification failed".to_string()
    } else {
        format!(
            "turnstile verification failed: {}",
            response.error_codes.join(", ")
        )
    };
    Err(turnstile_error(message))
}

fn turnstile_error(message: impl Into<String>) -> ApiError {
    ApiError {
        code: "TURNSTILE_VERIFY_FAILED",
        message: message.into(),
    }
}
