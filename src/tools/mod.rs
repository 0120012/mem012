mod create_memory;
mod delete_memory;

pub struct ToolContext<'a> {
    // Why：工具执行需要同时看到当前私库和共享库，但连接池生命周期应由 main 持有。
    pub profile: &'a str,
    pub profile_pool: &'a sqlx::Pool<sqlx::Postgres>,
    pub share_pool: &'a sqlx::Pool<sqlx::Postgres>,
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
        if key != "tool" && key != "args" {
            return Err(format!("未知字段: {key}").into());
        }
    }

    let tool = request
        .get("tool")
        .and_then(serde_json::Value::as_str)
        .ok_or("字段 tool 缺失或不是字符串")?;
    let args = request
        .get("args")
        .filter(|value| value.is_object())
        .ok_or("字段 args 缺失或不是 object")?;

    match tool {
        "create_memory" => create_memory::run(context, args).await,
        "delete_memory" => delete_memory::run(context, args).await,
        _ => Err(format!("未知工具: {tool}").into()),
    }
}
