// Why：AGE 是 SQL 主数据的派生层，所有工作态变更只需要留下一个可重建标记。
pub async fn mark_memory_graph_dirty(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO memory_graph_meta (graph_name, dirty, updated_at)
        VALUES ('memory_graph', true, now())
        ON CONFLICT (graph_name)
        DO UPDATE SET dirty = true, updated_at = EXCLUDED.updated_at
        "#,
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}

// Why：图状态必须能从 SQL 主数据自检，否则 rebuild 是否必要只能靠人工猜。
pub async fn get_memory_graph_status(
    pool: &sqlx::Pool<sqlx::Postgres>,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let row: String = sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object(
            'graph_name', 'memory_graph',
            'dirty', COALESCE((SELECT dirty FROM memory_graph_meta WHERE graph_name = 'memory_graph'), true),
            'updated_at', (SELECT updated_at::text FROM memory_graph_meta WHERE graph_name = 'memory_graph'),
            'memory_count', (SELECT count(*) FROM memory_units WHERE status = 'active'),
            'relation_count', (
                SELECT count(*)
                FROM memory_relations r
                JOIN memory_units f ON f.uuid = r.from_memory_uuid AND f.status = 'active'
                JOIN memory_units t ON t.uuid = r.to_memory_uuid AND t.status = 'active'
            )
        )::text
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(serde_json::from_str(&row)?)
}
