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

// Why：bge-base 硬上限 512 token，超限时 llama.cpp 直接返回 500 而非截断；
// WordPiece 分词下 token 数不超过字符数（实测 510 个汉字 = 512 token 恰好达界），
// 取 500 字符/块为 CLS/SEP 与混合文本留出余量。
const MAX_EMBED_CHARS: usize = 500;
// Why：单块嵌入在当前 2 核 VPS 上实测约 30s，必须为异常超长输入设上界；
// 40 块 ≈ 2 万字远超正常记忆体量，超出部分放弃嵌入而不是让写入流程失败。
const MAX_EMBED_CHUNKS: usize = 40;

// What：按字符边界把输入切成不超过 MAX_EMBED_CHARS 的块。
fn chunk_input(mut input: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    while let Some((byte_index, _)) = input.char_indices().nth(MAX_EMBED_CHARS) {
        let (head, tail) = input.split_at(byte_index);
        chunks.push(head);
        input = tail;
    }
    if !input.is_empty() || chunks.is_empty() {
        chunks.push(input);
    }
    chunks.truncate(MAX_EMBED_CHUNKS);
    chunks
}

pub async fn request_embedding(
    settings: &crate::config::EmbeddingSettings,
    input: &str,
) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    // What：长输入逐块嵌入，再把块向量归一化平均池化成单一向量。
    // Why：模型一次容不下全文；块向量均值再归一化可保持余弦距离语义，
    // 且结果仍是配置维度的单向量，与 pgvector 现有表结构兼容。
    let chunks = chunk_input(input);
    let mut vectors = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        vectors.push(request_single_embedding(settings, chunk).await?);
    }
    if vectors.len() == 1 {
        return Ok(vectors.remove(0));
    }
    let mut sum = vec![0.0f32; settings.dimension];
    for vector in &vectors {
        for (target, value) in sum.iter_mut().zip(vector) {
            *target += value;
        }
    }
    let norm = sum.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm == 0.0 {
        return Err("embedding 池化结果为零向量".into());
    }
    Ok(sum.into_iter().map(|value| value / norm).collect())
}

async fn request_single_embedding(
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
