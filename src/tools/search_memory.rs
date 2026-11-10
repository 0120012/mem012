// What：承载 search_memory 搜索工具入口。
// Why：先把工具路由接通，后续查询逻辑集中在独立模块里迭代。
use serde::Deserialize;

type ToolResult<T> = Result<T, Box<dyn std::error::Error>>;

// tune 中最低相关正例约 0.81，无关尾部最高约 0.18；冻结阈值后再跑 holdout。
const RERANK_MIN_SCORE: f32 = 0.3;
// 候选预算独立于最终 limit；在小规模 tune 验收后冻结，质量范围见验收记录。
const CANDIDATE_BUDGET: i32 = 40;

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchRequest {
    query: Option<String>,
    limit: Option<i32>,
    terms: Option<SearchTerms>,
    filters: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchTerms {
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[allow(dead_code)]
struct SearchPlan {
    query: String,
    semantic_query: String,
    effective_limit: i32,
    fields: Vec<String>,
    terms: SearchTermsPlan,
}

#[allow(dead_code)]
struct SearchTermsPlan {
    all: Vec<String>,
    none: Vec<String>,
    any: Vec<String>,
}

#[allow(dead_code)]
struct SearchCandidate {
    memory_uuid: String,
    title_norm: String,
    status: String,
    summary: String,
    content_preview: Option<String>,
    matched_fields: Vec<String>,
    score: f32,
    vector_range: Vec<(usize, usize)>,
}

async fn load_candidate_fields(
    context: &super::ToolContext<'_>,
    uuid: &str,
) -> ToolResult<Vec<(String, String)>> {
    // What：按候选 UUID 获取当前工作态的完整五字段原文。
    // Why：120 字预览不能作为重排证据，关键词和远端相关段落都必须可见。
    let row = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT title_text, summary_text, keywords_text, content_text, recall_when_text FROM memory_search_index WHERE memory_uuid = $1::uuid AND status IN ('active', 'pending')"
    ).bind(uuid).fetch_one(context.profile_pool).await?;
    Ok(["title", "summary", "keywords", "content", "recall_when"]
        .into_iter().map(str::to_string).zip([row.0, row.1, row.2, row.3, row.4]).collect())
}

fn evidence_document(
    fields: &[(String, String)], plan: &SearchPlan, ranges: &[(usize, usize)],
) -> String {
    // What：仅压缩连续完全相同的原文行，保留次数和原文字节范围。
    // Why：字面命中或单个向量块都不能证明其他段落无关，证据不完整时必须回退。
    let mut output = Vec::new();
    for (name, text) in fields.iter().filter(|(name, _)| plan.fields.contains(name)) {
        let mut offset = 0;
        let mut groups: Vec<(usize, usize, &str, usize)> = Vec::new();
        for part in text.split_inclusive('\n') {
            let start = offset; offset += part.len();
            if let Some(last) = groups.last_mut() && last.2 == part {
                last.1 = offset; last.3 += 1;
            } else {
                groups.push((start, offset, part, 1));
            }
        }
        for (start, end, part, count) in groups {
            output.push(format!("[{name}:{start}..{end}; repeated={count}]\n{part}"));
        }
        if name == "content" && !ranges.is_empty() {
            output.push(format!("[vector_hits:{ranges:?}]"));
        }
    }
    output.join("\n")
}

async fn build_rerank_documents(
    context: &super::ToolContext<'_>,
    plan: &SearchPlan,
    candidates: &[SearchCandidate],
) -> ToolResult<Vec<String>> {
    // What：为每条记忆读取选定字段的完整原文，并按实际 pair tokenizer 检查预算。
    // Why：无法证明完整证据可容纳时必须回退，不能把截断预览伪装成充分证据。
    let settings = context.rerank_settings.ok_or("rerank 配置缺失")?;
    let tokenizer = crate::provider::rerank::load_rerank_tokenizer(&settings.model)
        .map_err(|error| -> Box<dyn std::error::Error> {
            eprintln!("search_memory rerank evidence failed: tokenizer load: {error}");
            error
        })?;
    let mut documents = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let fields = load_candidate_fields(context, &candidate.memory_uuid).await
            .map_err(|error| -> Box<dyn std::error::Error> {
                eprintln!("search_memory rerank evidence failed: candidate fields: {error}");
                error
            })?;
        let mut document = fields.iter().filter(|(name, _)| plan.fields.contains(name))
            .map(|(name, text)| format!("[{name}]\n{}", text.trim()))
            .collect::<Vec<_>>().join("\n");
        let encoding = tokenizer.encode((plan.query.as_str(), document.as_str()), true)
            .map_err(|error| -> Box<dyn std::error::Error> {
                eprintln!("search_memory rerank evidence failed: pair encoding: {error}");
                error
            })?;
        if encoding.len() > 8192 {
            document = evidence_document(&fields, plan, &candidate.vector_range);
            let reduced = tokenizer.encode((plan.query.as_str(), document.as_str()), true)
                .map_err(|error| -> Box<dyn std::error::Error> {
                    eprintln!("search_memory rerank evidence failed: reduced pair encoding: {error}");
                    error
                })?;
            if document.is_empty() || reduced.len() > 8192 {
                eprintln!("search_memory rerank evidence failed: reduced document exceeds token budget");
                return Err("关键重排证据超过模型预算".into());
            }
        }
        documents.push(document);
    }
    Ok(documents)
}

struct SearchOutcome {
    results: Vec<SearchCandidate>,
    rerank: bool,
    errors: Vec<&'static str>,
}

fn merge_recalls(
    keyword: ToolResult<SearchOutcome>,
    vector: ToolResult<SearchOutcome>,
) -> (Vec<SearchCandidate>, Vec<&'static str>) {
    // What：按关键词、向量交替顺序合并候选，并收集召回故障。
    // Why：两路分数不可比较，UUID 去重必须保持可复现的回退顺序。
    let mut errors = Vec::new();
    let mut routes = Vec::new();
    for (result, code) in [(keyword, "keyword_recall_failed"), (vector, "vector_recall_failed")] {
        routes.push(match result {
            Ok(outcome) => outcome.results.into_iter(),
            Err(_) => { errors.push(code); Vec::new().into_iter() }
        });
    }
    let mut results: Vec<SearchCandidate> = Vec::new();
    loop {
        let mut exhausted = true;
        for route in &mut routes {
            if let Some(candidate) = route.next() {
                exhausted = false;
                if let Some(existing) = results.iter_mut().find(|item| item.memory_uuid == candidate.memory_uuid) {
                    existing.vector_range.extend(candidate.vector_range);
                    for field in candidate.matched_fields {
                        if !existing.matched_fields.contains(&field) { existing.matched_fields.push(field); }
                    }
                } else { results.push(candidate); }
            }
        }
        if exhausted { break; }
    }
    (results, errors)
}

pub async fn run(context: &super::ToolContext<'_>, args: &serde_json::Value) -> ToolResult<()> {
    // What：串联 search_memory 的参数、召回、重排和响应阶段。
    // Why：先固定执行骨架，避免后续实现时把输入校验、查询和输出格式混在一起。
    let request = parse_search_request(args)?;
    let plan = build_search_plan(context, request)?;
    let (keyword, vector) = tokio::join!(
        search_literal_candidates(context, &plan),
        search_embedding_fallback(context, &plan)
    );
    let (results, errors) = merge_recalls(keyword, vector);
    let mut outcome = SearchOutcome { results, errors, rerank: false };
    if outcome.results.len() > 1 {
        outcome = rerank_candidates(context, &plan, outcome).await?;
    }
    outcome.results.truncate(plan.effective_limit as usize);
    print_search_response(context, outcome)
}

fn parse_search_request(args: &serde_json::Value) -> ToolResult<SearchRequest> {
    // 1. 按 SearchRequest schema 解析 params；类型不匹配或包含未知字段会被 serde 拒绝。
    let mut request = serde_json::from_value::<SearchRequest>(args.clone())?;

    // 2. 规范化 query 的首尾空白。
    request.query = request.query.map(|query| query.trim().to_string());

    // 3. 校验 limit 下界；超过默认值的截断留给 build_search_plan 使用 context 处理。
    if matches!(request.limit, Some(limit) if limit < 1) {
        return Err("limit 必须大于 0".into());
    }

    // What：聚合搜索同时要求自然语言 query 和非空 include。
    // Why：query 只表达相关性意图，不解析其中的 AND/OR 为过滤指令。
    if request.query.as_deref().unwrap_or_default().is_empty() {
        return Err("query 不能为空".into());
    }
    let terms = request.terms.as_ref().ok_or("terms 必须提供")?;
    if terms.include.is_empty() {
        return Err("terms.include 必须非空".into());
    }
    if args.get("filters").is_some_and(serde_json::Value::is_null) {
        return Err("filters 必须是数组".into());
    }

    // 7. filters 只能来自文档定义的可搜索字段白名单。
    if let Some(filters) = &request.filters {
        validate_filters(filters)?;
    }

    // include 已保证非空，此处继续拒绝 include/exclude 中的空白字符串。
    if let Some(terms) = &request.terms {
        validate_terms(terms)?;
    }
    Ok(request)
}

fn validate_filters(filters: &[String]) -> ToolResult<()> {
    for filter in filters {
        if !matches!(
            filter.as_str(),
            "title" | "summary" | "keywords" | "content" | "recall_when"
        ) {
            return Err(format!("未知 filters 值: {filter}").into());
        }
    }
    Ok(())
}

fn validate_terms(terms: &SearchTerms) -> ToolResult<()> {
    let mut has_term = false;
    for (field, values) in [
        ("terms.include", &terms.include),
        ("terms.exclude", &terms.exclude),
    ] {
        if values.iter().any(|value| value.trim().is_empty()) {
            return Err(format!("{field} 不能为空").into());
        }
        has_term |= !values.is_empty();
    }
    has_term
        .then_some(())
        .ok_or_else(|| "terms.include、terms.exclude 至少一个必须非空".into())
}

fn build_semantic_query(query: &str, terms: &SearchTermsPlan) -> String {
    if !query.is_empty() {
        return query.to_string();
    }
    terms
        .all
        .iter()
        .chain(&terms.any)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_search_plan(
    context: &super::ToolContext<'_>,
    request: SearchRequest,
) -> ToolResult<SearchPlan> {
    // What：把已校验请求转换成后续召回阶段共享的搜索计划。
    // Why：limit 截断、默认字段和 terms 规范化必须只解释一次，避免后续阶段语义分叉。
    let default_limit = context.search_default_limit.max(1);
    let fields = request.filters.unwrap_or_default();
    let fields = if fields.is_empty() {
        ["title", "summary", "keywords", "content", "recall_when"]
            .into_iter()
            .map(str::to_string)
            .collect()
    } else {
        fields
    };
    let terms = request.terms.unwrap_or(SearchTerms {
        include: Vec::new(),
        exclude: Vec::new(),
    });

    let query = request.query.unwrap_or_default();
    let terms = SearchTermsPlan {
        all: terms
            .include
            .into_iter()
            .map(|value| value.trim().to_string())
            .collect(),
        none: terms
            .exclude
            .into_iter()
            .map(|value| value.trim().to_string())
            .collect(),
        any: Vec::new(),
    };
    let semantic_query = build_semantic_query(&query, &terms);

    Ok(SearchPlan {
        query,
        semantic_query,
        effective_limit: request.limit.unwrap_or(default_limit).min(default_limit),
        fields,
        terms,
    })
}

async fn search_literal_candidates(
    context: &super::ToolContext<'_>,
    plan: &SearchPlan,
) -> ToolResult<SearchOutcome> {
    // What：从 memory_search_index 做第一段字面候选召回。
    // Why：搜索必须消费派生投影表，避免运行时重复 join 工作态表和关键词表。
    let field_enabled = |name: &str| plan.fields.iter().any(|field| field == name);
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Vec<String>, f32)>(
        r#"
        WITH scoped AS (
            SELECT *,
                concat_ws(' ',
                    CASE WHEN $2 THEN title_text END,
                    CASE WHEN $3 THEN summary_text END,
                    CASE WHEN $4 THEN keywords_text END,
                    CASE WHEN $5 THEN content_text END,
                    CASE WHEN $6 THEN recall_when_text END
                ) AS search_text
            FROM memory_search_index
            WHERE status IN ('active', 'pending')
        )
        SELECT memory_uuid::text, title_text, status, summary_text,
            CASE WHEN $11 AND $5 AND (content_text % $1 OR strpos(lower(content_text), lower($1)) > 0
                    OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(content_text), lower(term)) > 0))
                THEN left(content_text, 120)
            END AS content_preview,
            ARRAY_REMOVE(ARRAY[
                CASE WHEN $2 AND EXISTS (SELECT 1 FROM unnest($7::text[]) t WHERE strpos(lower(title_text), lower(t)) > 0) THEN 'title' END,
                CASE WHEN $3 AND EXISTS (SELECT 1 FROM unnest($7::text[]) t WHERE strpos(lower(summary_text), lower(t)) > 0) THEN 'summary' END,
                CASE WHEN $4 AND EXISTS (SELECT 1 FROM unnest($7::text[]) t WHERE strpos(lower(keywords_text), lower(t)) > 0) THEN 'keywords' END,
                CASE WHEN $5 AND EXISTS (SELECT 1 FROM unnest($7::text[]) t WHERE strpos(lower(content_text), lower(t)) > 0) THEN 'content' END,
                CASE WHEN $6 AND EXISTS (SELECT 1 FROM unnest($7::text[]) t WHERE strpos(lower(recall_when_text), lower(t)) > 0) THEN 'recall_when' END
            ]::text[], NULL) AS matched_fields,
            (
                SELECT count(DISTINCT lower(term))::real
                FROM regexp_split_to_table($1, '\s+') AS query_terms(term)
                WHERE btrim(term) <> '' AND strpos(lower(search_text), lower(term)) > 0
            ) + GREATEST(
                CASE WHEN $2 THEN similarity(title_text, $1) ELSE 0 END,
                CASE WHEN $3 THEN similarity(summary_text, $1) ELSE 0 END,
                CASE WHEN $4 THEN similarity(keywords_text, $1) ELSE 0 END,
                CASE WHEN $5 THEN similarity(content_text, $1) ELSE 0 END,
                CASE WHEN $6 THEN similarity(recall_when_text, $1) ELSE 0 END
            ) AS score
        FROM scoped
        WHERE NOT EXISTS (SELECT 1 FROM unnest($7::text[]) AS terms(term) WHERE NOT (
                ($2 AND strpos(lower(title_text), lower(term)) > 0)
                OR ($3 AND strpos(lower(summary_text), lower(term)) > 0)
                OR ($4 AND strpos(lower(keywords_text), lower(term)) > 0)
                OR ($5 AND strpos(lower(content_text), lower(term)) > 0)
                OR ($6 AND strpos(lower(recall_when_text), lower(term)) > 0)))
            AND NOT EXISTS (SELECT 1 FROM unnest($8::text[]) AS terms(term) WHERE
                ($2 AND strpos(lower(title_text), lower(term)) > 0)
                OR ($3 AND strpos(lower(summary_text), lower(term)) > 0)
                OR ($4 AND strpos(lower(keywords_text), lower(term)) > 0)
                OR ($5 AND strpos(lower(content_text), lower(term)) > 0)
                OR ($6 AND strpos(lower(recall_when_text), lower(term)) > 0))
            AND (cardinality($9::text[]) = 0 OR EXISTS (SELECT 1 FROM unnest($9::text[]) AS terms(term) WHERE strpos(lower(search_text), lower(term)) > 0))
        ORDER BY score DESC, title_text ASC, memory_uuid ASC
        LIMIT $10
        "#,
    )
    .bind(&plan.query)
    .bind(field_enabled("title"))
    .bind(field_enabled("summary"))
    .bind(field_enabled("keywords"))
    .bind(field_enabled("content"))
    .bind(field_enabled("recall_when"))
    .bind(&plan.terms.all)
    .bind(&plan.terms.none)
    .bind(&plan.terms.any)
    .bind(CANDIDATE_BUDGET)
    .bind(!plan.query.is_empty())
    .fetch_all(context.profile_pool)
    .await?;
    Ok(SearchOutcome {
        rerank: false,
        errors: Vec::new(),
        results: rows
            .into_iter()
            .map(
                |(
                    memory_uuid,
                    title_norm,
                    status,
                    summary,
                    content_preview,
                    matched_fields,
                    score,
                )| {
                    SearchCandidate {
                        memory_uuid,
                        title_norm,
                        status,
                        summary,
                        content_preview,
                        matched_fields,
                        score,
                        vector_range: Vec::new(),
                    }
                },
            )
            .collect(),
    })
}

async fn search_embedding_fallback(
    context: &super::ToolContext<'_>,
    plan: &SearchPlan,
) -> ToolResult<SearchOutcome> {
    // What：独立使用 query 生成向量，并按记忆选择当前索引的候选。
    // Why：模型请求不持表锁，查询阶段锁表并核对物理代，避免旧向量查询已切换的新表。
    let settings = context.embedding_settings.ok_or("embedding 配置缺失")?;
    let (table_oid, model, dimension, fallback_max_distance) =
        crate::psql::read_current_embedding_config(context.profile_pool).await?;
    // Why：不能将文件中的新模型或维度与调用方保留的旧 API 参数混用。
    if settings.model != model || i32::try_from(settings.dimension).ok() != Some(dimension) {
        return Err("embedding 调用配置与文件配置不一致，请重新读取配置后重试".into());
    }
    let settings = crate::config::EmbeddingSettings {
        api: settings.api.clone(),
        api_type: settings.api_type.clone(),
        key: settings.key.clone(),
        model,
        dimension: usize::try_from(dimension)?,
        fallback_max_distance,
        proxy: settings.proxy.clone(),
    };
    let embedding =
        crate::provider::embedding::request_embedding(&settings, &plan.semantic_query).await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;
    let vector = format!(
        "[{}]",
        embedding
            .iter()
            .map(f32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut tx = context.profile_pool.begin().await?;
    sqlx::query("LOCK TABLE memory_embeddings IN ACCESS SHARE MODE").execute(&mut *tx).await?;
    let current_oid: i64 = sqlx::query_scalar("SELECT 'memory_embeddings'::regclass::oid::bigint")
        .fetch_one(&mut *tx).await?;
    if current_oid != table_oid {
        return Err("查询向量生成期间索引已切换，请重试查询".into());
    }
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Vec<String>, f32, Vec<i64>, Vec<i64>)>(
        r#"
        WITH scoped AS (
            SELECT i.*, e.chunk_index, e.content_start_byte, e.content_end_byte,
                (e.embedding <=> $1::vector) AS distance
            FROM memory_embeddings e
            JOIN memory_search_index i ON i.memory_uuid = e.memory_uuid
            JOIN memory_units u ON u.uuid = e.memory_uuid AND u.revision = e.source_revision
            WHERE i.status IN ('active', 'pending')
                AND e.embedding_model = $2 AND e.embedding_dimension = $3
        )
        SELECT memory_uuid::text, title_text, status, summary_text,
            NULL::text AS content_preview, ARRAY[]::text[] AS matched_fields, (1 - distance)::real AS score,
            starts, ends
        FROM (
            SELECT DISTINCT ON (memory_uuid) *,
                array_agg(content_start_byte) OVER hits AS starts,
                array_agg(content_end_byte) OVER hits AS ends
            FROM scoped WHERE distance <= $5
            WINDOW hits AS (PARTITION BY memory_uuid ORDER BY chunk_index
                ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)
            ORDER BY memory_uuid, distance ASC, chunk_index ASC
        ) memories
        ORDER BY distance ASC, title_text ASC, memory_uuid ASC
        LIMIT $4
        "#,
    )
    .bind(vector)
    .bind(&settings.model)
    .bind(settings.dimension as i32)
    .bind(CANDIDATE_BUDGET)
    .bind(settings.fallback_max_distance)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(SearchOutcome {
        rerank: false,
        errors: Vec::new(),
        results: rows
            .into_iter()
            .map(
                |(
                    memory_uuid,
                    title_norm,
                    status,
                    summary,
                    content_preview,
                    matched_fields,
                    score,
                    start,
                    end,
                )| {
                    SearchCandidate {
                        memory_uuid,
                        title_norm,
                        status,
                        summary,
                        content_preview,
                        matched_fields,
                        score,
                        vector_range: start.into_iter().zip(end).map(|(start, end)|
                            (usize::try_from(start).expect("块起点非负"), usize::try_from(end).expect("块终点非负"))
                        ).collect(),
                    }
                },
            )
            .collect(),
    })
}

async fn rerank_candidates(
    context: &super::ToolContext<'_>,
    plan: &SearchPlan,
    mut outcome: SearchOutcome,
) -> ToolResult<SearchOutcome> {
    // What：调用 rerank provider 重排已有候选。
    // Why：rerank 只能调整顺序，provider 失败或缺失配置时必须保留原召回结果。
    let Some(settings) = context.rerank_settings else {
        eprintln!("search_memory rerank_failed: configuration unavailable");
        outcome.errors.push("rerank_failed");
        return Ok(outcome);
    };
    let documents = match build_rerank_documents(context, plan, &outcome.results).await {
        Ok(documents) => documents,
        Err(_) => {
            eprintln!("search_memory rerank_failed: evidence preparation failed");
            outcome.errors.push("rerank_failed");
            return Ok(outcome);
        }
    };
    let Ok(ranked) =
        crate::provider::rerank::rerank_text_documents(settings, &plan.semantic_query, &documents)
            .await
    else {
        eprintln!("search_memory rerank_failed: provider request or score validation failed");
        outcome.errors.push("rerank_failed");
        return Ok(outcome);
    };
    if ranked.is_empty() {
        eprintln!("search_memory rerank_failed: provider returned no scores");
        outcome.errors.push("rerank_failed");
        return Ok(outcome);
    }
    let results = apply_rerank_results(outcome.results, ranked);
    Ok(SearchOutcome {
        results,
        rerank: true,
        errors: outcome.errors,
    })
}

fn apply_rerank_results(
    candidates: Vec<SearchCandidate>,
    ranked: Vec<crate::provider::rerank::RerankResult>,
) -> Vec<SearchCandidate> {
    // What：按 provider 排名重建候选列表，并丢弃接近 0 的 rerank 命中。
    // Why：rerank top_n 会返回低相关尾部，继续输出会让“候选”退化成噪声。
    let mut remaining = candidates.into_iter().map(Some).collect::<Vec<_>>();
    let mut results = Vec::with_capacity(remaining.len());
    for item in ranked {
        if let Some(slot) = remaining.get_mut(item.index)
            && let Some(mut candidate) = slot.take()
            && item.score >= RERANK_MIN_SCORE
        {
            candidate.score = item.score;
            results.push(candidate);
        }
    }
    results
}

fn print_search_response(
    context: &super::ToolContext<'_>,
    outcome: SearchOutcome,
) -> ToolResult<()> {
    // What：输出 search_memory 的成功 JSON 响应。
    // Why：统一在输出边界决定可见字段，避免召回和重排阶段拼装响应。
    let rerank = outcome.rerank;
    let failed = outcome.errors.contains(&"keyword_recall_failed")
        && outcome.errors.contains(&"vector_recall_failed");
    let error = match outcome.errors.as_slice() {
        [] => serde_json::Value::Null,
        [code] => serde_json::json!(code),
        codes => serde_json::json!(codes),
    };
    let results = outcome
        .results
        .into_iter()
        .map(|candidate| {
            let mut result = serde_json::json!({
                "memory_uuid": candidate.memory_uuid,
                "title_norm": candidate.title_norm,
                "status": candidate.status,
                "summary": candidate.summary,
                "matched_fields": candidate.matched_fields
            });
            if let Some(content_preview) = candidate.content_preview {
                result["content_preview"] = serde_json::json!(content_preview);
            }
            if rerank {
                result["score"] = serde_json::json!(candidate.score);
            }
            result
        })
        .collect::<Vec<_>>();
    let count = results.len();
    println!(
        "{}",
        serde_json::json!({
            "state": if failed { "error" } else { "success" },
            "tool": "search_memory",
            "data": {
                "results": results,
                "count": count
            },
            "error": error,
            "profile": context.profile
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        SearchCandidate, SearchTermsPlan, apply_rerank_results, build_semantic_query,
        parse_search_request,
    };
    use crate::provider::rerank::RerankResult;

    #[test]
    fn parse_search_request_accepts_supported_shapes() {
        let basic = serde_json::json!({"query": "  尼采深渊  ", "terms": {"include": ["尼采"]}, "limit": 8});
        let request = parse_search_request(&basic).unwrap();
        assert_eq!(request.query.as_deref(), Some("尼采深渊"));

        let advanced = serde_json::json!({
            "query": "导出 AND 保存 OR 归档",
            "terms": {"include": ["导出"], "exclude": []},
            "filters": []
        });
        let request = parse_search_request(&advanced).unwrap();
        assert_eq!(request.query.as_deref(), Some("导出 AND 保存 OR 归档"));
    }

    // Why：参数错误必须在搜索计划前拒绝，避免后续 SQL/provider 阶段承担输入边界。
    #[test]
    fn parse_search_request_rejects_invalid_inputs() {
        let cases = [
            serde_json::json!({"query": ""}),
            serde_json::json!({}),
            serde_json::json!({"query": "a AND b OR c"}),
            serde_json::json!({"query": "", "terms": {"include": ["a"], "exclude": []}, "filters": []}),
            serde_json::json!({"query": "a", "terms": {"include": ["a"], "exclude": []}, "filters": null}),
            serde_json::json!({"query": "a OR b", "terms": {}, "filters": []}),
            serde_json::json!({"query": "a", "terms": {}}),
            serde_json::json!({"query": "a", "filters": []}),
            serde_json::json!({"query": "a", "terms": {}, "filters": ["status"]}),
            serde_json::json!({"query": "a", "terms": {}, "filters": []}),
            serde_json::json!({"terms": {"all": [], "none": [], "any": ["a"]}, "filters": []}),
            serde_json::json!({"query": "a", "terms": {"include": [], "exclude": []}, "filters": []}),
            serde_json::json!({"query": "a", "terms": {"include": [""], "exclude": []}, "filters": []}),
            serde_json::json!({"query": "a", "limit": 0}),
            serde_json::json!({"query": "a", "mode": "semantic"}),
        ];

        for args in cases {
            assert!(parse_search_request(&args).is_err(), "{args}");
        }
    }

    #[test]
    fn build_semantic_query_uses_terms_when_query_is_absent() {
        let terms = SearchTermsPlan {
            all: vec!["饮酒".to_string()],
            none: vec!["送别".to_string()],
            any: vec!["月亮".to_string()],
        };

        assert_eq!(build_semantic_query("", &terms), "饮酒 月亮");
        assert_eq!(build_semantic_query("自然语言", &terms), "自然语言");
    }

    #[test]
    fn apply_rerank_results_filters_near_zero_scores() {
        let results = apply_rerank_results(
            vec![test_candidate("relevant"), test_candidate("noise")],
            vec![
                RerankResult {
                    index: 0,
                    score: 0.9,
                },
                RerankResult {
                    index: 1,
                    score: 0.0001,
                },
            ],
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title_norm, "relevant");
        assert_eq!(results[0].score, 0.9);
    }

    fn test_candidate(title_norm: &str) -> SearchCandidate {
        SearchCandidate {
            memory_uuid: title_norm.to_string(),
            title_norm: title_norm.to_string(),
            status: "active".to_string(),
            summary: String::new(),
            content_preview: None,
            matched_fields: Vec::new(),
            score: 0.0,
            vector_range: Vec::new(),
        }
    }
}
