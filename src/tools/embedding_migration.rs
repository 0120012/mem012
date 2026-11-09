use std::collections::VecDeque;

use crate::provider::embedding::{generate_chunks, load_bge_m3_tokenizer, GeneratedChunk};
use sqlx::{Pool, Postgres};

type MigrationError = Box<dyn std::error::Error + Send + Sync>;
type Source = (String, i64, String, String);

pub(super) async fn run(
    pool: &Pool<Postgres>,
    settings: &crate::config::EmbeddingSettings,
    rpm: u32,
    tpm: u32,
) -> Result<(), MigrationError> {
    // What：回填复核和建索引后，在切换事务内换表、更新配置并验证新代查询。
    // Why：网络请求先于切换锁，验证失败与改名/配置共同回滚，不发布未经验证的新代。
    if rpm == 0 || tpm == 0 || settings.model != "BAAI/bge-m3" {
        return Err("RPM/TPM 必须为正数；当前迁移分词仅支持 BAAI/bge-m3".into());
    }
    let tokenizer = load_bge_m3_tokenizer(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models/bge-m3/tokenizer.json"))?;
    let mut guard = pool.begin().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended('mem012:embedding_migration', 0))")
        .fetch_one(&mut *guard).await?;
    if !locked { return Err("当前 profile 已有迁移任务运行".into()); }
    crate::psql::ensure_memory_embedding_config_table(pool).await?;
    crate::psql::create_memory_embeddings_next(pool, settings.dimension, &settings.model).await?;
    sqlx::query("LOCK TABLE memory_embeddings_next IN ACCESS SHARE MODE").execute(&mut *guard).await?;
    // What：从下一代表按 UUID 去重定位倒数第二条，只有一条时取该条。
    // Why：续跑包含回退起点，避免块数影响进度；补齐轮不受该起点限制。
    let resume_start: Option<String> = sqlx::query_scalar(r#"
        SELECT memory_uuid::text FROM (
            SELECT DISTINCT memory_uuid FROM memory_embeddings_next ORDER BY memory_uuid DESC LIMIT 2
        ) completed ORDER BY memory_uuid LIMIT 1
    "#).fetch_optional(pool).await?;
    let mut recent = VecDeque::new();
    let mut repair: Option<Vec<String>> = None;
    for attempt in 0..2 {
        let mut cursor: Option<String> = None;
        let mut failed = false;
        loop {
            let source: Option<Source> = sqlx::query_as(r#"
                SELECT u.uuid::text, u.revision, u.content,
                    concat_ws(E'\n', u.title_norm, u.summary, u.recall_when,
                        (SELECT string_agg(k.keyword_norm, ' ' ORDER BY k.keyword_norm)
                         FROM memory_keywords k WHERE k.memory_uuid = u.uuid))
                FROM memory_units u WHERE u.status = 'active' AND ($1::uuid IS NULL OR u.uuid > $1::uuid)
                  AND ($2::uuid IS NULL OR u.uuid >= $2::uuid)
                ORDER BY u.uuid LIMIT 1
            "#).bind(cursor.as_deref())
                .bind(if attempt == 0 { resume_start.as_deref() } else { None })
                .fetch_optional(pool).await?;
            let Some(source) = source else { break; };
            cursor = Some(source.0.clone());
            if repair.as_ref().is_some_and(|ids| !ids.contains(&source.0)) { continue; }
            let result = match generate_chunks(&source.2, &source.3, settings, &tokenizer, &mut recent, rpm, tpm).await {
                Ok(chunks) => store_chunks(pool, &source, settings, &chunks).await,
                Err(error) => Err(error),
            };
            match result {
                Ok(()) => println!("{}", serde_json::json!({"state":"success","tool":"prepare_embedding_next","memory_uuid":source.0})),
                Err(error) => {
                    failed = true;
                    println!("{}", serde_json::json!({"state":"error","tool":"prepare_embedding_next","memory_uuid":source.0,"error":error.to_string()}));
                }
            }
        }
        if failed { return Err("部分记忆回填失败，未建索引或切换；保留下一代结果，可重新执行恢复".into()); }
        let policy = "metadata-v1:heading-path-v1:full-input-8192:tokenizer-21106b6d7dab2952c1d496fb21d5dc9db75c28ed361a05f5020bbba27810dd08";
        let (stale, source_versions): (Vec<String>, Vec<String>) = sqlx::query_as(r#"
            WITH blocks AS (
                SELECT e.*, row_number() OVER w - 1 AS expected_index,
                    lag(content_end_byte, 1, 0::bigint) OVER w AS previous_end,
                    max(content_end_byte) OVER (PARTITION BY memory_uuid) AS final_end,
                    count(*) OVER (PARTITION BY memory_uuid) AS block_count
                FROM memory_embeddings_next e
                WINDOW w AS (PARTITION BY memory_uuid ORDER BY chunk_index)
            ), stale AS (
                SELECT DISTINCT u.uuid::text AS memory_uuid FROM memory_units u
                LEFT JOIN blocks e ON e.memory_uuid = u.uuid
                WHERE (u.status = 'active' AND (e.memory_uuid IS NULL OR e.source_revision <> u.revision
                    OR e.embedding_model <> $1 OR e.embedding_dimension <> $2 OR vector_dims(e.embedding) <> $2
                    OR e.embedding_policy IS DISTINCT FROM $3
                    OR e.chunk_index <> e.expected_index OR e.content_start_byte <> e.previous_end
                    OR e.final_end <> octet_length(u.content) OR e.content_end_byte > octet_length(u.content)
                    OR (e.content_end_byte = e.content_start_byte AND NOT (u.content = '' AND e.block_count = 1))
                    OR CASE WHEN e.content_end_byte < octet_length(u.content)
                        THEN (get_byte(convert_to(u.content, 'UTF8'), e.content_end_byte::int) & 192) = 128 ELSE false END))
                   OR (u.status <> 'active' AND e.memory_uuid IS NOT NULL)
            )
            SELECT ARRAY(SELECT memory_uuid FROM stale ORDER BY memory_uuid),
                   ARRAY(SELECT uuid::text || ':' || revision::text FROM memory_units
                         WHERE status = 'active' ORDER BY uuid)
        "#).bind(&settings.model).bind(i32::try_from(settings.dimension)?).bind(policy).fetch_one(&mut *guard).await?;
        if stale.is_empty() {
            sqlx::query("CREATE INDEX memory_embeddings_next_embedding_hnsw_idx ON memory_embeddings_next USING hnsw (embedding vector_cosine_ops)")
                .execute(&mut *guard).await?;
            let index_ready: bool = sqlx::query_scalar(r#"
                SELECT EXISTS (
                    SELECT 1 FROM pg_index i
                    JOIN pg_class t ON t.oid = i.indrelid
                    JOIN pg_class x ON x.oid = i.indexrelid AND x.relnamespace = t.relnamespace
                    JOIN pg_am am ON am.oid = x.relam
                    JOIN pg_attribute a ON a.attrelid = t.oid AND a.attname = 'embedding' AND NOT a.attisdropped
                    JOIN pg_opclass op ON op.oid = i.indclass[0] AND op.opcmethod = am.oid AND op.opcintype = a.atttypid
                    WHERE t.oid = 'memory_embeddings_next'::regclass
                      AND x.relname = 'memory_embeddings_next_embedding_hnsw_idx'
                      AND am.amname = 'hnsw' AND op.opcname = 'vector_cosine_ops'
                      AND i.indnkeyatts = 1 AND i.indnatts = 1 AND i.indkey[0] = a.attnum
                      AND i.indexprs IS NULL AND i.indpred IS NULL AND i.indisready AND i.indisvalid
                )
            "#).fetch_one(&mut *guard).await?;
            if !index_ready { return Err("下一代 HNSW 索引结构或可用状态不符，拒绝完成".into()); }
            let verification_input = "memory index migration verification";
            let verification_tokens = u32::try_from(tokenizer.encode(verification_input, true)?.len())?;
            let verification_vector = crate::provider::embedding::request_limited_embedding_with_retry(
                settings, verification_input, &mut recent, rpm, tpm, verification_tokens,
            ).await?;
            if verification_vector.len() != settings.dimension
                || verification_vector.iter().any(|value| !value.is_finite())
                || !verification_vector.iter().any(|value| *value != 0.0) {
                return Err("目标查询验证向量维度或数值无效，拒绝切换".into());
            }
            sqlx::query("LOCK TABLE memory_embeddings, memory_embeddings_next IN ACCESS EXCLUSIVE MODE NOWAIT")
                .execute(&mut *guard).await?;
            sqlx::query("LOCK TABLE memory_units, memory_keywords, memory_search_index, memory_embedding_targets IN SHARE MODE NOWAIT")
                .execute(&mut *guard).await?;
            sqlx::query("LOCK TABLE memory_embedding_config IN EXCLUSIVE MODE NOWAIT")
                .execute(&mut *guard).await?;
            let current_versions: Vec<String> = sqlx::query_scalar(
                "SELECT uuid::text || ':' || revision::text FROM memory_units WHERE status = 'active' ORDER BY uuid",
            ).fetch_all(&mut *guard).await?;
            if current_versions != source_versions {
                return Err("复核后 active 源集合或版本已变化，拒绝切换；保留下一代结果，可重新执行恢复".into());
            }
            swap_next_table(&mut guard, settings).await?;
            let (model, dimension, threshold, table_dimension): (String, i32, f64, i32) = sqlx::query_as(r#"
                SELECT cfg.embedding_model, cfg.embedding_dimension, cfg.fallback_max_distance, a.atttypmod
                FROM memory_embedding_config cfg JOIN pg_attribute a ON a.attrelid = 'memory_embeddings'::regclass
                  AND a.attname = 'embedding' AND a.atttypid = 'vector'::regtype AND NOT a.attisdropped
                WHERE cfg.singleton
            "#).fetch_one(&mut *guard).await?;
            if model != settings.model || dimension != settings.dimension as i32
                || table_dimension != dimension || threshold != settings.fallback_max_distance {
                return Err("换表后当前表与索引参数不一致，回滚切换".into());
            }
            let vector = format!("[{}]", verification_vector.iter().map(f32::to_string).collect::<Vec<_>>().join(","));
            let distance: Option<f64> = sqlx::query_scalar(r#"
                SELECT e.embedding <=> $1::vector FROM memory_embeddings e
                JOIN memory_units u ON u.uuid = e.memory_uuid AND u.status = 'active'
                WHERE e.embedding_model = $2 AND e.embedding_dimension = $3
                ORDER BY e.embedding <=> $1::vector LIMIT 1
            "#).bind(vector).bind(&settings.model).bind(dimension).fetch_optional(&mut *guard).await?;
            if distance.is_some_and(|value| !value.is_finite())
                || (distance.is_none() && !source_versions.is_empty()) {
                return Err("换表后余弦查询结果异常，回滚切换".into());
            }
            guard.commit().await?;
            return Ok(());
        }
        if attempt == 1 { return Err("补齐后全量复核仍不一致，拒绝完成；保留下一代结果，可重新执行恢复".into()); }
        // What：补齐前只清理已停用记忆的块，active 记忆交由存储事务替换。
        // Why：不能先删除 active 旧块，否则生成或写入失败后无法保留原结果。
        sqlx::query(r#"
            DELETE FROM memory_embeddings_next e USING memory_units u
            WHERE e.memory_uuid = u.uuid AND u.status <> 'active'
              AND e.memory_uuid = ANY($1::text[]::uuid[])
        "#).bind(&stale).execute(pool).await?;
        repair = Some(stale);
    }
    unreachable!("两轮扫描均会返回或进入下一轮")
}

async fn remove_previous_table(tx: &mut sqlx::Transaction<'_, Postgres>) -> Result<(), MigrationError> {
    // What：锁定并回收本程序标记的非活动上一代表，无槽位时直接返回。
    // Why：拒绝删除同名未知对象，且不使用 CASCADE；后续切换失败仍可随事务恢复。
    let Some(previous): Option<String> = sqlx::query_scalar(r#"
        SELECT quote_ident(n.nspname) || '.' || quote_ident(p.relname)
        FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        JOIN pg_class p ON p.relnamespace = n.oid AND p.relname = 'memory_embeddings_previous'
        WHERE c.oid = 'memory_embeddings'::regclass
    "#).fetch_optional(&mut **tx).await? else { return Ok(()); };
    sqlx::query(&format!("LOCK TABLE {previous} IN ACCESS EXCLUSIVE MODE NOWAIT")).execute(&mut **tx).await?;
    let recognized: bool = sqlx::query_scalar(r#"
        SELECT EXISTS (
            SELECT 1 FROM pg_class p JOIN pg_class c ON c.oid = 'memory_embeddings'::regclass
            JOIN pg_attribute a ON a.attrelid = p.oid
            WHERE p.oid = $1::regclass AND p.relkind = 'r'
              AND p.relowner = c.relowner AND p.relnamespace = c.relnamespace
              AND p.oid <> c.oid AND p.oid <> 'memory_embeddings_next'::regclass
              AND obj_description(p.oid, 'pg_class') = 'mem012:embedding_previous'
              AND a.attname = 'embedding' AND NOT a.attisdropped
              AND a.atttypid = 'vector'::regtype AND a.atttypmod BETWEEN 1 AND 2000
        )
    "#).bind(&previous).fetch_one(&mut **tx).await?;
    if !recognized { return Err("上一代槽位归属、标记或结构不符，拒绝回收".into()); }
    sqlx::query(&format!("DROP TABLE {previous}")).execute(&mut **tx).await?;
    Ok(())
}

async fn swap_next_table(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    settings: &crate::config::EmbeddingSettings,
) -> Result<(), MigrationError> {
    // What：在调用方已锁定的事务内协调上一代槽位、表名、索引名及当前配置。
    // Why：切换不可自行提交，查询验证失败须连同槽位回收和参数变更一起回滚。
    remove_previous_table(tx).await?;
    let schema: String = sqlx::query_scalar("SELECT quote_ident(n.nspname) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.oid = 'memory_embeddings'::regclass").fetch_one(&mut **tx).await?;
    let indexes: Vec<(String, String)> = sqlx::query_as(r#"
        SELECT quote_ident(n.nspname) || '.' || quote_ident(c.relname),
               quote_ident('memory_embeddings_previous_' || c.oid::text)
        FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE i.indrelid = 'memory_embeddings'::regclass
    "#).fetch_all(&mut **tx).await?;
    for (index, previous_name) in indexes {
        sqlx::query(&format!("ALTER INDEX {index} RENAME TO {previous_name}")).execute(&mut **tx).await?;
    }
    for query in [
        format!("ALTER TABLE {schema}.memory_embeddings RENAME TO memory_embeddings_previous"),
        format!("ALTER TABLE {schema}.memory_embeddings_next RENAME TO memory_embeddings"),
        format!("ALTER INDEX {schema}.memory_embeddings_next_pkey RENAME TO memory_embeddings_pkey"),
        format!("ALTER INDEX {schema}.memory_embeddings_next_embedding_hnsw_idx RENAME TO memory_embeddings_embedding_hnsw_idx"),
        format!("COMMENT ON TABLE {schema}.memory_embeddings_previous IS 'mem012:embedding_previous'"),
    ] { sqlx::query(&query).execute(&mut **tx).await?; }
    let changed = sqlx::query(&format!("UPDATE {schema}.memory_embedding_config SET embedding_model = $1, embedding_dimension = $2, fallback_max_distance = $3 WHERE singleton"))
        .bind(&settings.model).bind(i32::try_from(settings.dimension)?).bind(settings.fallback_max_distance).execute(&mut **tx).await?;
    if changed.rows_affected() != 1 { return Err("本库当前索引配置缺行，拒绝提交换表".into()); }
    Ok(())
}

async fn store_chunks(
    pool: &Pool<Postgres>,
    source: &Source,
    settings: &crate::config::EmbeddingSettings,
    chunks: &[GeneratedChunk],
) -> Result<(), MigrationError> {
    // What：核验块集合和源版本，在同一事务内替换该记忆全部块及生成策略。
    // Why：续跑可能改变块数，删除与写入必须共同回滚，避免重复主键、残留旧块或半条结果。
    let policy = "metadata-v1:heading-path-v1:full-input-8192:tokenizer-21106b6d7dab2952c1d496fb21d5dc9db75c28ed361a05f5020bbba27810dd08";
    let mut end = 0;
    if chunks.is_empty() || chunks.iter().any(|chunk| {
        let valid = chunk.start == end && chunk.end <= source.2.len()
            && (chunk.end > chunk.start || (source.2.is_empty() && chunks.len() == 1))
            && source.2.is_char_boundary(chunk.start) && source.2.is_char_boundary(chunk.end)
            && chunk.vector.len() == settings.dimension;
        end = chunk.end;
        !valid
    }) || end != source.2.len() {
        return Err("块覆盖或维度无效，拒绝写入".into());
    }
    let paths = crate::provider::embedding::markdown_heading_paths(&source.2);
    let mut headings = paths.iter().peekable();
    let mut title_path = "";
    let mut tx = pool.begin().await?;
    let revision: Option<i64> = sqlx::query_scalar("SELECT revision FROM memory_units WHERE uuid=$1::uuid AND status='active' FOR SHARE")
        .bind(&source.0).fetch_optional(&mut *tx).await?;
    if revision != Some(source.1) { return Err("源记忆已变化或不再 active，拒绝过期结果".into()); }
    sqlx::query("DELETE FROM memory_embeddings_next WHERE memory_uuid=$1::uuid")
        .bind(&source.0).execute(&mut *tx).await?;
    for (index, chunk) in chunks.iter().enumerate() {
        while headings.peek().is_some_and(|(offset, _)| *offset <= chunk.start) {
            title_path = &headings.next().unwrap().1;
        }
        let vector = format!("[{}]", chunk.vector.iter().map(f32::to_string).collect::<Vec<_>>().join(","));
        sqlx::query(r#"INSERT INTO memory_embeddings_next
            (memory_uuid,chunk_index,content_start_byte,content_end_byte,title_path,embedding,embedding_model,embedding_dimension,source_revision,embedding_policy,embedded_at)
            VALUES ($1::uuid,$2,$3,$4,$9,$5::vector,$6,$7,$8,$10,now())"#)
            .bind(&source.0).bind(i32::try_from(index)?).bind(i64::try_from(chunk.start)?)
            .bind(i64::try_from(chunk.end)?).bind(vector).bind(&settings.model)
            .bind(i32::try_from(settings.dimension)?).bind(source.1).bind(title_path).bind(policy).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn store_rejects_invalid_chunk_coverage_before_database_access() {
    let pool = sqlx::postgres::PgPoolOptions::new().connect_lazy("postgresql://unused:unused@127.0.0.1/unused").unwrap();
    pool.close().await;
    let mut settings = crate::config::EmbeddingSettings { api: String::new(), api_type: "embeddings".into(), key: String::new(), model: "BAAI/bge-m3".into(), dimension: 2, fallback_max_distance: 1.0, proxy: None };
    let source = (String::new(), 1, "中a".into(), String::new());
    for ranges in [vec![], vec![(0, 3)], vec![(1, 4)], vec![(0, 1), (1, 4)], vec![(0, 3), (2, 4)], vec![(0, 3), (4, 4)], vec![(0, 5)], vec![(0, 0), (0, 4)], vec![(3, 0), (0, 4)]] {
        let chunks: Vec<_> = ranges.into_iter().map(|(start, end)| GeneratedChunk { chunk_index: 0, title_path: String::new(), start, end, vector: vec![0.1, 0.2] }).collect();
        assert!(store_chunks(&pool, &source, &settings, &chunks).await.unwrap_err().to_string().contains("块覆盖或维度无效"));
    }
    let chunks = [GeneratedChunk { chunk_index: 0, title_path: String::new(), start: 0, end: 4, vector: vec![0.1, 0.2] }];
    settings.dimension = 3;
    assert!(store_chunks(&pool, &source, &settings, &chunks).await.unwrap_err().to_string().contains("块覆盖或维度无效"));
    settings.dimension = 2;
    assert_eq!(store_chunks(&pool, &source, &settings, &chunks).await.unwrap_err().to_string(), sqlx::Error::PoolClosed.to_string());
    let empty = (String::new(), 1, String::new(), String::new());
    let chunks = [GeneratedChunk { chunk_index: 0, title_path: String::new(), start: 0, end: 0, vector: vec![0.1, 0.2] }];
    assert_eq!(store_chunks(&pool, &empty, &settings, &chunks).await.unwrap_err().to_string(), sqlx::Error::PoolClosed.to_string());
}
