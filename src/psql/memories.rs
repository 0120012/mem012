#[derive(serde::Deserialize)]
pub struct MemoryUpdateInput {
    pub expected_revision: i64,
    pub title_norm: String,
    pub summary: Option<String>,
    pub recall_when: Option<String>,
    pub content: String,
    pub keywords: Vec<String>,
}

const LIST_MEMORIES_SQL: &str = r#"
SELECT COALESCE(
    jsonb_agg(
        jsonb_build_object(
            'memory_uuid', u.uuid::text,
            'category', u.category,
            'title_norm', u.title_norm,
            'revision', u.revision,
            'summary', u.summary,
            'content', u.content,
            'recall_when', u.recall_when,
            'status', u.status,
            'keywords', COALESCE((
                SELECT jsonb_agg(k.keyword_norm ORDER BY k.keyword_norm)
                FROM memory_keywords k
                WHERE k.memory_uuid = u.uuid
            ), '[]'::jsonb),
            'has_open_change', c.memory_uuid IS NOT NULL,
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
WHERE u.status = 'active'
"#;

const LIST_MEMORY_CATEGORY_KEYWORDS_SQL: &str = r#"
SELECT COALESCE(jsonb_agg(keyword_norm ORDER BY keyword_norm), '[]'::jsonb)::text
FROM (
    SELECT DISTINCT k.keyword_norm
    FROM memory_units u
    JOIN memory_keywords k ON k.memory_uuid = u.uuid
    LEFT JOIN memory_changes c ON c.memory_uuid = u.uuid
    WHERE u.status = 'active'
        AND u.category = $1
) keywords
"#;

const UPDATE_DUPLICATE_CHECK_SQL: &str = r#"
SELECT CASE
    WHEN EXISTS (SELECT 1 FROM memory_units WHERE uuid <> $1::uuid AND status IN ('pending', 'active') AND title_norm = normalize_title($2)) THEN 'title_norm'
    WHEN EXISTS (SELECT 1 FROM memory_units WHERE uuid <> $1::uuid AND status IN ('pending', 'active') AND content = $3) THEN 'content'
    WHEN $4::text IS NOT NULL AND EXISTS (SELECT 1 FROM memory_units WHERE uuid <> $1::uuid AND status IN ('pending', 'active') AND summary = $4) THEN 'summary'
END
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

// What：列出单个 category 下可展开的关键词。
// Why：侧栏关键词必须在点击 category 时按需读取，不能随记忆分组一次性加载完整层级。
pub async fn list_memory_category_keywords(
    database_url: &str,
    category: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let rows: String = sqlx::query_scalar(LIST_MEMORY_CATEGORY_KEYWORDS_SQL)
        .bind(category)
        .fetch_one(&pool)
        .await?;
    Ok(serde_json::from_str(&rows)?)
}

pub async fn update_memory(
    database_url: &str,
    memory_uuid: &str,
    input: MemoryUpdateInput,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    // What：把前端编辑后的完整记忆快照写回当前工作态。
    // Why：人工编辑也需要可恢复，必须保留最早 before_state，只覆盖当前 after_state。
    if input.expected_revision < 1 {
        return Err("MEMORY_UPDATE_INVALID: expected_revision is invalid".into());
    }
    let title = required_text("title_norm", input.title_norm)?;
    let content = required_nonempty_text("content", input.content)?;
    let summary = input
        .summary
        .map(|value| required_nonempty_text("summary", value))
        .transpose()?;
    let recall_when = input
        .recall_when
        .map(|value| required_nonempty_text("recall_when", value))
        .transpose()?;
    let keywords = normalize_keywords(input.keywords)?;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let mut tx = pool.begin().await?;
    let locked_revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM memory_units WHERE uuid = $1::uuid AND status = 'active' FOR UPDATE",
    )
    .bind(memory_uuid)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(locked_revision) = locked_revision else {
        tx.rollback().await?;
        return Ok(false);
    };
    if locked_revision != input.expected_revision {
        tx.rollback().await?;
        return Err("MEMORY_UPDATE_CONFLICT: revision mismatch".into());
    }
    let before_state: Option<String> = sqlx::query_scalar(
        "SELECT before_state::text FROM memory_changes WHERE memory_uuid = $1::uuid FOR UPDATE",
    )
    .bind(memory_uuid)
    .fetch_optional(&mut *tx)
    .await?;
    let before_state = match before_state {
        Some(state) => state,
        None => crate::psql::memory_state(&mut tx, memory_uuid).await?,
    };
    reject_update_duplicates(&mut tx, memory_uuid, &title, &content, summary.as_deref()).await?;
    sqlx::query("UPDATE memory_units SET title_norm = normalize_title($2), summary = $3, content = $4, recall_when = $5, updated_at = now() WHERE uuid = $1::uuid")
        .bind(memory_uuid)
        .bind(&title)
        .bind(summary.as_deref())
        .bind(&content)
        .bind(recall_when.as_deref())
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM memory_keywords WHERE memory_uuid = $1::uuid")
        .bind(memory_uuid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memory_keywords (uuid, memory_uuid, keyword_norm, weight, created_at) SELECT gen_random_uuid(), $1::uuid, keyword, NULL::int, now() FROM jsonb_array_elements_text($2::jsonb) AS keywords(keyword)")
        .bind(memory_uuid)
        .bind(serde_json::to_string(&keywords)?)
        .execute(&mut *tx)
        .await?;
    crate::psql::search_index::refresh_memory_search_index(&mut tx, memory_uuid).await?;
    let after_state = crate::psql::memory_state(&mut tx, memory_uuid).await?;
    sqlx::query("INSERT INTO memory_changes (uuid, memory_uuid, action, before_state, after_state, created_at, updated_at) VALUES ($1::uuid, $1::uuid, 'update', $2::jsonb, $3::jsonb, now(), now()) ON CONFLICT (memory_uuid) DO UPDATE SET after_state = EXCLUDED.after_state, updated_at = EXCLUDED.updated_at")
        .bind(memory_uuid)
        .bind(before_state)
        .bind(after_state)
        .execute(&mut *tx)
        .await?;
    crate::psql::mark_memory_graph_dirty(&mut tx).await?;
    tx.commit().await?;
    Ok(true)
}

async fn reject_update_duplicates(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    title: &str,
    content: &str,
    summary: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let field: Option<String> = sqlx::query_scalar(UPDATE_DUPLICATE_CHECK_SQL)
        .bind(memory_uuid)
        .bind(title)
        .bind(content)
        .bind(summary)
        .fetch_one(&mut **tx)
        .await?;
    if let Some(field) = field {
        return Err(format!("MEMORY_UPDATE_INVALID: {field} is duplicated").into());
    }
    Ok(())
}

fn required_nonempty_text(
    name: &str,
    value: String,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if value.trim().is_empty() {
        return Err(format!("MEMORY_UPDATE_INVALID: {name} is required").into());
    }
    Ok(value)
}

fn required_text(
    name: &str,
    value: String,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("MEMORY_UPDATE_INVALID: {name} is required").into());
    }
    Ok(value)
}

fn normalize_keywords(
    values: Vec<String>,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut keywords = Vec::new();
    for value in values {
        let keyword = value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if keyword.is_empty() {
            return Err("MEMORY_UPDATE_INVALID: keyword is invalid".into());
        }
        if keywords.iter().any(|existing| existing == &keyword) {
            return Err("MEMORY_UPDATE_INVALID: keyword is duplicated".into());
        }
        keywords.push(keyword);
    }
    if keywords.is_empty() {
        return Err("MEMORY_UPDATE_INVALID: keyword is required".into());
    }
    Ok(keywords)
}

#[cfg(test)]
mod tests {
    use super::UPDATE_DUPLICATE_CHECK_SQL;

    #[test]
    fn update_duplicate_check_covers_user_edit_fields() {
        assert!(UPDATE_DUPLICATE_CHECK_SQL.contains("title_norm = normalize_title($2)"));
        assert!(UPDATE_DUPLICATE_CHECK_SQL.contains("content = $3"));
        assert!(UPDATE_DUPLICATE_CHECK_SQL.contains("summary = $4"));
    }
}
