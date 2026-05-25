const LIST_CHANGES_SQL: &str = r#"
SELECT COALESCE(
    jsonb_agg(
        jsonb_build_object(
            'memory_uuid', c.memory_uuid::text,
            'action', c.action,
            'title_norm', COALESCE(u.title_norm, c.after_state #>> '{memory,title_norm}'),
            'summary', COALESCE(u.summary, c.after_state #>> '{memory,summary}'),
            'created_at', c.created_at::text,
            'updated_at', c.updated_at::text
        )
        ORDER BY c.updated_at DESC
    ),
    '[]'::jsonb
)::text
FROM memory_changes c
LEFT JOIN memory_units u ON u.uuid = c.memory_uuid
"#;

const CHANGE_DETAIL_SQL: &str = r#"
SELECT jsonb_build_object(
    'memory_uuid', c.memory_uuid::text,
    'action', c.action,
    'title_norm', COALESCE(u.title_norm, c.after_state #>> '{memory,title_norm}'),
    'summary', COALESCE(u.summary, c.after_state #>> '{memory,summary}'),
    'before_state', c.before_state,
    'after_state', c.after_state,
    'created_at', c.created_at::text,
    'updated_at', c.updated_at::text
)::text
FROM memory_changes c
LEFT JOIN memory_units u ON u.uuid = c.memory_uuid
WHERE c.memory_uuid = $1::uuid
"#;

pub struct ApprovedEmbedding {
    pub model: String,
    pub dimension: i32,
    pub values: Vec<f32>,
}

// Why：changes 列表只暴露审查摘要，详情里的 before/after 由 detail 接口单独返回。
pub async fn list_changes(
    database_url: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let rows: String = sqlx::query_scalar(LIST_CHANGES_SQL)
        .fetch_one(&pool)
        .await?;
    Ok(serde_json::from_str(&rows)?)
}

// Why：详情接口必须返回完整状态快照，前端才能展示 create/update/delete 的差异。
pub async fn get_change(
    database_url: &str,
    memory_uuid: &str,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let row = sqlx::query_scalar::<_, String>(CHANGE_DETAIL_SQL)
        .bind(memory_uuid)
        .fetch_optional(&pool)
        .await?;
    row.map(|value| Ok(serde_json::from_str(&value)?))
        .transpose()
}

// Why：批准 create 后记忆已成为正式工作态，可以在同一事务内生成默认图关系。
pub async fn approve_change(
    database_url: &str,
    memory_uuid: &str,
    embedding: Option<ApprovedEmbedding>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let mut tx = pool.begin().await?;
    let Some(change) = lock_change(&mut tx, memory_uuid).await? else {
        tx.rollback().await?;
        return Ok(false);
    };
    approve_locked_change(&mut tx, &change.0, &change.1).await?;
    if let Some(embedding) = embedding {
        upsert_approved_embedding(&mut tx, memory_uuid, embedding).await?;
    }
    tx.commit().await?;
    Ok(true)
}

// Why：拒绝必须在事务里恢复工作态和删除 change，避免出现半回滚状态。
pub async fn reject_change(
    database_url: &str,
    memory_uuid: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let mut tx = pool.begin().await?;
    let Some(change) = lock_change(&mut tx, memory_uuid).await? else {
        tx.rollback().await?;
        return Ok(false);
    };
    if change.0 == "create" {
        reject_create(&mut tx, &change.1).await?;
    } else {
        restore_before_state(&mut tx, &change.1, change.2.as_deref()).await?;
    }
    tx.commit().await?;
    Ok(true)
}

// Why：审批路径必须和 update 工具保持同一锁顺序，避免 memory_units / memory_changes 反向拿锁造成死锁。
async fn lock_change(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
) -> Result<Option<(String, String, Option<String>)>, sqlx::Error> {
    let locked_memory: Option<String> =
        sqlx::query_scalar("SELECT uuid::text FROM memory_units WHERE uuid = $1::uuid FOR UPDATE")
            .bind(memory_uuid)
            .fetch_optional(&mut **tx)
            .await?;
    if locked_memory.is_none() {
        return Ok(None);
    }
    sqlx::query_as(
        r#"
        SELECT action, memory_uuid::text, before_state::text
        FROM memory_changes
        WHERE memory_uuid = $1::uuid
        FOR UPDATE
        "#,
    )
    .bind(memory_uuid)
    .fetch_optional(&mut **tx)
    .await
}

// Why：approve 是用户最终确认入口，create 要激活，delete 要硬删，不能只删除 change。
async fn approve_locked_change(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    action: &str,
    memory_uuid: &str,
) -> Result<(), sqlx::Error> {
    if action == "create" {
        let activated = sqlx::query(
            "UPDATE memory_units SET status = 'active', updated_at = now() WHERE uuid = $1::uuid AND status = 'pending'",
        )
        .bind(memory_uuid)
        .execute(&mut **tx)
        .await?;
        if activated.rows_affected() != 1 {
            return Err(sqlx::Error::RowNotFound);
        }
    }
    if action == "delete" {
        sqlx::query("DELETE FROM memory_changes WHERE memory_uuid = $1::uuid")
            .bind(memory_uuid)
            .execute(&mut **tx)
            .await?;
        sqlx::query("DELETE FROM memory_units WHERE uuid = $1::uuid")
            .bind(memory_uuid)
            .execute(&mut **tx)
            .await?;
        super::mark_memory_graph_dirty(tx).await?;
        return Ok(());
    }
    super::search_index::refresh_memory_search_index(tx, memory_uuid).await?;
    sqlx::query("DELETE FROM memory_changes WHERE memory_uuid = $1::uuid")
        .bind(memory_uuid)
        .execute(&mut **tx)
        .await?;
    if action == "create" {
        super::relations::insert_auto_relations_for_approved_memory(tx, memory_uuid).await?;
        super::mark_memory_graph_dirty(tx).await?;
    }
    Ok(())
}

async fn upsert_approved_embedding(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    embedding: ApprovedEmbedding,
) -> Result<(), sqlx::Error> {
    // What：在 approve 同一事务内写入已生成的 memory embedding。
    // Why：embedding 写入失败时必须回滚 approve 状态，避免 HTTP 报错但记忆已变 active。
    let vector = format!(
        "[{}]",
        embedding
            .values
            .iter()
            .map(f32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    sqlx::query(
        r#"
        INSERT INTO memory_embeddings (memory_uuid, embedding, embedding_model, embedding_dimension, embedded_at)
        VALUES ($1::uuid, $2::vector, $3, $4, now())
        ON CONFLICT (memory_uuid)
        DO UPDATE SET embedding = EXCLUDED.embedding,
            embedding_model = EXCLUDED.embedding_model,
            embedding_dimension = EXCLUDED.embedding_dimension,
            embedded_at = EXCLUDED.embedded_at
        "#,
    )
    .bind(memory_uuid)
    .bind(vector)
    .bind(embedding.model)
    .bind(embedding.dimension)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// Why：create 的拒绝语义是移除新工作态，派生表交给外键级联清理。
async fn reject_create(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM memory_changes WHERE memory_uuid = $1::uuid")
        .bind(memory_uuid)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM memory_units WHERE uuid = $1::uuid")
        .bind(memory_uuid)
        .execute(&mut **tx)
        .await?;
    super::mark_memory_graph_dirty(tx).await?;
    Ok(())
}

// Why：非 create 拒绝要恢复完整工作态快照，不能只改 memory_units 主表。
async fn restore_before_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    before_state: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = before_state.ok_or_else(|| std::io::Error::other("before_state is required"))?;
    restore_memory_unit(tx, memory_uuid, state).await?;
    replace_keywords(tx, memory_uuid, state).await?;
    replace_relations(tx, memory_uuid, state).await?;
    super::search_index::refresh_memory_search_index(tx, memory_uuid).await?;
    super::mark_memory_graph_dirty(tx).await?;
    sqlx::query("DELETE FROM memory_changes WHERE memory_uuid = $1::uuid")
        .bind(memory_uuid)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// Why：memory_units 是当前工作态核心，拒绝 update/delete/restore 必须先恢复它。
async fn restore_memory_unit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    state: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE memory_units
        SET category = input.state #>> '{memory,category}',
            title_norm = input.state #>> '{memory,title_norm}',
            content = input.state #>> '{memory,content}',
            summary = input.state #>> '{memory,summary}',
            status = input.state #>> '{memory,status}',
            recall_when = input.state #>> '{memory,recall_when}',
            trashed_at = (input.state #>> '{memory,trashed_at}')::timestamptz,
            updated_at = now()
        FROM (SELECT $2::jsonb AS state) input
        WHERE uuid = $1::uuid
        "#,
    )
    .bind(memory_uuid)
    .bind(state)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// Why：keywords 是完整快照集合，拒绝时必须先清空再按 before_state 重建。
async fn replace_keywords(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    state: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM memory_keywords WHERE memory_uuid = $1::uuid")
        .bind(memory_uuid)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        r#"
        INSERT INTO memory_keywords (uuid, memory_uuid, keyword_norm, weight, created_at)
        SELECT gen_random_uuid(), $1::uuid, item ->> 'keyword_norm', (item ->> 'weight')::int, now()
        FROM jsonb_array_elements($2::jsonb -> 'keywords') AS items(item)
        "#,
    )
    .bind(memory_uuid)
    .bind(state)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// Why：relations 中目标 memory 相关边也是工作态，拒绝时要和 before_state 一起回滚。
async fn replace_relations(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    state: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM memory_relations WHERE from_memory_uuid = $1::uuid OR to_memory_uuid = $1::uuid",
    )
    .bind(memory_uuid)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO memory_relations (
            uuid, from_memory_uuid, to_memory_uuid, relation_type, weight, note, created_at
        )
        SELECT gen_random_uuid(), (item ->> 'from_memory_uuid')::uuid,
            (item ->> 'to_memory_uuid')::uuid, item ->> 'relation_type',
            (item ->> 'weight')::int, item ->> 'note', now()
        FROM jsonb_array_elements($1::jsonb -> 'relations') AS items(item)
        "#,
    )
    .bind(state)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
