const REFRESH_MEMORY_SEARCH_INDEX_SQL: &str = r#"
WITH source AS (
    SELECT
        u.uuid AS memory_uuid,
        u.status,
        u.title_norm AS title_text,
        COALESCE(u.summary, '') AS summary_text,
        COALESCE(string_agg(k.keyword_norm, ' ' ORDER BY k.keyword_norm), '') AS keywords_text,
        u.content AS content_text,
        COALESCE(u.recall_when, '') AS recall_when_text
    FROM memory_units u
    LEFT JOIN memory_keywords k ON k.memory_uuid = u.uuid
    WHERE u.uuid = $1::uuid
    GROUP BY u.uuid, u.status, u.title_norm, u.summary, u.content, u.recall_when
)
INSERT INTO memory_search_index (
    memory_uuid,
    status,
    title_text,
    summary_text,
    keywords_text,
    content_text,
    recall_when_text,
    all_text,
    indexed_at
)
SELECT
    memory_uuid,
    status,
    title_text,
    summary_text,
    keywords_text,
    content_text,
    recall_when_text,
    concat_ws(' ', title_text, summary_text, keywords_text, content_text, recall_when_text),
    now()
FROM source
ON CONFLICT (memory_uuid) DO UPDATE SET
    status = EXCLUDED.status,
    title_text = EXCLUDED.title_text,
    summary_text = EXCLUDED.summary_text,
    keywords_text = EXCLUDED.keywords_text,
    content_text = EXCLUDED.content_text,
    recall_when_text = EXCLUDED.recall_when_text,
    all_text = EXCLUDED.all_text,
    indexed_at = EXCLUDED.indexed_at
"#;

#[allow(dead_code)]
pub(crate) async fn refresh_memory_search_index(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
) -> Result<(), sqlx::Error> {
    // What：从当前工作态重建单条 memory_search_index。
    // Why：搜索投影必须和 memory_units / memory_keywords 在同一事务内刷新，避免搜索读到旧文本。
    let result = sqlx::query(REFRESH_MEMORY_SEARCH_INDEX_SQL)
        .bind(memory_uuid)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}
