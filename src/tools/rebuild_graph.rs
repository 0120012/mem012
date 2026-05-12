// Why：图是 SQL 工作态的派生结果，显式工具入口比 approve 时隐式重建更容易验证。
pub async fn run(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if !args.as_object().is_some_and(serde_json::Map::is_empty) {
        return Err("rebuild_graph 不接受参数".into());
    }

    crate::psql::rebuild_memory_graph(context.profile_pool).await?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "rebuild_graph",
            "data": {
                "graph": "memory_graph"
            },
            "error": null,
            "meta": {
                "spec_version": "v10",
                "profile": context.profile
            }
        })
    );
    Ok(())
}
