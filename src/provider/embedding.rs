use serde::Deserialize;

use super::http::{http_client, provider_endpoint};

// 备注：当前已接入 search_memory 保底召回；provider 协议细节仍需随模型返回格式迭代。

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

pub async fn request_embedding(
    settings: &crate::config::EmbeddingSettings,
    input: &str,
) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    // Why：远程模型必须返回配置维度，和 pgvector 表结构保持硬一致。
    let endpoint = provider_endpoint(&settings.api, &settings.api_type)?;
    let request = http_client(settings.proxy.as_deref())?
        .post(endpoint)
        .json(&serde_json::json!({ "model": settings.model, "input": input }));
    let request = if settings.key.trim().is_empty() {
        request
    } else {
        request.bearer_auth(&settings.key)
    };
    let response: EmbeddingResponse = request.send().await?.error_for_status()?.json().await?;
    let embedding = response
        .data
        .into_iter()
        .next()
        .ok_or("embedding 响应为空")?
        .embedding;
    if embedding.len() != settings.dimension {
        return Err(format!("embedding 维度错误: {}", embedding.len()).into());
    }
    Ok(embedding)
}
