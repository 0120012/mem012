// What：承载 search_memory 搜索工具入口。
// Why：先把工具路由接通，后续查询逻辑集中在独立模块里迭代。
use serde::Deserialize;

type ToolResult<T> = Result<T, Box<dyn std::error::Error>>;

const RERANK_MIN_SCORE: f32 = 0.1;

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
}

struct SearchOutcome {
    results: Vec<SearchCandidate>,
    embedding_fallback: bool,
    rerank: bool,
}

pub async fn run(context: &super::ToolContext<'_>, args: &serde_json::Value) -> ToolResult<()> {
    // What：串联 search_memory 的参数、召回、重排和响应阶段。
    // Why：先固定执行骨架，避免后续实现时把输入校验、查询和输出格式混在一起。
    let request = parse_search_request(args)?;
    let plan = build_search_plan(context, request)?;
    let has_semantic_query = !plan.semantic_query.is_empty();
    // 字面召回是第一阶段；只有 0 命中时才进入 embedding 保底。
    let mut outcome = search_literal_candidates(context, &plan).await?;

    // 保底召回只在字面搜索完全无结果时触发，不能扩大已有候选集合。
    if has_semantic_query && outcome.results.is_empty() {
        outcome = search_embedding_fallback(context, &plan).await?;
    }
    if has_semantic_query && outcome.results.len() > 1 {
        outcome = rerank_candidates(context, &plan, outcome).await?;
    }

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

    // 4. 只要 terms 或 filters 任一出现，就进入高级搜索参数规则。
    let advanced = request.terms.is_some() || request.filters.is_some();

    // 5. 高级搜索要求 terms 和 filters 成对出现；terms 内 include/exclude 必须都出现。
    if request.terms.is_some() != request.filters.is_some() {
        return Err("terms 和 filters 必须同时传入".into());
    }
    if advanced && request.query.is_some() {
        return Err("高级搜索不能传 query".into());
    }
    if !advanced && request.query.as_deref().unwrap_or_default().is_empty() {
        return Err("query 不能为空".into());
    }

    // 6. 基础搜索 query 只承载自然语言意图；高级搜索的布尔逻辑必须写入 terms。
    validate_query_logic(request.query.as_deref().unwrap_or_default(), advanced)?;

    // 7. filters 只能来自文档定义的可搜索字段白名单。
    if let Some(filters) = &request.filters {
        validate_filters(filters)?;
    }

    // 8. terms 可以带空数组，但 all/none/any 三组里至少要有一个有效关键词。
    if let Some(terms) = &request.terms {
        validate_terms(terms)?;
    }
    Ok(request)
}

fn validate_query_logic(query: &str, advanced: bool) -> ToolResult<()> {
    let has_and = query
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("AND"));
    let has_or = query
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("OR"));
    let invalid_logic = if advanced {
        has_and || has_or
    } else {
        has_and && has_or
    };
    if invalid_logic {
        Err("query 不能包含非法 AND/OR 逻辑".into())
    } else {
        Ok(())
    }
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
        all: Vec::new(),
        none: terms
            .exclude
            .into_iter()
            .map(|value| value.trim().to_string())
            .collect(),
        any: terms
            .include
            .into_iter()
            .map(|value| value.trim().to_string())
            .collect(),
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
            WHERE status <> 'trashed'
        )
        SELECT memory_uuid::text, title_text, status, summary_text,
            CASE WHEN $11 AND $5 AND (content_text % $1 OR strpos(lower(content_text), lower($1)) > 0
                    OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(content_text), lower(term)) > 0))
                THEN left(content_text, 120)
            END AS content_preview,
            ARRAY_REMOVE(ARRAY[
                CASE WHEN $11 AND $2 AND (title_text % $1 OR strpos(lower(title_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(title_text), lower(term)) > 0)) THEN 'title' END,
                CASE WHEN $11 AND $3 AND (summary_text % $1 OR strpos(lower(summary_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(summary_text), lower(term)) > 0)) THEN 'summary' END,
                CASE WHEN $11 AND $4 AND (keywords_text % $1 OR strpos(lower(keywords_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(keywords_text), lower(term)) > 0)) THEN 'keywords' END,
                CASE WHEN $11 AND $5 AND (content_text % $1 OR strpos(lower(content_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(content_text), lower(term)) > 0)) THEN 'content' END,
                CASE WHEN $11 AND $6 AND (recall_when_text % $1 OR strpos(lower(recall_when_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(recall_when_text), lower(term)) > 0)) THEN 'recall_when' END
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
        WHERE (NOT $11 OR (
                ($2 AND (title_text % $1 OR strpos(lower(title_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(title_text), lower(term)) > 0)))
                OR ($3 AND (summary_text % $1 OR strpos(lower(summary_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(summary_text), lower(term)) > 0)))
                OR ($4 AND (keywords_text % $1 OR strpos(lower(keywords_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(keywords_text), lower(term)) > 0)))
                OR ($5 AND (content_text % $1 OR strpos(lower(content_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(content_text), lower(term)) > 0)))
                OR ($6 AND (recall_when_text % $1 OR strpos(lower(recall_when_text), lower($1)) > 0 OR EXISTS (SELECT 1 FROM regexp_split_to_table($1, '\s+') AS query_terms(term) WHERE strpos(lower(recall_when_text), lower(term)) > 0)))
            ))
            AND NOT EXISTS (SELECT 1 FROM unnest($7::text[]) AS terms(term) WHERE strpos(lower(search_text), lower(term)) = 0)
            AND NOT EXISTS (SELECT 1 FROM unnest($8::text[]) AS terms(term) WHERE strpos(lower(search_text), lower(term)) > 0)
            AND (cardinality($9::text[]) = 0 OR EXISTS (SELECT 1 FROM unnest($9::text[]) AS terms(term) WHERE strpos(lower(search_text), lower(term)) > 0))
        ORDER BY score DESC, title_text ASC
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
    .bind(plan.effective_limit)
    .bind(!plan.query.is_empty())
    .fetch_all(context.profile_pool)
    .await?;
    Ok(SearchOutcome {
        embedding_fallback: false,
        rerank: false,
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
    // What：在字面召回为 0 时，用语义输入 embedding 从已生成的向量索引召回候选。
    // Why：embedding 只是保底路径，仍必须复用 memory_search_index 执行状态、字段和 terms 边界。
    let Some(settings) = context.embedding_settings else {
        return Ok(SearchOutcome {
            results: vec![],
            embedding_fallback: false,
            rerank: false,
        });
    };
    let Ok(embedding) =
        crate::provider::embedding::request_embedding(settings, &plan.semantic_query).await
    else {
        return Ok(SearchOutcome {
            results: vec![],
            embedding_fallback: false,
            rerank: false,
        });
    };
    let vector = format!(
        "[{}]",
        embedding
            .iter()
            .map(f32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let field_enabled = |name: &str| plan.fields.iter().any(|field| field == name);
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Vec<String>, f32)>(
        r#"
        WITH scoped AS (
            SELECT i.*, (e.embedding <=> $1::vector) AS distance,
                concat_ws(' ',
                    CASE WHEN $2 THEN i.title_text END,
                    CASE WHEN $3 THEN i.summary_text END,
                    CASE WHEN $4 THEN i.keywords_text END,
                    CASE WHEN $5 THEN i.content_text END,
                    CASE WHEN $6 THEN i.recall_when_text END
                ) AS search_text
            FROM memory_embeddings e
            JOIN memory_search_index i ON i.memory_uuid = e.memory_uuid
            WHERE i.status = 'active'
                AND e.embedding_model = $10
                AND e.embedding_dimension = $11
        )
        SELECT memory_uuid::text, title_text, status, summary_text,
            NULL::text AS content_preview, ARRAY[]::text[] AS matched_fields, (1 - distance)::real AS score
        FROM scoped
        WHERE NOT EXISTS (SELECT 1 FROM unnest($7::text[]) AS terms(term) WHERE strpos(lower(search_text), lower(term)) = 0)
            AND NOT EXISTS (SELECT 1 FROM unnest($8::text[]) AS terms(term) WHERE strpos(lower(search_text), lower(term)) > 0)
            AND (cardinality($9::text[]) = 0 OR EXISTS (SELECT 1 FROM unnest($9::text[]) AS terms(term) WHERE strpos(lower(search_text), lower(term)) > 0))
            AND distance <= $13
        ORDER BY distance ASC, title_text ASC
        LIMIT $12
        "#,
    )
    .bind(vector)
    .bind(field_enabled("title"))
    .bind(field_enabled("summary"))
    .bind(field_enabled("keywords"))
    .bind(field_enabled("content"))
    .bind(field_enabled("recall_when"))
    .bind(&plan.terms.all)
    .bind(&plan.terms.none)
    .bind(&plan.terms.any)
    .bind(&settings.model)
    .bind(settings.dimension as i32)
    .bind(plan.effective_limit)
    .bind(settings.fallback_max_distance)
    .fetch_all(context.profile_pool)
    .await?;
    Ok(SearchOutcome {
        embedding_fallback: true,
        rerank: false,
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
                    }
                },
            )
            .collect(),
    })
}

async fn rerank_candidates(
    context: &super::ToolContext<'_>,
    plan: &SearchPlan,
    outcome: SearchOutcome,
) -> ToolResult<SearchOutcome> {
    // What：调用 rerank provider 重排已有候选。
    // Why：rerank 只能调整顺序，provider 失败或缺失配置时必须保留原召回结果。
    let Some(settings) = context.rerank_settings else {
        return Ok(outcome);
    };
    let documents = outcome
        .results
        .iter()
        .map(|candidate| match candidate.content_preview.as_deref() {
            Some(content_preview) => format!(
                "{}\n{}\n{}",
                candidate.title_norm, candidate.summary, content_preview
            ),
            None => format!("{}\n{}", candidate.title_norm, candidate.summary),
        })
        .collect::<Vec<_>>();
    let Ok(ranked) =
        crate::provider::rerank::rerank_text_documents(settings, &plan.semantic_query, &documents)
            .await
    else {
        return Ok(outcome);
    };
    if ranked.is_empty() {
        return Ok(outcome);
    }
    let embedding_fallback = outcome.embedding_fallback;
    let results = apply_rerank_results(outcome.results, ranked);
    Ok(SearchOutcome {
        results,
        embedding_fallback,
        rerank: true,
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
        {
            if item.score >= RERANK_MIN_SCORE {
                candidate.score = item.score;
                results.push(candidate);
            }
        }
    }
    results.extend(remaining.into_iter().flatten());
    results
}

fn print_search_response(
    context: &super::ToolContext<'_>,
    outcome: SearchOutcome,
) -> ToolResult<()> {
    // What：输出 search_memory 的成功 JSON 响应。
    // Why：统一在输出边界决定可见字段，避免召回和重排阶段拼装响应。
    let embedding_fallback = outcome.embedding_fallback;
    let rerank = outcome.rerank;
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
            if rerank || embedding_fallback {
                result["score"] = serde_json::json!(candidate.score);
            }
            result
        })
        .collect::<Vec<_>>();
    let count = results.len();
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "search_memory",
            "data": {
                "strategy": { "embedding_fallback": embedding_fallback, "rerank": rerank },
                "results": results,
                "count": count
            },
            "error": null,
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
        let basic = serde_json::json!({"query": "  尼采深渊  ", "limit": 8});
        let request = parse_search_request(&basic).unwrap();
        assert_eq!(request.query.as_deref(), Some("尼采深渊"));

        let advanced = serde_json::json!({
            "terms": {"include": ["导出"], "exclude": []},
            "filters": []
        });
        let request = parse_search_request(&advanced).unwrap();
        assert_eq!(request.query, None);
    }

    // Why：参数错误必须在搜索计划前拒绝，避免后续 SQL/provider 阶段承担输入边界。
    #[test]
    fn parse_search_request_rejects_invalid_inputs() {
        let cases = [
            serde_json::json!({"query": ""}),
            serde_json::json!({}),
            serde_json::json!({"query": "a AND b OR c"}),
            serde_json::json!({"query": "", "terms": {"include": ["a"], "exclude": []}, "filters": []}),
            serde_json::json!({"query": "a", "terms": {"include": ["a"], "exclude": []}, "filters": []}),
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
        }
    }
}
