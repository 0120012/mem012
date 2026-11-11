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
    request_chunked_rerank(settings, query, documents).await
}

pub struct RerankResult {
    pub index: usize,
    pub score: f32,
}

fn split_rerank_document(
    tokenizer: &tokenizers::Tokenizer, query: &str, document: &str,
) -> ProviderResult<Vec<String>> {
    // What：按实际 pair 预算切分原文，保留重叠和全部原文字节。
    // Why：只截取开头会漏掉远端证据，不能依赖服务端自动处理长文。
    let reserved = tokenizer.encode((query, ""), true)?.len();
    let capacity = 8192usize.checked_sub(reserved).filter(|size| *size > 0)
        .ok_or("rerank query 已耗尽模型预算")?;
    let encoding = tokenizer.encode(document, false)?;
    let offsets = encoding.get_offsets();
    if encoding.len() <= capacity && tokenizer.encode((query, document), true)?.len() <= 8192 {
        return Ok(vec![document.to_string()]);
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < offsets.len() {
        let begin = if start == 0 { 0 } else { offsets[start].0 };
        let mut end = (start + capacity).min(offsets.len());
        loop {
            let finish = if end == offsets.len() { document.len() } else { offsets[end].0 };
            let text = document.get(begin..finish).ok_or("rerank 分块不是有效 UTF-8 边界")?;
            if !text.is_empty() && tokenizer.encode((query, text), true)?.len() <= 8192 {
                chunks.push(text.to_string());
                break;
            }
            end -= 1;
            if end <= start { return Err("rerank 预算无法容纳一个原文字符".into()); }
        }
        if end == offsets.len() { break; }
        start = end - 80.min((end - start) / 2);
    }
    Ok(chunks)
}

async fn score_rerank_batch(
    settings: &crate::config::RerankSettings, query: &str, chunks: &[String],
    owners: &[usize], scores: &mut [f32],
) -> ProviderResult<()> {
    // What：校验一批块的完整评分，并映射回原记忆累计最高分。
    // Why：块索引属于当前请求，不能直接当作候选记忆索引返回。
    debug_assert_eq!(chunks.len(), owners.len());
    let response = request_rerank(settings, query, chunks).await?;
    for result in normalize_results(response.results, chunks.len())? {
        let score = &mut scores[owners[result.index]];
        *score = score.max(result.score);
    }
    Ok(())
}

async fn request_chunked_rerank(
    settings: &crate::config::RerankSettings, query: &str, documents: &[String],
) -> ProviderResult<Vec<RerankResult>> {
    // What：按 40 块和 32768 pair token 双预算分批，再恢复记忆级排序。
    // Why：限制单次模型负载，同时让长记忆的所有块都有机会参与重排。
    let tokenizer = load_rerank_tokenizer(&settings.model)?;
    let mut scores = vec![f32::NEG_INFINITY; documents.len()];
    let (mut chunks, mut owners, mut tokens) = (Vec::new(), Vec::new(), 0);
    for (owner, document) in documents.iter().enumerate() {
        for chunk in split_rerank_document(&tokenizer, query, document)? {
            let count = tokenizer.encode((query, chunk.as_str()), true)?.len();
            if !chunks.is_empty() && (chunks.len() == 40 || tokens + count > 32768) {
                score_rerank_batch(settings, query, &chunks, &owners, &mut scores).await?;
                chunks.clear(); owners.clear(); tokens = 0;
            }
            chunks.push(chunk); owners.push(owner); tokens += count;
        }
    }
    if !chunks.is_empty() {
        score_rerank_batch(settings, query, &chunks, &owners, &mut scores).await?;
    }
    debug_assert!(scores.iter().all(|score| score.is_finite()));
    let mut results = scores.into_iter().enumerate()
        .map(|(index, score)| RerankResult { index, score }).collect::<Vec<_>>();
    results.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.index.cmp(&b.index)));
    Ok(results)
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

#[cfg(test)]
mod tests {
    use super::{load_rerank_tokenizer, split_rerank_document};

    #[test]
    fn overlapping_chunks_keep_boundary_evidence_together() {
        // What：将唯一证据放在切分边界附近，验证至少一个块完整保留它。
        // Why：全文字节覆盖仍可能拆散短证据，重叠应避免这种相关性损失。
        let tokenizer = load_rerank_tokenizer("BAAI/bge-reranker-v2-m3").unwrap();
        let query = "如何安全升级并回滚";
        let mut document = "background ".repeat(20000);
        let boundary = split_rerank_document(&tokenizer, query, &document).unwrap()[0].len();
        let evidence = "安全升级的完整证据：先备份再原子换表，失败保留旧代。";
        document.insert_str(boundary - 5, evidence);
        let chunks = split_rerank_document(&tokenizer, query, &document).unwrap();
        assert!(chunks.iter().any(|chunk| chunk.contains(evidence)));
    }

    #[test]
    fn short_documents_and_query_budget_are_explicit() {
        // What：短文保持原样，query 耗尽预算时拒绝分块。
        // Why：不能为超长 query 静默裁剪，也不能丢失空白或空文档身份。
        let tokenizer = load_rerank_tokenizer("BAAI/bge-reranker-v2-m3").unwrap();
        for document in ["", "  标题\n原文🧠\n ", "\n\t "] {
            assert_eq!(split_rerank_document(&tokenizer, "查询", document).unwrap(), vec![document]);
        }
        let query = "检索 ".repeat(9000);
        assert!(split_rerank_document(&tokenizer, &query, "原文").is_err());
    }

    #[test]
    fn chunks_cover_unicode_original_within_pair_budget() {
        // What：验证带 emoji、换行和唯一段落的长文覆盖至末尾。
        // Why：分块必须保留原文且能被重新编码，不能漏掉边界或远端证据。
        let tokenizer = load_rerank_tokenizer("BAAI/bge-reranker-v2-m3").unwrap();
        let query = "查找末尾的升级说明";
        let document = (0..4000).map(|i| format!("第{i}段：中文🧠 café e\u{301} 升级资料。\n"))
            .collect::<String>() + "TAIL_RELEVANT\n";
        let chunks = split_rerank_document(&tokenizer, query, &document).unwrap();
        assert!(chunks.len() > 1);
        let (mut covered, mut previous) = (0, 0);
        for chunk in &chunks {
            assert!(tokenizer.encode((query, chunk.as_str()), true).unwrap().len() <= 8192);
            let start = previous + document[previous..].find(chunk.as_str()).unwrap();
            assert!(start <= covered);
            covered = covered.max(start + chunk.len());
            previous = start;
        }
        assert_eq!(covered, document.len());
        assert!(chunks.last().unwrap().ends_with("TAIL_RELEVANT\n"));
    }
}
