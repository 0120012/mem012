#![allow(dead_code)]

use serde::Deserialize;

use super::http::{http_client, provider_endpoint};

// 备注：当前已接入 search_memory 重排；provider 协议细节仍需随模型返回格式迭代。

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

pub(crate) fn load_rerank_tokenizer(model: &str) -> ProviderResult<tokenizers::Tokenizer> {
    // What：加载已冻结模型的实际 tokenizer，并禁用隐式截断。
    // Why：必须按 query/document 完整输入计数，未知模型不能借用其他分词器。
    use sha2::{Digest, Sha256};
    if !matches!(model, "BAAI/bge-reranker-v2-m3" | "bge-reranker-v2-m3") {
        return Err(format!("尚未冻结 rerank 模型的 tokenizer: {model}").into());
    }
    let data = include_bytes!("../../models/bge-reranker-v2-m3/tokenizer.json");
    if format!("{:x}", Sha256::digest(&data))
        != "69564b696052886ed0ac63fa393e928384e0f8caada38c1f4864a9bfbf379c15"
    {
        return Err("rerank tokenizer SHA256 校验失败".into());
    }
    let mut tokenizer = tokenizers::Tokenizer::from_bytes(&data)?;
    tokenizer.with_truncation(None)?;
    tokenizer.with_padding(None);
    Ok(tokenizer)
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
    // What：要求每份文档恰好对应一个有限评分。
    // Why：部分、重复或畸形评分不能冒充一次完整成功的重排。
    if results.len() != document_count {
        return Err("rerank 评分数量与文档数量不一致".into());
    }
    let mut output = Vec::with_capacity(results.len());
    let mut seen = std::collections::HashSet::new();
    for result in results {
        if result.index >= document_count {
            return Err(format!("rerank 返回越界 index: {}", result.index).into());
        }
        if !seen.insert(result.index) || !result.relevance_score.is_finite() {
            return Err("rerank 返回重复 index 或非有限评分".into());
        }
        output.push(RerankResult {
            index: result.index,
            score: result.relevance_score,
        });
    }
    output.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.index.cmp(&b.index)));
    Ok(output)
}
