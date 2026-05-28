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

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::verify_token;

    async fn siteverify_url(response_body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer).await.unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn verify_token_accepts_successful_siteverify_response() {
        let settings = crate::config::TurnstileSettings {
            site_key: "site".to_string(),
            secret_key: "secret".to_string(),
            verify_url: siteverify_url(r#"{"success":true}"#).await,
        };

        assert!(
            verify_token(&settings, "challenge-token", None)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn verify_token_reports_siteverify_error_codes() {
        let settings = crate::config::TurnstileSettings {
            site_key: "site".to_string(),
            secret_key: "secret".to_string(),
            verify_url: siteverify_url(
                r#"{"success":false,"error-codes":["invalid-input-response"]}"#,
            )
            .await,
        };

        let error = verify_token(&settings, "challenge-token", None)
            .await
            .unwrap_err();

        assert_eq!(error.code, "TURNSTILE_VERIFY_FAILED");
        assert!(error.message.contains("invalid-input-response"));
    }
}
