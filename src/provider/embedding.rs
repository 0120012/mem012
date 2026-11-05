use std::time::Duration;

use serde::Deserialize;
use tokio::sync::Semaphore;

use super::http::{http_client, provider_endpoint};

// 备注：当前已接入 search_memory 保底召回；provider 协议细节仍需随模型返回格式迭代。

// What：限制进程内同时进行的 embedding 请求数为 2。
// Why：避免批量审批在双核机器上并发压垮远程 provider。
static EMBED_GATE: Semaphore = Semaphore::const_new(2);
const EMBEDDING_RETRY_DELAYS: [u64; 3] = [5, 20, 60];

pub(crate) fn embedding_request_wait(recent: &mut std::collections::VecDeque<(std::time::Instant, u32)>, rpm: u32, tpm: u32, tokens: u32, now: std::time::Instant) -> Result<Duration, &'static str> {
    // What：核对最近 60 秒的请求和 token 额度，允许时登记，否则返回等待时长。
    // Why：使用滑动窗口避免分钟边界突发；等待不扣额度，每次实际尝试均需调用。
    assert!(rpm > 0 && tpm > 0);
    if tokens > tpm { return Err("单次请求 token 数超过 TPM，无法发送"); }
    let window = Duration::from_secs(60);
    while recent.front().is_some_and(|(at, _)| now.duration_since(*at) >= window) {
        recent.pop_front();
    }
    let mut count = recent.len() as u64;
    let mut total = recent.iter().map(|(_, used)| u64::from(*used)).sum::<u64>() + u64::from(tokens);
    if count < u64::from(rpm) && total <= u64::from(tpm) {
        recent.push_back((now, tokens));
        return Ok(Duration::ZERO);
    }
    for (at, used) in recent.iter() {
        count -= 1;
        total -= u64::from(*used);
        if count < u64::from(rpm) && total <= u64::from(tpm) { return Ok(window - now.duration_since(*at)); }
    }
    unreachable!("窗口清空后必能容纳不超过 TPM 的单次请求")
}

#[derive(Debug)]
struct PermanentEmbeddingError(String);

impl std::fmt::Display for PermanentEmbeddingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PermanentEmbeddingError {}

// What：判断 embedding 错误是否来自本地可确定的响应问题。
// Why：这类错误与网络无关，重试只会重复相同失败并占用 embedding 闸门。
fn is_permanent(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.downcast_ref::<PermanentEmbeddingError>().is_some()
}

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

pub(crate) fn load_bge_m3_tokenizer(
    path: &std::path::Path,
) -> Result<tokenizers::Tokenizer, Box<dyn std::error::Error + Send + Sync>> {
    // What：加载并校验固定版本的 BGE-M3 分词文件，关闭截断和填充。
    // Why：预算必须统计完整输入，避免文件漂移或自动截断掩盖超出模型窗口的内容。
    use sha2::{Digest, Sha256};

    let data = std::fs::read(path)?;
    let hash = format!("{:x}", Sha256::digest(&data));
    if hash != "21106b6d7dab2952c1d496fb21d5dc9db75c28ed361a05f5020bbba27810dd08" {
        return Err("BGE-M3 tokenizer SHA256 校验失败".into());
    }
    let mut tokenizer = tokenizers::Tokenizer::from_bytes(&data)?;
    tokenizer.with_truncation(None)?;
    tokenizer.with_padding(None);
    Ok(tokenizer)
}

pub(crate) fn markdown_heading_paths(content: &str) -> Vec<(usize, String)> {
    // What：提取顶层 ATX 标题的原文字节起点与层级路径，跳过代码围栏。
    // Why：后续按原文分块需保留章节上下文；围栏内的井号不能改变正文标题层级。
    let (mut paths, mut headings, mut fence) = (Vec::new(), Vec::<(usize, &str)>::new(), None);
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let line = line.trim_end_matches(['\r', '\n']);
        let text = line.trim_start_matches(' ');
        if line.len() - text.len() > 3 { continue; }
        let marker = text.as_bytes().first().copied().unwrap_or_default();
        let run = text.bytes().take_while(|byte| *byte == marker).count();
        if let Some((kind, length)) = fence {
            if marker == kind && run >= length && text[run..].trim_matches([' ', '\t']).is_empty() { fence = None; }
            continue;
        }
        if matches!(marker, b'`' | b'~') && run >= 3 {
            if marker == b'~' || !text[run..].contains('`') { fence = Some((marker, run)); }
            continue;
        }
        if marker != b'#' || run > 6 || !matches!(text.as_bytes().get(run), None | Some(b' ' | b'\t')) { continue; }
        let title = text[run..].trim_matches([' ', '\t']);
        let without_hashes = title.trim_end_matches('#');
        let title = if without_hashes.is_empty() || without_hashes.ends_with([' ', '\t']) { without_hashes.trim_end_matches([' ', '\t']) } else { title };
        headings.retain(|(level, _)| *level < run);
        if !title.is_empty() { headings.push((run, title)); }
        paths.push((start, headings.iter().map(|(_, title)| *title).collect::<Vec<_>>().join(" > ")));
    }
    paths
}

pub(crate) struct ContentChunk<'a> {
    pub chunk_index: usize,
    pub content_start_byte: usize,
    pub content_end_byte: usize,
    pub title_path: &'a str,
    pub content: &'a str,
}

pub(crate) fn single_content_chunk<'a>(
    content: &'a str,
    title_path: &'a str,
    embedding_input: &str,
    tokenizer: &tokenizers::Tokenizer,
) -> Result<Option<ContentChunk<'a>>, Box<dyn std::error::Error + Send + Sync>> {
    // What：完整请求含特殊 token 不超过 BGE-M3 窗口时返回单块，超限返回 None。
    // Why：使用已关闭截断和填充的分词器统计元信息，正文范围仍取原文 UTF-8 字节，避免漏算或偏移失真。
    if tokenizer.encode(embedding_input, true)?.len() > 8192 {
        return Ok(None);
    }
    Ok(Some(ContentChunk {
        chunk_index: 0,
        content_start_byte: 0,
        content_end_byte: content.len(),
        title_path,
        content,
    }))
}

pub(crate) fn fitting_content_end(
    content: &str,
    title_path: &str,
    metadata: &str,
    tokenizer: &tokenizers::Tokenizer,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    // What：按完整请求预算定位一个可发送的正文前缀，返回其 UTF-8 字节终点。
    // Why：直接使用原文字节偏移并复核截取后的 token 数，避免解码改写原文或切开多字节字符。
    let prefix = [metadata, title_path, ""].join("\n");
    let input = format!("{prefix}{content}");
    let encoding = tokenizer.encode(input.as_str(), true)?;
    if encoding.len() <= 8192 {
        return Ok(content.len());
    }
    let cut = encoding.get_offsets()[..8191]
        .iter()
        .map(|p| p.1)
        .max()
        .unwrap_or(0);
    let mut end = cut.saturating_sub(prefix.len()).min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    while end > 0 {
        let input = format!("{prefix}{}", &content[..end]);
        if single_content_chunk(&content[..end], title_path, &input, tokenizer)?.is_some() {
            return Ok(end);
        }
        end = content[..end].char_indices().next_back().unwrap().0;
    }
    Err("元信息或单个正文字符超出 BGE-M3 token 预算".into())
}

// Why：BAAI/bge-m3 上限是 8192 token，而字符数不等于 token 数；取 6000 字符为中英混合、代码和特殊符号留出余量。
const MAX_EMBED_CHARS: usize = 6000;
// Why：超长输入仍需设上界；10 块约 6 万字符既覆盖完整记忆，又避免一次写入触发过多远程调用。
const MAX_EMBED_CHUNKS: usize = 10;

// What：按字符边界把输入切成不超过 MAX_EMBED_CHARS 的块，
// 返回保留的块与因超过 MAX_EMBED_CHUNKS 被丢弃的块数（未超限时为 0）。
fn chunk_input(mut input: &str) -> (Vec<&str>, usize) {
    let mut chunks = Vec::new();
    while let Some((byte_index, _)) = input.char_indices().nth(MAX_EMBED_CHARS) {
        let (head, tail) = input.split_at(byte_index);
        chunks.push(head);
        input = tail;
    }
    if !input.is_empty() || chunks.is_empty() {
        chunks.push(input);
    }
    let discarded_chunks = chunks.len().saturating_sub(MAX_EMBED_CHUNKS);
    chunks.truncate(MAX_EMBED_CHUNKS);
    (chunks, discarded_chunks)
}

pub async fn request_embedding(
    settings: &crate::config::EmbeddingSettings,
    input: &str,
) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    // What：长输入逐块嵌入，再把块向量归一化平均池化成单一向量。
    // Why：模型一次容不下全文；块向量均值再归一化可保持余弦距离语义，
    // 且结果仍是配置维度的单向量，与 pgvector 现有表结构兼容。
    let (chunks, discarded_chunks) = chunk_input(input);
    if discarded_chunks > 0 {
        eprintln!(
            "embedding 输入过长：总计 {} 块，丢弃 {} 块，输入 {} 字符",
            chunks.len() + discarded_chunks,
            discarded_chunks,
            input.chars().count()
        );
    }
    let mut vectors = Vec::with_capacity(chunks.len());
    for (chunk_index, chunk) in chunks.into_iter().enumerate() {
        vectors.push(request_chunk_with_retry(settings, chunk_index, chunk).await?);
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

// What：给确定性错误补上块序号并保持永久标记。
// Why：早退与末次失败两条路径都要这层包装，抽出来避免同一段文案重复。
fn permanent_chunk_error(
    chunk_index: usize,
    error: &(dyn std::error::Error + Send + Sync + 'static),
) -> Box<dyn std::error::Error + Send + Sync> {
    PermanentEmbeddingError(format!(
        "embedding 块 {} 确定性错误，跳过重试：{error}",
        chunk_index + 1
    ))
    .into()
}

// What：对单个 embedding 块执行最多三次重试，并按递增间隔退避。
// Why：每次请求返回后 permit 已在 request_single_embedding 内释放，退避不会占用闸门名额。
async fn request_chunk_with_retry(
    settings: &crate::config::EmbeddingSettings,
    chunk_index: usize,
    input: &str,
) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    let attempts = EMBEDDING_RETRY_DELAYS.len() + 1;
    for (retry, delay) in EMBEDDING_RETRY_DELAYS.iter().enumerate() {
        match request_single_embedding(settings, input).await {
            Ok(vector) => return Ok(vector),
            Err(error) if is_permanent(error.as_ref()) => {
                return Err(permanent_chunk_error(chunk_index, error.as_ref()));
            }
            Err(error) => {
                eprintln!(
                    "embedding 块 {} 第 {}/{attempts} 次尝试失败：{error}",
                    chunk_index + 1,
                    retry + 1
                );
                tokio::time::sleep(Duration::from_secs(*delay)).await;
            }
        }
    }
    // Why：末次失败要带上块序号和总次数向上传播，否则调用方日志只有 provider 原文，无法定位。
    match request_single_embedding(settings, input).await {
        Ok(vector) => Ok(vector),
        Err(error) if is_permanent(error.as_ref()) => {
            Err(permanent_chunk_error(chunk_index, error.as_ref()))
        }
        Err(error) => Err(format!(
            "embedding 块 {} 连续 {attempts} 次尝试失败：{error}",
            chunk_index + 1
        )
        .into()),
    }
}

pub(crate) async fn request_limited_embedding_with_retry(
    settings: &crate::config::EmbeddingSettings,
    input: &str,
    recent: &mut std::collections::VecDeque<(std::time::Instant, u32)>,
    rpm: u32,
    tpm: u32,
    tokens: u32,
) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    // What：迁移块请求沿用既有退避间隔，每次尝试重新经过 RPM/TPM 闸门。
    // Why：失败请求也消耗 provider 额度；不能只对整个重试流程登记一次。
    if tokens > tpm {
        return Err("单次请求 token 数超过 TPM，无法发送".into());
    }
    for attempt in 0..=EMBEDDING_RETRY_DELAYS.len() {
        match request_limited_embedding_attempt(settings, input, recent, rpm, tpm, tokens).await {
            Ok(vector) => return Ok(vector),
            Err(error) if is_permanent(error.as_ref()) || attempt == EMBEDDING_RETRY_DELAYS.len() => {
                return Err(error);
            }
            Err(_) => tokio::time::sleep(Duration::from_secs(EMBEDDING_RETRY_DELAYS[attempt])).await,
        }
    }
    unreachable!("最后一次尝试必定返回")
}

pub(crate) async fn request_limited_embedding_attempt(settings: &crate::config::EmbeddingSettings, input: &str, recent: &mut std::collections::VecDeque<(std::time::Instant, u32)>, rpm: u32, tpm: u32, tokens: u32) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    // What：等待迁移额度后发出一次请求，失败尝试也保留计数。
    // Why：每次尝试独立经过闸门，等待期间不持有 HTTP permit，不把重试藏进一次计数。
    loop {
        let wait = embedding_request_wait(recent, rpm, tpm, tokens, std::time::Instant::now())?;
        if wait.is_zero() { break; }
        tokio::time::sleep(wait).await;
    }
    request_single_embedding(settings, input).await
}

async fn request_single_embedding(
    settings: &crate::config::EmbeddingSettings,
    input: &str,
) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    let _permit = EMBED_GATE.acquire().await?;
    // Why：远程模型必须返回配置维度，和 pgvector 表结构保持硬一致。
    let endpoint = provider_endpoint(&settings.api, &settings.api_type)?;
    let body = if settings.model.starts_with("Qwen/Qwen3") {
        serde_json::json!({ "model": settings.model, "input": input, "dimensions": settings.dimension })
    } else {
        serde_json::json!({ "model": settings.model, "input": input })
    };
    let request = http_client(settings.proxy.as_deref())?
        .post(endpoint)
        .json(&body);
    let request = if settings.key.trim().is_empty() {
        request
    } else {
        request.bearer_auth(&settings.key)
    };
    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        let message = format!("embedding API {status}: {}", response.text().await?.trim());
        if matches!(status.as_u16(), 400 | 401 | 403 | 404) {
            return Err(PermanentEmbeddingError(message).into());
        }
        return Err(message.into());
    }
    let response: EmbeddingResponse = response.json().await?;
    let embedding = response
        .first_embedding()
        .ok_or_else(|| PermanentEmbeddingError("embedding 响应为空或格式错误".into()))?;
    if embedding.len() != settings.dimension {
        return Err(
            PermanentEmbeddingError(format!("embedding 维度错误: {}", embedding.len())).into(),
        );
    }
    Ok(embedding)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "由 Python 本地模拟服务启动"]
    async fn limited_attempt_waits_and_counts_failures() {
        let settings = crate::config::EmbeddingSettings { api: std::env::var("MEM012_RATE_MOCK_URL").expect("需要 Python 模拟服务"), api_type: "embeddings".into(), key: String::new(), model: "test".into(), dimension: 2, fallback_max_distance: 1.0, proxy: None };
        let started = std::time::Instant::now();
        let mut recent = std::collections::VecDeque::from([(started - super::Duration::from_millis(59_950), 4)]);
        assert!(super::request_limited_embedding_attempt(&settings, "test", &mut recent, 1, 10, 4).await.is_err());
        assert!(started.elapsed() >= super::Duration::from_millis(50));
        assert_eq!(recent.len(), 1);
        assert_eq!(super::request_limited_embedding_attempt(&settings, "test", &mut recent, 2, 10, 4).await.unwrap(), vec![0.1, 0.2]);
        assert_eq!(recent.len(), 2);
        assert!(super::request_limited_embedding_attempt(&settings, "test", &mut recent, 2, 10, 11).await.is_err());
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn migration_rate_window_reserves_only_when_allowed() {
        let now = std::time::Instant::now();
        let mut recent = std::collections::VecDeque::new();
        for (offset, tokens, expected) in [(0, 4, Ok(0)), (1, 6, Ok(0)), (2, 0, Ok(58)), (2, 1, Ok(58)), (60, 4, Ok(0)), (61, 10, Ok(59)), (120, 10, Ok(0)), (121, 11, Err("单次请求 token 数超过 TPM，无法发送"))] {
            assert_eq!(super::embedding_request_wait(&mut recent, 2, 10, tokens, now + super::Duration::from_secs(offset)).map(|wait| wait.as_secs()), expected);
        }
        assert_eq!(recent.len(), 1);
    }

    use super::{
        EmbeddingResponse, MAX_EMBED_CHARS, MAX_EMBED_CHUNKS, PermanentEmbeddingError, chunk_input,
        is_permanent, permanent_chunk_error,
    };

    #[test]
    fn is_permanent_only_matches_permanent_error() {
        let permanent = PermanentEmbeddingError("embedding 维度错误: 512".into());
        assert!(is_permanent(&permanent));

        let transient: Box<dyn std::error::Error + Send + Sync> = "timeout".into();
        assert!(!is_permanent(transient.as_ref()));
    }

    // Why：包装后必须仍被判为永久错误，否则外层重试会重新把它当成可重试失败。
    #[test]
    fn permanent_chunk_error_keeps_marker_and_chunk_index() {
        let source = PermanentEmbeddingError("embedding 响应为空或格式错误".into());
        let wrapped = permanent_chunk_error(2, &source);
        assert!(is_permanent(wrapped.as_ref()));
        assert!(wrapped.to_string().contains("块 3"));
    }

    #[test]
    fn chunk_input_keeps_all_chunks_within_limit() {
        let input = "汉".repeat(MAX_EMBED_CHARS * 2 + 1);
        let (chunks, discarded) = chunk_input(&input);
        assert_eq!(chunks.len(), 3);
        assert_eq!(discarded, 0);
    }

    // Why：恰好达到 MAX_EMBED_CHUNKS 时不得报告丢弃，这是 saturating_sub 最易写反的边界。
    #[test]
    fn chunk_input_reports_discarded_chunks_beyond_limit() {
        let exact = "汉".repeat(MAX_EMBED_CHARS * MAX_EMBED_CHUNKS);
        let (chunks, discarded) = chunk_input(&exact);
        assert_eq!(chunks.len(), MAX_EMBED_CHUNKS);
        assert_eq!(discarded, 0);

        let overflow = "汉".repeat(MAX_EMBED_CHARS * (MAX_EMBED_CHUNKS + 2) + 1);
        let (chunks, discarded) = chunk_input(&overflow);
        assert_eq!(chunks.len(), MAX_EMBED_CHUNKS);
        assert_eq!(discarded, 3);
    }

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
