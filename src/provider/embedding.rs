use serde::Deserialize;

use super::http::{http_client, provider_endpoint};

// 备注：当前已接入 search_memory 保底召回；provider 协议细节仍需随模型返回格式迭代。

#[derive(Deserialize)]
#[serde(untagged)]
enum EmbeddingResponse {
    OpenAi { data: Vec<EmbeddingData> },
    Bge(Vec<BgeEmbeddingData>),
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct BgeEmbeddingData {
    embedding: Vec<Vec<f32>>,
}

impl EmbeddingResponse {
    // What：从兼容的 provider 响应中取第一条 embedding。
    // Why：当前 BGE 返回二维向量，必须在统一入口展平后再交给 pgvector。
    fn first_embedding(self) -> Option<Vec<f32>> {
        match self {
            Self::OpenAi { data } => data.into_iter().next().map(|item| item.embedding),
            Self::Bge(data) => data
                .into_iter()
                .next()
                .and_then(|item| item.embedding.into_iter().next()),
        }
    }
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
        .first_embedding()
        .ok_or("embedding 响应为空或格式错误")?;
    if embedding.len() != settings.dimension {
        return Err(format!("embedding 维度错误: {}", embedding.len()).into());
    }
    Ok(embedding)
}

#[cfg(test)]
mod tests {
    use super::EmbeddingResponse;

    #[test]
    fn parses_current_bge_response() {
        let response = serde_json::from_str::<EmbeddingResponse>(
            r#"[{"index":0,"embedding":[[0.1,0.2,0.3]]}]"#,
        )
        .unwrap();
        assert_eq!(response.first_embedding().unwrap(), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn parses_openai_response() {
        let response =
            serde_json::from_str::<EmbeddingResponse>(r#"{"data":[{"embedding":[0.1,0.2,0.3]}]}"#)
                .unwrap();
        assert_eq!(response.first_embedding().unwrap(), vec![0.1, 0.2, 0.3]);
    }
}
