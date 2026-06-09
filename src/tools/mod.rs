mod auth;
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
    pub category_index_list: &'a [String],
    pub api_base_url: &'a str,
    pub embedding_settings: Option<&'a crate::config::EmbeddingSettings>,
    pub rerank_settings: Option<&'a crate::config::RerankSettings>,
}

pub async fn dispatch_auth_command(
    server_addr: &str,
    auth_token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：分发顶层 `mem012 --auth` 命令。
    // Why：main 只处理 CLI 生命周期，具体 auth 命令仍归 tools 模块管理。
    auth::run(server_addr, auth_token).await
}

pub async fn dispatch_init_command(
    pool: &sqlx::Pool<sqlx::Postgres>,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：读取当前 profile 中用于 CLI init 的记忆内容。
    // Why：init 空结果也必须显式标记成功，避免调用方把裸数组误判为异常响应。
    let rows = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT title_norm, content
        FROM memory_units
        WHERE category = 'init' AND status <> 'trashed'
        ORDER BY title_norm ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    let results = rows
        .into_iter()
        .map(|(title_norm, content)| serde_json::json!({ "title_norm": title_norm, "content": content }))
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "init",
            "data": {
                "memories": results
            },
            "error": null
        })
    );
    Ok(())
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
