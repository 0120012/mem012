#![allow(dead_code)]

use serde::Deserialize;

use super::http::{http_client, provider_endpoint};

// 备注：当前仅用于 provider/API 测试；search_memory 尚未接入，正式搜索链路还需二次迭代。

type ProviderResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

// What：调用外部 rerank provider，对 query 和候选文本做相关性重排。
// Why：搜索工具只应该消费稳定的 index/score 结果，provider 协议细节集中在这一层适配。
pub async fn rerank_text_documents(
    settings: &crate::config::RerankSettings,
    query: &str,
    documents: &[String],
) -> ProviderResult<Vec<RerankResult>> {
    let query = validate_query(query)?;
    if documents.is_empty() {
        return Ok(Vec::new());
    }
    let response = request_rerank(settings, query, documents).await?;
    normalize_results(response.results, documents.len())
}

pub struct RerankResult {
    pub index: usize,
    pub score: f32,
}

#[derive(Deserialize)]
struct RerankResponse {
    results: Vec<RerankResponseItem>,
}

#[derive(Deserialize)]
struct RerankResponseItem {
    index: usize,
    #[serde(alias = "score")]
    relevance_score: f32,
}

fn validate_query(query: &str) -> ProviderResult<&str> {
    let query = query.trim();
    if query.is_empty() {
        Err("rerank query 不能为空".into())
    } else {
        Ok(query)
    }
}

async fn request_rerank(
    settings: &crate::config::RerankSettings,
    query: &str,
    documents: &[String],
) -> ProviderResult<RerankResponse> {
    let endpoint = provider_endpoint(&settings.api, &settings.api_type)?;
    let request = http_client(settings.proxy.as_deref())?.post(endpoint).json(
        &serde_json::json!({ "model": settings.model, "query": query, "documents": documents, "top_n": documents.len() }),
    );
    let request = if settings.key.trim().is_empty() {
        request
    } else {
        request.bearer_auth(&settings.key)
    };
    Ok(request.send().await?.error_for_status()?.json().await?)
}

fn normalize_results(
    results: Vec<RerankResponseItem>,
    document_count: usize,
) -> ProviderResult<Vec<RerankResult>> {
    let mut output = Vec::with_capacity(results.len());
    for result in results {
        if result.index >= document_count {
            return Err(format!("rerank 返回越界 index: {}", result.index).into());
        }
        output.push(RerankResult {
            index: result.index,
            score: result.relevance_score,
        });
    }
    Ok(output)
}
