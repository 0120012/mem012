mod create_memory;
mod delete_memory;
mod read_memory;
mod search_memory;
mod update_memory;

pub struct ToolContext<'a> {
    // Why：工具执行只操作当前 profile，连接池生命周期应由 main 持有。
    pub profile: &'a str,
    pub profile_pool: &'a sqlx::Pool<sqlx::Postgres>,
    pub search_default_limit: i32,
    pub embedding_settings: Option<&'a crate::config::EmbeddingSettings>,
    pub rerank_settings: Option<&'a crate::config::RerankSettings>,
}

pub async fn dispatch_tool_request(
    context: &ToolContext<'_>,
    request_args: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：工具层统一校验 canonical 外壳，具体工具只需要处理自己的 args。
    let request = request_args
        .as_object()
        .ok_or("工具请求必须是 JSON object")?;

    for key in request.keys() {
        if key != "tool" && key != "params" {
            return Err(format!("未知字段: {key}").into());
        }
    }

    let tool = request
        .get("tool")
        .and_then(serde_json::Value::as_str)
        .ok_or("字段 tool 缺失或不是字符串")?;
    let args = request
        .get("params")
        .filter(|value| value.is_object())
        .ok_or("字段 params 缺失或不是 object")?;

    match tool {
        "create_memory" => create_memory::run(context, args).await,
        "delete_memory" => delete_memory::run(context, args).await,
        "read_memory" | "read_memory_hash" => read_memory::run(context, tool, args).await,
        "search_memory" => search_memory::run(context, args).await,
        "update_memory_replace"
        | "update_memory_patch_content"
        | "update_memory_append"
        | "update_memory_add_keywords"
        | "update_memory_remove_keywords" => update_memory::run(context, tool, args).await,
        _ => Err(format!("未知工具: {tool}").into()),
    }
}
