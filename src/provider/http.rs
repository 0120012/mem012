pub(super) fn http_client(
    proxy: Option<&str>,
) -> Result<reqwest::Client, Box<dyn std::error::Error + Send + Sync>> {
    // What：构造 provider 访问外部模型 API 时使用的 HTTP client。
    // Why：embedding 和 rerank 都需要统一处理本机代理，避免每个 provider 调用点重复拼接代理 URL。
    let mut builder = reqwest::Client::builder();
    if let Some(proxy) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url(proxy))?);
    }
    Ok(builder.build()?)
}

pub(super) fn provider_endpoint(
    api: &str,
    api_type: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // What：把 provider base URL 和 endpoint 类型合成为最终请求地址。
    // Why：不同模型能力共享同一个 base URL，但 embedding/rerank endpoint 需要可配置，避免写死路径。
    let api = api.trim().trim_end_matches('/');
    if api == "local" {
        return Err("local provider executor 尚未接入".into());
    }
    let api_type = api_type.trim().trim_matches('/');
    if api_type.is_empty() || api.rsplit('/').next() == Some(api_type) {
        Ok(api.to_string())
    } else {
        Ok(format!("{api}/{api_type}"))
    }
}

fn proxy_url(proxy: &str) -> String {
    let proxy = proxy.trim();
    if proxy.contains("://") {
        proxy.to_string()
    } else {
        format!("http://{proxy}")
    }
}
