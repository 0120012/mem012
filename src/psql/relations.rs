pub struct RelationCreate {
    pub from_memory_uuid: String,
    pub to_memory_uuid: String,
    pub relation_type: String,
    pub weight: Option<i32>,
    pub note: Option<String>,
}

pub struct RelationUpdate {
    pub relation_type: Option<String>,
    pub weight: Option<i32>,
    pub note: Option<String>,
}

// Why：关系变更属于当前工作态，必须同时留下可撤销的 memory_changes 快照。
pub async fn add_memory_relation(
    pool: &sqlx::Pool<sqlx::Postgres>,
    input: RelationCreate,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    validate_relation_type(&input.relation_type)?;
    validate_weight(input.weight)?;
    let mut tx = pool.begin().await?;
    ensure_active_endpoints(&mut tx, &input.from_memory_uuid, &input.to_memory_uuid).await?;
    let before_state = memory_state(&mut tx, &input.from_memory_uuid).await?;
    let relation = insert_relation(&mut tx, &input).await?;
    touch_relation_endpoints_revision(&mut tx, &input.from_memory_uuid, &input.to_memory_uuid)
        .await?;
    let after_state = memory_state(&mut tx, &input.from_memory_uuid).await?;
    upsert_relation_change(
        &mut tx,
        &input.from_memory_uuid,
        &before_state,
        &after_state,
    )
    .await?;
    super::mark_memory_graph_dirty(&mut tx).await?;
    tx.commit().await?;
    Ok(relation)
}

// Why：relation 的类型和权重会影响图排序，修改时必须走同一套回滚记录。
pub async fn update_memory_relation(
    pool: &sqlx::Pool<sqlx::Postgres>,
    relation_uuid: &str,
    patch: RelationUpdate,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(relation_type) = patch.relation_type.as_deref() {
        validate_relation_type(relation_type)?;
    }
    validate_relation_patch(&patch)?;
    validate_weight(patch.weight)?;
    let mut tx = pool.begin().await?;
    let Some((memory_uuid, other_memory_uuid)) = relation_owner(&mut tx, relation_uuid).await?
    else {
        tx.rollback().await?;
        return Ok(None);
    };
    let before_state = memory_state(&mut tx, &memory_uuid).await?;
    let relation = update_relation(&mut tx, relation_uuid, &patch).await?;
    touch_relation_endpoints_revision(&mut tx, &memory_uuid, &other_memory_uuid).await?;
    let after_state = memory_state(&mut tx, &memory_uuid).await?;
    upsert_relation_change(&mut tx, &memory_uuid, &before_state, &after_state).await?;
    super::mark_memory_graph_dirty(&mut tx).await?;
    tx.commit().await?;
    Ok(Some(relation))
}

// Why：删除 relation 会改变图可见性，必须先捕获 before_state 让用户可以 reject。
pub async fn delete_memory_relation(
    pool: &sqlx::Pool<sqlx::Postgres>,
    relation_uuid: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut tx = pool.begin().await?;
    let Some((memory_uuid, other_memory_uuid)) = relation_owner(&mut tx, relation_uuid).await?
    else {
        tx.rollback().await?;
        return Ok(false);
    };
    let before_state = memory_state(&mut tx, &memory_uuid).await?;
    sqlx::query("DELETE FROM memory_relations WHERE uuid = $1::uuid")
        .bind(relation_uuid)
        .execute(&mut *tx)
        .await?;
    touch_relation_endpoints_revision(&mut tx, &memory_uuid, &other_memory_uuid).await?;
    let after_state = memory_state(&mut tx, &memory_uuid).await?;
    upsert_relation_change(&mut tx, &memory_uuid, &before_state, &after_state).await?;
    super::mark_memory_graph_dirty(&mut tx).await?;
    tx.commit().await?;
    Ok(true)
}

// Why：SQL 一跳关系查询即使 AGE dirty 也可用，前端和 Agent 都能先看真实工作态。
pub async fn list_memory_neighbors(
    pool: &sqlx::Pool<sqlx::Postgres>,
    memory_uuid: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let row: String = sqlx::query_scalar(NEIGHBORS_SQL)
        .bind(memory_uuid)
        .fetch_one(pool)
        .await?;
    Ok(serde_json::from_str(&row)?)
}

// Why：图谱页默认态不应该要求用户先知道 UUID，先返回当前工作态的小型全图。
pub async fn list_memory_graph_overview(
    pool: &sqlx::Pool<sqlx::Postgres>,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let row: String = sqlx::query_scalar(GRAPH_OVERVIEW_SQL)
        .fetch_one(pool)
        .await?;
    Ok(serde_json::from_str(&row)?)
}

// What：分页列出历史边，并按当前类型和旧自动生成备注标记优先复核项。
// Why：备注可以编辑且可能沿用，是否清理需要结合当前类型及变更记录判断。
pub async fn list_relation_inventory(
    pool: &sqlx::PgPool,
    page: u32,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let row: String = sqlx::query_scalar(RELATION_INVENTORY_SQL)
        .bind(i64::from(page) * 50)
        .fetch_one(pool)
        .await?;
    Ok(serde_json::from_str(&row)?)
}

const RELATION_INVENTORY_SQL: &str = r#"
WITH inventory AS (
    SELECT r.uuid AS relation_uuid, r.relation_type, r.note,
           jsonb_build_object('memory_uuid', f.uuid, 'title_norm', f.title_norm) AS source,
           jsonb_build_object('memory_uuid', t.uuid, 'title_norm', t.title_norm) AS target,
           COALESCE(r.relation_type = 'related_to' AND r.note = 'auto: same_category', false) AS priority_review
    FROM memory_relations r
    JOIN memory_units f ON f.uuid = r.from_memory_uuid
    JOIN memory_units t ON t.uuid = r.to_memory_uuid
), page AS (
    SELECT * FROM inventory ORDER BY priority_review DESC, relation_uuid LIMIT 50 OFFSET $1
)
SELECT jsonb_build_object(
    'total', (SELECT count(*) FROM inventory),
    'priority_count', (SELECT count(*) FROM inventory WHERE priority_review),
    'relations', COALESCE((SELECT jsonb_agg(to_jsonb(page) ORDER BY priority_review DESC, relation_uuid) FROM page), '[]'::jsonb)
)::text
"#;

// Why：候选生成只读 SQL 主数据，避免 Agent 直接凭文本自行发明关系。
pub async fn suggest_memory_relations(
    pool: &sqlx::Pool<sqlx::Postgres>,
    memory_uuid: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let row: String = sqlx::query_scalar(SUGGEST_RELATIONS_SQL)
        .bind(memory_uuid)
        .fetch_one(pool)
        .await?;
    Ok(serde_json::from_str(&row)?)
}

// What：在新记忆审批事务中，根据预览目标或即时候选写入默认相关边。
// Why：已展示的候选限定了用户确认的范围，写入数量用于识别过期预览。
pub(crate) async fn insert_auto_relations_for_approved_memory(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    preview: Option<&[String]>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(AUTO_RELATIONS_SQL)
        .bind(memory_uuid)
        .bind(preview)
        .execute(&mut **tx)
        .await?;
    Ok(result.rows_affected())
}

// What：把明确选定的活跃旧记忆记录为新版替代边的目标。
// Why：锁住旧记忆可防止它在审批提交前被其他事务归档或删除。
pub(crate) async fn insert_approved_supersedes(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    previous_uuid: &str,
) -> Result<bool, sqlx::Error> {
    let inserted = sqlx::query(r#"
        WITH previous AS (
            SELECT uuid FROM memory_units
            WHERE uuid = $2::uuid AND uuid <> $1::uuid AND status = 'active'
            FOR SHARE
        )
        INSERT INTO memory_relations (uuid, from_memory_uuid, to_memory_uuid, relation_type, created_at)
        SELECT gen_random_uuid(), $1::uuid, uuid, 'supersedes', now() FROM previous
        ON CONFLICT (from_memory_uuid, to_memory_uuid, relation_type)
        DO UPDATE SET relation_type = EXCLUDED.relation_type
        "#)
        .bind(memory_uuid)
        .bind(previous_uuid)
        .execute(&mut **tx)
        .await?;
    Ok(inserted.rows_affected() == 1)
}

const MEMORY_STATE_SQL: &str = r#"
SELECT jsonb_build_object(
    'memory', jsonb_build_object(
        'uuid', u.uuid::text,
        'category', u.category,
        'title_norm', u.title_norm,
        'content', u.content,
        'summary', u.summary,
        'revision', u.revision,
        'status', u.status,
        'recall_when', u.recall_when,
        'trashed_at', u.trashed_at
    ),
    'keywords', COALESCE((
        SELECT jsonb_agg(jsonb_build_object('keyword_norm', keyword_norm, 'weight', weight) ORDER BY keyword_norm)
        FROM memory_keywords WHERE memory_uuid = u.uuid
    ), '[]'::jsonb),
    'relations', COALESCE((
        SELECT jsonb_agg(jsonb_build_object(
            'relation_uuid', uuid::text,
            'from_memory_uuid', from_memory_uuid::text,
            'to_memory_uuid', to_memory_uuid::text,
            'relation_type', relation_type,
            'weight', weight,
            'note', note
        ) ORDER BY created_at, uuid)
        FROM memory_relations
        WHERE from_memory_uuid = u.uuid OR to_memory_uuid = u.uuid
    ), '[]'::jsonb)
)::text
FROM memory_units u
WHERE u.uuid = $1::uuid
"#;

const NEIGHBORS_SQL: &str = r#"
SELECT jsonb_build_object(
    'memory_uuid', self.uuid::text,
    'memory', jsonb_build_object(
        'memory_uuid', self.uuid::text,
        'category', self.category,
        'title_norm', self.title_norm,
        'summary', self.summary,
        'status', self.status
    ),
    'neighbors', COALESCE(jsonb_agg(jsonb_build_object(
        'relation_uuid', r.uuid::text,
        'direction', CASE WHEN r.from_memory_uuid = $1::uuid THEN 'outgoing' ELSE 'incoming' END,
        'relation_type', r.relation_type,
        'weight', r.weight,
        'note', r.note,
        'memory', jsonb_build_object(
            'memory_uuid', other.uuid::text,
            'category', other.category,
            'title_norm', other.title_norm,
            'summary', other.summary,
            'status', other.status
        )
    ) ORDER BY r.created_at DESC, r.uuid) FILTER (WHERE other.uuid IS NOT NULL), '[]'::jsonb)
)::text
FROM memory_units self
LEFT JOIN memory_relations r ON r.from_memory_uuid = self.uuid OR r.to_memory_uuid = self.uuid
LEFT JOIN memory_units other ON other.uuid = CASE
    WHEN r.from_memory_uuid = $1::uuid THEN r.to_memory_uuid
    ELSE r.from_memory_uuid
END AND other.status = 'active'
WHERE self.uuid = $1::uuid AND self.status = 'active'
GROUP BY self.uuid, self.category, self.title_norm, self.summary, self.status
"#;

// What：先按关系权重为端点排序，再从有限的节点集合中取两端都可见的边。
// Why：限制总览规模的同时优先保留高权重关系，并避免返回悬空边。
const GRAPH_OVERVIEW_SQL: &str = r#"
WITH active_edges AS (
    SELECT r.from_memory_uuid, r.to_memory_uuid,
           row_number() OVER (ORDER BY r.weight DESC NULLS LAST, r.created_at DESC, r.uuid) AS priority
    FROM memory_relations r
    JOIN memory_units f ON f.uuid = r.from_memory_uuid AND f.status = 'active'
    JOIN memory_units t ON t.uuid = r.to_memory_uuid AND t.status = 'active'
), ranked_endpoints AS (
    SELECT endpoint, min(priority) AS priority
    FROM active_edges
    CROSS JOIN LATERAL (VALUES (from_memory_uuid), (to_memory_uuid)) AS endpoints(endpoint)
    GROUP BY endpoint
), nodes AS (
    SELECT m.*, e.priority
    FROM memory_units m LEFT JOIN ranked_endpoints e ON e.endpoint = m.uuid
    WHERE m.status = 'active'
    ORDER BY e.priority NULLS LAST, m.updated_at DESC, m.uuid
    LIMIT 100
),
relations AS (
    SELECT r.*
    FROM memory_relations r
    JOIN nodes f ON f.uuid = r.from_memory_uuid
    JOIN nodes t ON t.uuid = r.to_memory_uuid
)
SELECT jsonb_build_object(
    'nodes', COALESCE((
        SELECT jsonb_agg(jsonb_build_object(
            'memory_uuid', uuid::text,
            'category', category,
            'title_norm', title_norm,
            'summary', summary,
            'status', status
        ) ORDER BY priority NULLS LAST, updated_at DESC, uuid)
        FROM nodes
    ), '[]'::jsonb),
    'relations', COALESCE((
        SELECT jsonb_agg(jsonb_build_object(
            'relation_uuid', uuid::text,
            'from_memory_uuid', from_memory_uuid::text,
            'to_memory_uuid', to_memory_uuid::text,
            'relation_type', relation_type,
            'weight', weight,
            'note', note
        ) ORDER BY created_at DESC)
        FROM relations
    ), '[]'::jsonb)
)::text
"#;

// What：让只读预览和审批写入共用同一套候选筛选与排序。
// Why：两边规则一致，审批才能准确识别预览到提交之间的候选变化。
macro_rules! relation_candidate_query {
    ($statement:literal) => {
        concat!(r#"
WITH source_keywords AS (
    SELECT keyword_norm FROM memory_keywords WHERE memory_uuid = $1::uuid
),
candidates AS (
    SELECT m.uuid, m.category, m.title_norm, m.summary, count(*)::int AS shared_keywords,
        array_agg(k.keyword_norm ORDER BY k.keyword_norm) AS common_keywords
    FROM memory_units m
    JOIN memory_keywords k ON k.memory_uuid = m.uuid
    JOIN source_keywords sk ON sk.keyword_norm = k.keyword_norm
    WHERE m.uuid <> $1::uuid
        AND m.status = 'active'
        AND NOT EXISTS (
            SELECT 1 FROM memory_relations r
            WHERE r.relation_type = 'related_to'
              AND ((r.from_memory_uuid = $1::uuid AND r.to_memory_uuid = m.uuid)
                OR (r.from_memory_uuid = m.uuid AND r.to_memory_uuid = $1::uuid))
        )
    GROUP BY m.uuid, m.category, m.title_norm, m.summary
    ORDER BY shared_keywords DESC, m.title_norm, m.uuid
    LIMIT 3
)
"#, $statement)
    };
}

const SUGGEST_RELATIONS_SQL: &str = relation_candidate_query!(r#"
SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'from_memory_uuid', $1::uuid::text,
    'to_memory_uuid', uuid::text,
    'relation_type', 'related_to',
    'weight', LEAST(100, 50 + shared_keywords * 10),
    'note', format('shared_keywords=%s', shared_keywords),
    'candidate', jsonb_build_object(
        'memory_uuid', uuid::text,
        'category', category,
        'title_norm', title_norm,
        'summary', summary,
        'shared_keywords', shared_keywords,
        'common_keywords', common_keywords
    )
) ORDER BY shared_keywords DESC, title_norm, uuid), '[]'::jsonb)::text
FROM candidates
"#);

const AUTO_RELATIONS_SQL: &str = relation_candidate_query!(r#"
, locked_targets AS (
    SELECT target.uuid FROM memory_units target JOIN candidates USING (uuid)
    WHERE target.status = 'active' AND ($2::text[] IS NULL OR target.uuid::text = ANY($2::text[]))
    ORDER BY target.uuid
    FOR SHARE OF target
)
INSERT INTO memory_relations (
    uuid, from_memory_uuid, to_memory_uuid, relation_type, weight, note, created_at
)
SELECT
    gen_random_uuid(),
    $1::uuid,
    candidates.uuid,
    'related_to',
    LEAST(100, 50 + shared_keywords * 10),
    format('auto: shared_keywords=%s', shared_keywords),
    now()
FROM candidates JOIN locked_targets USING (uuid)
WHERE EXISTS (SELECT 1 FROM memory_units WHERE uuid = $1::uuid AND status = 'active')
ON CONFLICT (from_memory_uuid, to_memory_uuid, relation_type) DO NOTHING
"#);

pub(crate) async fn memory_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query_scalar::<_, String>(MEMORY_STATE_SQL)
        .bind(memory_uuid)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| std::io::Error::other("memory not found").into())
}

async fn ensure_active_endpoints(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    from_memory_uuid: &str,
    to_memory_uuid: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if from_memory_uuid == to_memory_uuid {
        return Err("relation endpoints must be different".into());
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memory_units WHERE uuid IN ($1::uuid, $2::uuid) AND status = 'active'",
    )
    .bind(from_memory_uuid)
    .bind(to_memory_uuid)
    .fetch_one(&mut **tx)
    .await?;
    if count != 2 {
        return Err("relation endpoints must be active memories".into());
    }
    Ok(())
}

async fn touch_relation_endpoints_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    from_memory_uuid: &str,
    to_memory_uuid: &str,
) -> Result<(), sqlx::Error> {
    // What：触碰 relation 两端 memory_units，让 revision trigger 推进工作态版本。
    // Why：relations 属于 memory_state，子表写入成功后两端记忆的快照都已经变化。
    sqlx::query("UPDATE memory_units SET updated_at = now() WHERE uuid IN ($1::uuid, $2::uuid)")
        .bind(from_memory_uuid)
        .bind(to_memory_uuid)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn insert_relation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &RelationCreate,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let row = sqlx::query_scalar::<_, String>(
        r#"
        INSERT INTO memory_relations (
            uuid, from_memory_uuid, to_memory_uuid, relation_type, weight, note, created_at
        )
        VALUES (gen_random_uuid(), $1::uuid, $2::uuid, $3, $4, NULLIF($5, ''), now())
        ON CONFLICT (from_memory_uuid, to_memory_uuid, relation_type) DO NOTHING
        RETURNING jsonb_build_object(
            'relation_uuid', uuid::text,
            'from_memory_uuid', from_memory_uuid::text,
            'to_memory_uuid', to_memory_uuid::text,
            'relation_type', relation_type,
            'weight', weight,
            'note', note
        )::text
        "#,
    )
    .bind(&input.from_memory_uuid)
    .bind(&input.to_memory_uuid)
    .bind(&input.relation_type)
    .bind(input.weight)
    .bind(input.note.as_deref())
    .fetch_optional(&mut **tx)
    .await?;
    let row = row.ok_or_else(|| std::io::Error::other("relation already exists"))?;
    Ok(serde_json::from_str(&row)?)
}

async fn update_relation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    relation_uuid: &str,
    patch: &RelationUpdate,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let row: String = sqlx::query_scalar(
        r#"
        UPDATE memory_relations
        SET relation_type = COALESCE($2, relation_type),
            weight = COALESCE($3, weight),
            note = CASE WHEN $4::text IS NULL THEN note ELSE NULLIF($4, '') END
        WHERE uuid = $1::uuid
        RETURNING jsonb_build_object(
            'relation_uuid', uuid::text,
            'from_memory_uuid', from_memory_uuid::text,
            'to_memory_uuid', to_memory_uuid::text,
            'relation_type', relation_type,
            'weight', weight,
            'note', note
        )::text
        "#,
    )
    .bind(relation_uuid)
    .bind(patch.relation_type.as_deref())
    .bind(patch.weight)
    .bind(patch.note.as_deref())
    .fetch_one(&mut **tx)
    .await?;
    Ok(serde_json::from_str(&row)?)
}

async fn relation_owner(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    relation_uuid: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT from_memory_uuid::text, to_memory_uuid::text
        FROM memory_relations
        WHERE uuid = $1::uuid
        FOR UPDATE
        "#,
    )
    .bind(relation_uuid)
    .fetch_optional(&mut **tx)
    .await
}

async fn upsert_relation_change(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    before_state: &str,
    after_state: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO memory_changes (
            uuid, memory_uuid, action, before_state, after_state, created_at, updated_at
        )
        VALUES ($1::uuid, $1::uuid, 'update', $2::jsonb, $3::jsonb, now(), now())
        ON CONFLICT (memory_uuid)
        DO UPDATE SET after_state = EXCLUDED.after_state, updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(memory_uuid)
    .bind(before_state)
    .bind(after_state)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn validate_relation_type(value: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let allowed = [
        "related_to",
        "supersedes",
        "depends_on",
        "conflicts_with",
        "elaborates",
        "applies_to",
    ];
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("invalid relation_type: {value}").into())
    }
}

fn validate_weight(value: Option<i32>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if value.is_some_and(|weight| !(0..=100).contains(&weight)) {
        return Err("weight must be between 0 and 100".into());
    }
    Ok(())
}

fn validate_relation_patch(
    patch: &RelationUpdate,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if patch.relation_type.is_none() && patch.weight.is_none() && patch.note.is_none() {
        return Err("relation patch cannot be empty".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RelationUpdate, validate_relation_patch, validate_relation_type, validate_weight};

    // Why：空 patch 会制造没有实际业务变化的 pending change，必须在进入事务前拒绝。
    #[test]
    fn rejects_empty_relation_patch() {
        let patch = RelationUpdate {
            relation_type: None,
            weight: None,
            note: None,
        };
        assert!(validate_relation_patch(&patch).is_err());
    }

    // Why：relation_type 是图边标签来源，非法值不能留到 AGE rebuild 时才失败。
    #[test]
    fn validates_relation_type_allowlist() {
        assert!(validate_relation_type("related_to").is_ok());
        assert!(validate_relation_type("unknown").is_err());
    }

    // Why：weight 参与排序，越界值必须在写入前被拒绝。
    #[test]
    fn validates_relation_weight_range() {
        assert!(validate_weight(Some(0)).is_ok());
        assert!(validate_weight(Some(100)).is_ok());
        assert!(validate_weight(Some(101)).is_err());
    }
}
