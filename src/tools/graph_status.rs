// Why：rebuild_graph 会改 AGE 派生层，先提供只读状态工具用于判断是否需要执行。
pub async fn run(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if !args.as_object().is_some_and(serde_json::Map::is_empty) {
        return Err("graph_status 不接受参数".into());
    }

    let status = crate::psql::get_memory_graph_status(context.profile_pool)
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error })?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "graph_status",
            "data": status,
            "error": null,
            "meta": {
                "spec_version": "v10",
                "profile": context.profile
            }
        })
    );
    Ok(())
}
