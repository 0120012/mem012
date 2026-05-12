const LIST_MEMORIES_SQL: &str = r#"
SELECT COALESCE(
    jsonb_agg(
        jsonb_build_object(
            'memory_uuid', u.uuid::text,
            'category', u.category,
            'title_norm', u.title_norm,
            'summary', u.summary,
            'status', u.status,
            'has_open_change', c.uuid IS NOT NULL,
            'change_action', c.action,
            'created_at', u.created_at::text,
            'updated_at', u.updated_at::text
        )
        ORDER BY u.updated_at DESC
    ),
    '[]'::jsonb
)::text
FROM memory_units u
LEFT JOIN memory_changes c ON c.memory_uuid = u.uuid
WHERE c.uuid IS NULL
"#;

// Why：列表展示查询放在 psql 层，避免 HTTP handler 持有 memory_changes 派生规则。
pub async fn list_memories(
    database_url: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let rows: String = sqlx::query_scalar(LIST_MEMORIES_SQL)
        .fetch_one(&pool)
        .await?;
    Ok(serde_json::from_str(&rows)?)
}
