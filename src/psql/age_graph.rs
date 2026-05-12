const REBUILD_MEMORY_GRAPH_SQL: &str = r#"
DO $$
DECLARE
    memory_row record;
    relation_row record;
BEGIN
    IF EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph') THEN
        PERFORM ag_catalog.drop_graph('memory_graph', true);
    END IF;
    PERFORM ag_catalog.create_graph('memory_graph');

    FOR memory_row IN SELECT uuid::text uuid, category, title_norm, status, summary FROM memory_units WHERE status = 'active'
    LOOP
        EXECUTE format($sql$SELECT * FROM ag_catalog.cypher('memory_graph', $cypher$
            CREATE (:Memory {uuid: %L, category: %L, title_norm: %L, status: %L, summary: %L})
        $cypher$) AS (v agtype)$sql$, memory_row.uuid, memory_row.category, memory_row.title_norm, memory_row.status, memory_row.summary);
    END LOOP;

    FOR relation_row IN
        SELECT r.uuid::text relation_uuid, r.from_memory_uuid::text from_uuid, r.to_memory_uuid::text to_uuid,
            upper(r.relation_type) edge_label, r.weight, r.note, r.created_at::text created_at
        FROM memory_relations r
        JOIN memory_units f ON f.uuid = r.from_memory_uuid AND f.status = 'active'
        JOIN memory_units t ON t.uuid = r.to_memory_uuid AND t.status = 'active'
    LOOP
        EXECUTE format($sql$SELECT * FROM ag_catalog.cypher('memory_graph', $cypher$
            MATCH (a:Memory {uuid: %L}), (b:Memory {uuid: %L})
            CREATE (a)-[:%s {relation_uuid: %L, weight: %s, note: %s, created_at: %L}]->(b)
        $cypher$) AS (e agtype)$sql$, relation_row.from_uuid, relation_row.to_uuid, relation_row.edge_label,
            relation_row.relation_uuid, COALESCE(relation_row.weight::text, 'null'), COALESCE(to_json(relation_row.note)::text, 'null'), relation_row.created_at);
    END LOOP;

    INSERT INTO memory_graph_meta (graph_name, dirty, updated_at) VALUES ('memory_graph', false, now())
    ON CONFLICT (graph_name) DO UPDATE SET dirty = false, updated_at = EXCLUDED.updated_at;
END $$;
"#;

// Why：AGE 是派生层，重建必须只从 SQL 当前工作态生成，避免 pending change 影响图结构。
pub async fn rebuild_memory_graph(pool: &sqlx::Pool<sqlx::Postgres>) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    for sql in [
        "CREATE EXTENSION IF NOT EXISTS age",
        "LOAD 'age'",
        r#"SET LOCAL search_path = ag_catalog, "$user", public"#,
    ] {
        sqlx::query(sql).execute(&mut *tx).await?;
    }
    sqlx::query(REBUILD_MEMORY_GRAPH_SQL)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
