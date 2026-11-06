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
WHERE c.action <> 'delete' OR u.status IS DISTINCT FROM 'trashed'
"#;

const COUNT_CHANGES_SQL: &str = r#"
SELECT COUNT(*)
FROM memory_changes c
LEFT JOIN memory_units u ON u.uuid = c.memory_uuid
WHERE c.action <> 'delete' OR u.status IS DISTINCT FROM 'trashed'
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
    AND (c.action <> 'delete' OR u.status IS DISTINCT FROM 'trashed')
"#;

const LIST_TRASH_SQL: &str = r#"
SELECT COALESCE(
    jsonb_agg(
        jsonb_build_object(
            'memory_uuid', c.memory_uuid::text,
            'action', c.action,
            'title_norm', COALESCE(u.title_norm, c.after_state #>> '{memory,title_norm}'),
            'summary', COALESCE(u.summary, c.after_state #>> '{memory,summary}'),
            'trashed_at', u.trashed_at::text,
            'expires_at', (u.trashed_at + ($1::bigint * interval '1 minute'))::text,
            'created_at', c.created_at::text,
            'updated_at', c.updated_at::text
        )
        ORDER BY u.trashed_at DESC
    ),
    '[]'::jsonb
)::text
FROM memory_changes c
JOIN memory_units u ON u.uuid = c.memory_uuid
WHERE c.action = 'delete' AND u.status = 'trashed' AND u.trashed_at IS NOT NULL
"#;

const TRASH_DETAIL_SQL: &str = r#"
SELECT jsonb_build_object(
    'memory_uuid', c.memory_uuid::text,
    'action', c.action,
    'title_norm', COALESCE(u.title_norm, c.after_state #>> '{memory,title_norm}'),
    'summary', COALESCE(u.summary, c.after_state #>> '{memory,summary}'),
    'before_state', c.before_state,
    'after_state', c.after_state,
    'trashed_at', u.trashed_at::text,
    'expires_at', (u.trashed_at + ($2::bigint * interval '1 minute'))::text,
    'created_at', c.created_at::text,
    'updated_at', c.updated_at::text
)::text
FROM memory_changes c
JOIN memory_units u ON u.uuid = c.memory_uuid
WHERE c.memory_uuid = $1::uuid
    AND c.action = 'delete'
    AND u.status = 'trashed'
    AND u.trashed_at IS NOT NULL
"#;

const LOCK_TRASH_MEMORY_SQL: &str = r#"
SELECT uuid::text
FROM memory_units
WHERE uuid = $1::uuid AND status = 'trashed'
FOR UPDATE
"#;

const RESTORE_TRASH_CHANGE_SQL: &str = r#"
SELECT before_state::text
FROM memory_changes
WHERE memory_uuid = $1::uuid AND action = 'delete'
FOR UPDATE
"#;

const RESTORE_PENDING_CREATE_SQL: &str = r#"
UPDATE memory_changes
SET action = 'create', before_state = NULL, after_state = $2::jsonb, updated_at = now()
WHERE memory_uuid = $1::uuid
"#;

const DELETE_EXPIRED_TRASH_SQL: &str = r#"
WITH expired AS (
    SELECT u.uuid
    FROM memory_units u
    JOIN memory_changes c ON c.memory_uuid = u.uuid
    WHERE u.status = 'trashed'
        AND c.action = 'delete'
        AND u.trashed_at IS NOT NULL
        AND u.trashed_at + ($1::bigint * interval '1 minute') <= now()
    FOR UPDATE OF u, c
),
deleted_changes AS (
    DELETE FROM memory_changes c
    USING expired e
    WHERE c.memory_uuid = e.uuid
    RETURNING c.memory_uuid
)
DELETE FROM memory_units u
USING deleted_changes d
WHERE u.uuid = d.memory_uuid
"#;

const LIST_EMBEDDING_BACKFILL_SQL: &str = r#"
SELECT u.uuid::text
FROM memory_units u
LEFT JOIN memory_embeddings e ON e.memory_uuid = u.uuid
WHERE u.status = 'active'
    AND (e.memory_uuid IS NULL OR e.embedding_model <> $1 OR e.embedding_dimension <> $2)
ORDER BY u.updated_at ASC, u.uuid ASC
LIMIT 50
"#;

pub struct ApprovedEmbedding {
    pub model: String,
    pub dimension: i32,
    pub(crate) values: crate::provider::embedding::EmbeddingValues,
    pub source_state: String,
}

pub(crate) async fn register_embedding_target(
    pool: &sqlx::Pool<sqlx::Postgres>,
    memory_uuid: &str,
    source_revision: i64,
    model: &str,
    dimension: i32,
    policy: &str,
) -> Result<i64, sqlx::Error> {
    // What：锁定 active 源版本并登记当前生成目标，返回本次生成代号。
    // Why：目标先于模型请求生效，新任务失败时也必须阻止旧策略任务回写。
    sqlx::query_scalar(r#"
        WITH source AS (
            SELECT uuid, revision FROM memory_units
            WHERE uuid = $1::uuid AND status = 'active' AND revision = $2 FOR UPDATE
        )
        INSERT INTO memory_embedding_targets
            (memory_uuid, source_revision, embedding_model, embedding_dimension, embedding_policy)
        SELECT uuid, revision, $3, $4, $5 FROM source
        ON CONFLICT (memory_uuid) DO UPDATE SET generation = DEFAULT,
            source_revision = EXCLUDED.source_revision, embedding_model = EXCLUDED.embedding_model,
            embedding_dimension = EXCLUDED.embedding_dimension, embedding_policy = EXCLUDED.embedding_policy
        RETURNING generation
    "#)
    .bind(memory_uuid).bind(source_revision).bind(model).bind(dimension).bind(policy)
    .fetch_optional(pool).await?
    .ok_or_else(|| sqlx::Error::protocol("生成目标源状态或版本已变化，拒绝登记"))
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

// What：只返回当前项目可审查变更的数量。
// Why：侧栏徽标不需要完整摘要，避免应用启动时构造整个变更列表。
pub async fn count_changes(
    database_url: &str,
) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    Ok(sqlx::query_scalar(COUNT_CHANGES_SQL)
        .fetch_one(&pool)
        .await?)
}

// What：列出当前项目回收站里的待删除记忆，并计算自动硬删时间。
// Why：expires_at 是配置派生值，不应落库，否则配置变更后页面会显示过期数据。
pub async fn list_trash(
    database_url: &str,
    retention_minutes: i64,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let rows: String = sqlx::query_scalar(LIST_TRASH_SQL)
        .bind(retention_minutes)
        .fetch_one(&pool)
        .await?;
    Ok(serde_json::from_str(&rows)?)
}

// Why：详情接口必须在 SQL 层限定 delete + trashed，避免 HTTP 层误把普通 change 当回收站项。
pub async fn get_trash(
    database_url: &str,
    memory_uuid: &str,
    retention_minutes: i64,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let row = sqlx::query_scalar::<_, String>(TRASH_DETAIL_SQL)
        .bind(memory_uuid)
        .bind(retention_minutes)
        .fetch_optional(&pool)
        .await?;
    row.map(|value| Ok(serde_json::from_str(&value)?))
        .transpose()
}

// What：永久删除一个已进入回收站且等待 delete 审批的记忆。
// Why：硬删入口必须自带状态约束，避免 API 或定时任务误删普通待审批项。
pub async fn delete_trash(
    database_url: &str,
    memory_uuid: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let mut tx = pool.begin().await?;
    let Some(locked_uuid) = sqlx::query_scalar::<_, String>(LOCK_TRASH_MEMORY_SQL)
        .bind(memory_uuid)
        .fetch_optional(&mut *tx)
        .await?
    else {
        tx.rollback().await?;
        return Ok(false);
    };
    let delete_change = sqlx::query_scalar::<_, String>(
        r#"
        SELECT memory_uuid::text
        FROM memory_changes
        WHERE memory_uuid = $1::uuid AND action = 'delete'
        FOR UPDATE
        "#,
    )
    .bind(&locked_uuid)
    .fetch_optional(&mut *tx)
    .await?;
    if delete_change.is_none() {
        return Err(std::io::Error::other("TRASH_STATE_INVALID: delete change is missing").into());
    }
    super::mark_memory_graph_dirty(&mut tx).await?;
    sqlx::query("DELETE FROM memory_changes WHERE memory_uuid = $1::uuid")
        .bind(&locked_uuid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM memory_units WHERE uuid = $1::uuid")
        .bind(&locked_uuid)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

// What：从回收站恢复一条等待 delete 审批的记忆。
// Why：pending create 的删除恢复后仍必须保留 create 审批，不能复用 reject_change 删除 change。
pub async fn restore_trash(
    database_url: &str,
    memory_uuid: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let mut tx = pool.begin().await?;
    let Some(locked_uuid) = sqlx::query_scalar::<_, String>(LOCK_TRASH_MEMORY_SQL)
        .bind(memory_uuid)
        .fetch_optional(&mut *tx)
        .await?
    else {
        tx.rollback().await?;
        return Ok(false);
    };
    let before_state = sqlx::query_scalar::<_, String>(RESTORE_TRASH_CHANGE_SQL)
        .bind(&locked_uuid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            std::io::Error::other("DELETE_CHANGE_NOT_FOUND: delete change is missing")
        })?;
    let state = serde_json::from_str::<serde_json::Value>(&before_state)?;
    match state
        .pointer("/memory/status")
        .and_then(serde_json::Value::as_str)
    {
        Some("active") => restore_before_state(&mut tx, &locked_uuid, Some(&before_state)).await?,
        Some("pending") => {
            restore_memory_unit(&mut tx, &locked_uuid, &before_state).await?;
            replace_keywords(&mut tx, &locked_uuid, &before_state).await?;
            replace_relations(&mut tx, &locked_uuid, &before_state).await?;
            super::search_index::refresh_memory_search_index(&mut tx, &locked_uuid).await?;
            sqlx::query(RESTORE_PENDING_CREATE_SQL)
                .bind(&locked_uuid)
                .bind(&before_state)
                .execute(&mut *tx)
                .await?;
        }
        _ => return Err(std::io::Error::other("unsupported trash restore state").into()),
    }
    tx.commit().await?;
    Ok(true)
}

// What：永久删除单个数据库里已超过保留期的回收站记忆。
// Why：后台 worker 会逐 profile 调用这里；SQL 自带 trash 条件，避免跨入口误删普通变更。
pub async fn delete_expired_trash(
    database_url: &str,
    retention_minutes: i64,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let mut tx = pool.begin().await?;
    let deleted = sqlx::query(DELETE_EXPIRED_TRASH_SQL)
        .bind(retention_minutes)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if deleted > 0 {
        super::mark_memory_graph_dirty(&mut tx).await?;
    }
    tx.commit().await?;
    Ok(deleted)
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
    if let Some(embedding) = embedding.as_ref() {
        validate_embedding_source(&mut tx, memory_uuid, &embedding.source_state).await?;
    }
    approve_locked_change(&mut tx, &change.0, &change.1).await?;
    if let Some(embedding) = embedding {
        upsert_approved_embedding(&mut tx, memory_uuid, embedding).await?;
    }
    tx.commit().await?;
    Ok(true)
}

// What：在审批事务之外单独为一条记忆刷新 embedding。
// Why：approve 不再同步等待向量生成，后台写入必须自带行锁与 source_state 校验，
// 才能避免把生成期间已被更新的旧向量覆盖到新工作态上；记忆已不存在时静默跳过。
pub async fn refresh_memory_embedding(
    database_url: &str,
    memory_uuid: &str,
    embedding: ApprovedEmbedding,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let mut tx = pool.begin().await?;
    // What：先取得当前向量表写锁，再锁定 active 源记忆并核验发布条件。
    // Why：与换表按同一顺序取锁，避免持有源行锁后等待换表；提前取得写锁避免发布时升级锁。
    sqlx::query("LOCK TABLE memory_embeddings IN ROW EXCLUSIVE MODE").execute(&mut *tx).await?;
    let locked: Option<String> =
        sqlx::query_scalar("SELECT uuid::text FROM memory_units WHERE uuid = $1::uuid AND status = 'active' FOR UPDATE")
            .bind(memory_uuid)
            .fetch_optional(&mut *tx)
            .await?;
    if locked.is_none() {
        tx.rollback().await?;
        return Ok(());
    }
    validate_embedding_source(&mut tx, memory_uuid, &embedding.source_state).await?;
    upsert_approved_embedding(&mut tx, memory_uuid, embedding).await?;
    tx.commit().await?;
    Ok(())
}

// What：按本库当前索引参数扫描缺失或模型/维度不匹配的 active 记忆。
// Why：各库可独立换代，扫描持锁核对表身份，避免旧全局参数或在途换表制造错误待办。
pub async fn list_embedding_backfill_memory_uuids(
    database_url: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let (table_oid, model, dimension, _) = super::read_current_embedding_config(&pool).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("LOCK TABLE memory_embeddings IN ACCESS SHARE MODE").execute(&mut *tx).await?;
    let current_oid: i64 = sqlx::query_scalar("SELECT 'memory_embeddings'::regclass::oid::bigint")
        .fetch_one(&mut *tx).await?;
    if current_oid != table_oid { return Err("待办扫描前索引已切换，下轮重新扫描".into()); }
    let uuids = sqlx::query_scalar::<_, String>(LIST_EMBEDDING_BACKFILL_SQL)
        .bind(model)
        .bind(dimension)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(uuids)
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
        FROM memory_changes c
        JOIN memory_units u ON u.uuid = c.memory_uuid
        WHERE c.memory_uuid = $1::uuid
            AND (c.action <> 'delete' OR u.status IS DISTINCT FROM 'trashed')
        FOR UPDATE OF c
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

async fn validate_embedding_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    source_state: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // What：单独核验携带的表身份、源快照与生成目标，持锁至发布事务结束。
    // Why：表身份和目标标记不属于源内容，换代后晚完成的任务不能发布到新表。
    let mut source = serde_json::from_str::<serde_json::Value>(source_state)?;
    if let Some(oid) = source.as_object_mut().and_then(|state| state.remove("embedding_table_oid")) {
        let expected_oid = oid.as_i64().filter(|oid| *oid > 0)
            .ok_or("APPROVE_CONFLICT: invalid embedding table identity")?;
        sqlx::query("LOCK TABLE memory_embeddings IN ACCESS SHARE MODE").execute(&mut **tx).await?;
        let current_oid: i64 = sqlx::query_scalar("SELECT 'memory_embeddings'::regclass::oid::bigint")
            .fetch_one(&mut **tx).await?;
        if current_oid != expected_oid {
            return Err("APPROVE_CONFLICT: embedding table changed during generation".into());
        }
    }
    let current_state = super::memory_state(tx, memory_uuid).await?;
    let current = serde_json::from_str::<serde_json::Value>(&current_state)?;
    let target = source.as_object_mut().and_then(|state| state.remove("embedding_target"));
    if current != source {
        return Err(std::io::Error::other(
            "APPROVE_CONFLICT: memory changed during embedding generation",
        )
        .into());
    }
    if let Some(target) = target {
        let current_target: Option<String> = sqlx::query_scalar(r#"
            SELECT jsonb_build_object('generation', generation, 'model', embedding_model,
                'dimension', embedding_dimension, 'policy', embedding_policy)::text
            FROM memory_embedding_targets
            WHERE memory_uuid = $1::uuid AND source_revision = $2 FOR SHARE
        "#).bind(memory_uuid).bind(source.pointer("/memory/revision").and_then(serde_json::Value::as_i64))
            .fetch_optional(&mut **tx).await?;
        let current_target = current_target.as_deref()
            .map(serde_json::from_str::<serde_json::Value>).transpose()?;
        if current_target.as_ref() != Some(&target) {
            return Err(std::io::Error::other(
                "APPROVE_CONFLICT: embedding target changed during generation",
            ).into());
        }
    }
    Ok(())
}

async fn upsert_approved_embedding(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    embedding: ApprovedEmbedding,
) -> Result<(), sqlx::Error> {
    // What：核验源与目标后，在原事务替换整套块并保存发布时的策略及代号。
    // Why：块上的目标快照不会随新请求变化，失败时内容与策略标记共同回滚。
    let values = match embedding.values {
        crate::provider::embedding::EmbeddingValues::Legacy(values) => values,
        crate::provider::embedding::EmbeddingValues::Chunks(chunks) => {
            let source: serde_json::Value = serde_json::from_str(&embedding.source_state)
                .map_err(|_| sqlx::Error::protocol("块索引源快照不是有效 JSON"))?;
            let content = source.pointer("/memory/content").and_then(serde_json::Value::as_str)
                .ok_or_else(|| sqlx::Error::protocol("块索引源快照缺少正文"))?;
            let mut end = 0;
            if !(1..=2000).contains(&embedding.dimension) || chunks.is_empty()
                || chunks.iter().enumerate().any(|(index, chunk)| {
                    let valid = chunk.chunk_index == index && chunk.start == end
                        && chunk.end <= content.len()
                        && (chunk.end > chunk.start || (content.is_empty() && chunks.len() == 1))
                        && content.is_char_boundary(chunk.start) && content.is_char_boundary(chunk.end)
                        && chunk.vector.len() == embedding.dimension as usize
                        && chunk.vector.iter().all(|value| value.is_finite())
                        && chunk.vector.iter().any(|value| *value != 0.0);
                    end = chunk.end;
                    !valid
                }) || end != content.len()
            {
                return Err(sqlx::Error::protocol("块序号、正文覆盖或向量无效，拒绝写入"));
            }
            let source_revision = source.pointer("/memory/revision")
                .and_then(serde_json::Value::as_i64).filter(|revision| *revision > 0)
                .ok_or_else(|| sqlx::Error::protocol("块索引源快照缺少有效版本"))?;
            let current_revision: Option<i64> = sqlx::query_scalar(
                "SELECT revision FROM memory_units WHERE uuid = $1::uuid AND status = 'active' FOR UPDATE",
            ).bind(memory_uuid).fetch_optional(&mut **tx).await?;
            if current_revision != Some(source_revision) {
                return Err(sqlx::Error::protocol("块索引源状态或版本已变化，拒绝写入"));
            }
            let target = source.get("embedding_target")
                .ok_or_else(|| sqlx::Error::protocol("块索引缺少生成目标，拒绝写入"))?;
            if target.get("model").and_then(serde_json::Value::as_str) != Some(embedding.model.as_str())
                || target.get("dimension").and_then(serde_json::Value::as_i64) != Some(i64::from(embedding.dimension))
            {
                return Err(sqlx::Error::protocol("块索引载荷与生成目标模型或维度不符"));
            }
            validate_embedding_source(tx, memory_uuid, &embedding.source_state).await
                .map_err(|error| sqlx::Error::protocol(error.to_string()))?;
            sqlx::query("DELETE FROM memory_embeddings WHERE memory_uuid = $1::uuid")
                .bind(memory_uuid).execute(&mut **tx).await?;
            for chunk in chunks {
                let vector = format!("[{}]", chunk.vector.iter().map(f32::to_string).collect::<Vec<_>>().join(","));
                sqlx::query(r#"INSERT INTO memory_embeddings
                    (memory_uuid,chunk_index,content_start_byte,content_end_byte,title_path,embedding,embedding_model,embedding_dimension,source_revision,embedding_policy,target_generation,embedded_at)
                    VALUES ($1::uuid,$2,$3,$4,$5,$6::vector,$7,$8,$9,$10,$11,now())"#)
                    .bind(memory_uuid).bind(i32::try_from(chunk.chunk_index).map_err(|error| sqlx::Error::protocol(error.to_string()))?)
                    .bind(i64::try_from(chunk.start).map_err(|error| sqlx::Error::protocol(error.to_string()))?)
                    .bind(i64::try_from(chunk.end).map_err(|error| sqlx::Error::protocol(error.to_string()))?)
                    .bind(chunk.title_path).bind(vector).bind(&embedding.model).bind(embedding.dimension)
                    .bind(source_revision).bind(target.get("policy").and_then(serde_json::Value::as_str))
                    .bind(target.get("generation").and_then(serde_json::Value::as_i64))
                    .execute(&mut **tx).await?;
            }
            return Ok(());
        }
    };
    let vector = format!(
        "[{}]",
        values
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

#[cfg(test)]
mod tests {
    use super::{
        CHANGE_DETAIL_SQL, DELETE_EXPIRED_TRASH_SQL, LIST_CHANGES_SQL, LOCK_TRASH_MEMORY_SQL,
        RESTORE_TRASH_CHANGE_SQL,
    };

    #[test]
    fn review_change_sql_excludes_trashed_delete_items() {
        let filter = "c.action <> 'delete' OR u.status IS DISTINCT FROM 'trashed'";

        assert!(LIST_CHANGES_SQL.contains(filter));
        assert!(CHANGE_DETAIL_SQL.contains(filter));
    }

    #[test]
    fn restore_trash_sql_locks_memory_before_delete_change_lookup() {
        assert!(LOCK_TRASH_MEMORY_SQL.contains("status = 'trashed'"));
        assert!(!LOCK_TRASH_MEMORY_SQL.contains("memory_changes"));
        assert!(RESTORE_TRASH_CHANGE_SQL.contains("action = 'delete'"));
    }

    #[test]
    fn delete_expired_trash_sql_keeps_expiry_filter_and_delete_order() {
        let sql = DELETE_EXPIRED_TRASH_SQL;
        assert!(sql.contains("WHERE u.status = 'trashed'"));
        assert!(sql.contains("AND c.action = 'delete'"));
        assert!(sql.contains("AND u.trashed_at IS NOT NULL"));
        assert!(sql.contains("AND u.trashed_at + ($1::bigint * interval '1 minute') <= now()"));

        let change_delete = sql.find("DELETE FROM memory_changes").unwrap();
        let unit_delete = sql.find("DELETE FROM memory_units").unwrap();
        assert!(change_delete < unit_delete);
    }
}
