// What：承载 search_memory 搜索工具入口。
// Why：先把工具路由接通，后续查询逻辑集中在独立模块里迭代。
use serde::Deserialize;

type ToolResult<T> = Result<T, Box<dyn std::error::Error>>;

pub async fn run(context: &super::ToolContext<'_>, args: &serde_json::Value) -> ToolResult<()> {
    // What：串联 search_memory 的参数、召回、重排和响应阶段。
    // Why：先固定执行骨架，避免后续实现时把输入校验、查询和输出格式混在一起。
    let request = parse_search_request(args)?;
    let plan = build_search_plan(context, request)?;
    let mut outcome = search_literal_candidates(context, &plan).await?;

    if outcome.results.is_empty() {
        outcome = search_embedding_fallback(context, &plan).await?;
    }
    if outcome.results.len() > 1 {
        outcome = rerank_candidates(context, &plan, outcome).await?;
    }

    print_search_response(context, outcome)
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchRequest {
    query: String,
    limit: Option<i32>,
    terms: Option<SearchTerms>,
    filters: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchTerms {
    all: Option<Vec<String>>,
    none: Option<Vec<String>>,
    any: Option<Vec<String>>,
}

#[allow(dead_code)]
struct SearchPlan {
    query: String,
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

struct SearchCandidate;

struct SearchOutcome {
    results: Vec<SearchCandidate>,
}

fn parse_search_request(args: &serde_json::Value) -> ToolResult<SearchRequest> {
    // 1. 按 SearchRequest schema 解析 params；类型不匹配或包含未知字段会被 serde 拒绝。
    let mut request = serde_json::from_value::<SearchRequest>(args.clone())?;

    // 2. 规范化 query 的首尾空白；搜索入口不能接受空查询。
    request.query = request.query.trim().to_string();
    if request.query.is_empty() {
        return Err("query 不能为空".into());
    }

    // 3. 校验 limit 下界；超过默认值的截断留给 build_search_plan 使用 context 处理。
    if matches!(request.limit, Some(limit) if limit < 1) {
        return Err("limit 必须大于 0".into());
    }

    // 4. 只要 terms 或 filters 任一出现，就进入高级搜索参数规则。
    let advanced = request.terms.is_some() || request.filters.is_some();

    // 5. 高级搜索要求 terms 和 filters 成对出现；允许 terms={} 和 filters=[]。
    if request.terms.is_some() != request.filters.is_some() {
        return Err("terms 和 filters 必须同时传入".into());
    }

    // 6. query 只承载自然语言意图；高级搜索的布尔逻辑必须写入 terms。
    validate_query_logic(&request.query, advanced)?;

    // 7. filters 只能来自文档定义的可搜索字段白名单。
    if let Some(filters) = &request.filters {
        validate_filters(filters)?;
    }

    // 8. terms 内部数组如果出现，必须非空且不能包含空关键词。
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
    if (advanced && (has_and || has_or)) || (!advanced && has_and && has_or) {
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
    for (field, values) in [
        ("terms.all", terms.all.as_ref()),
        ("terms.none", terms.none.as_ref()),
        ("terms.any", terms.any.as_ref()),
    ] {
        if let Some(values) = values {
            if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
                return Err(format!("{field} 不能为空").into());
            }
        }
    }
    Ok(())
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
        all: None,
        none: None,
        any: None,
    });

    Ok(SearchPlan {
        query: request.query,
        effective_limit: request.limit.unwrap_or(default_limit).min(default_limit),
        fields,
        terms: SearchTermsPlan {
            all: terms
                .all
                .unwrap_or_default()
                .into_iter()
                .map(|value| value.trim().to_string())
                .collect(),
            none: terms
                .none
                .unwrap_or_default()
                .into_iter()
                .map(|value| value.trim().to_string())
                .collect(),
            any: terms
                .any
                .unwrap_or_default()
                .into_iter()
                .map(|value| value.trim().to_string())
                .collect(),
        },
    })
}

async fn search_literal_candidates(
    _context: &super::ToolContext<'_>,
    _plan: &SearchPlan,
) -> ToolResult<SearchOutcome> {
    Err("search_memory 字面召回尚未实现".into())
}

async fn search_embedding_fallback(
    _context: &super::ToolContext<'_>,
    _plan: &SearchPlan,
) -> ToolResult<SearchOutcome> {
    Err("search_memory embedding 保底尚未实现".into())
}

async fn rerank_candidates(
    _context: &super::ToolContext<'_>,
    _plan: &SearchPlan,
    _outcome: SearchOutcome,
) -> ToolResult<SearchOutcome> {
    Err("search_memory rerank 尚未实现".into())
}

fn print_search_response(
    _context: &super::ToolContext<'_>,
    _outcome: SearchOutcome,
) -> ToolResult<()> {
    Err("search_memory 响应输出尚未实现".into())
}

#[cfg(test)]
mod tests {
    use super::parse_search_request;

    #[test]
    fn parse_search_request_accepts_supported_shapes() {
        let basic = serde_json::json!({"query": "  尼采深渊  ", "limit": 8});
        let request = parse_search_request(&basic).unwrap();
        assert_eq!(request.query, "尼采深渊");

        let advanced = serde_json::json!({"query": "微信读书导出", "terms": {}, "filters": []});
        assert!(parse_search_request(&advanced).is_ok());
    }

    // Why：参数错误必须在搜索计划前拒绝，避免后续 SQL/provider 阶段承担输入边界。
    #[test]
    fn parse_search_request_rejects_invalid_inputs() {
        let cases = [
            serde_json::json!({"query": ""}),
            serde_json::json!({"query": "a AND b OR c"}),
            serde_json::json!({"query": "a OR b", "terms": {}, "filters": []}),
            serde_json::json!({"query": "a", "terms": {}}),
            serde_json::json!({"query": "a", "filters": []}),
            serde_json::json!({"query": "a", "terms": {}, "filters": ["status"]}),
            serde_json::json!({"query": "a", "terms": {"all": []}, "filters": []}),
            serde_json::json!({"query": "a", "terms": {"any": [""]}, "filters": []}),
            serde_json::json!({"query": "a", "limit": 0}),
            serde_json::json!({"query": "a", "mode": "semantic"}),
        ];

        for args in cases {
            assert!(parse_search_request(&args).is_err(), "{args}");
        }
    }
}
