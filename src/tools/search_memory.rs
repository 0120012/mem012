// What：承载 search_memory 搜索工具入口。
// Why：先把工具路由接通，后续查询逻辑集中在独立模块里迭代。
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

struct SearchRequest;

struct SearchPlan;

struct SearchCandidate;

struct SearchOutcome {
    results: Vec<SearchCandidate>,
}

fn parse_search_request(_args: &serde_json::Value) -> ToolResult<SearchRequest> {
    Err("search_memory 参数解析尚未实现".into())
}

fn build_search_plan(
    _context: &super::ToolContext<'_>,
    _request: SearchRequest,
) -> ToolResult<SearchPlan> {
    Err("search_memory 搜索计划尚未实现".into())
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
