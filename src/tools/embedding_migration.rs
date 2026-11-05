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
    // What：串行回填本轮 active 记忆，进程内共享额度队列并逐条输出结果。
    // Why：整轮互斥防止另一准备命令清空目标表；单条失败继续处理，但整轮返回失败。
    if rpm == 0 || tpm == 0 || settings.model != "BAAI/bge-m3" {
        return Err("RPM/TPM 必须为正数；当前迁移分词仅支持 BAAI/bge-m3".into());
    }
    let tokenizer = load_bge_m3_tokenizer(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models/bge-m3/tokenizer.json"))?;
    let mut guard = pool.begin().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended('mem012:embedding_migration', 0))")
        .fetch_one(&mut *guard).await?;
    if !locked { return Err("当前 profile 已有迁移任务运行".into()); }
    crate::psql::create_memory_embeddings_next(pool, settings.dimension).await?;
    sqlx::query("LOCK TABLE memory_embeddings_next IN ACCESS SHARE MODE").execute(&mut *guard).await?;
    let mut recent = VecDeque::new();
    let mut cursor: Option<String> = None;
    let mut failed = false;
    loop {
        let source: Option<Source> = sqlx::query_as(r#"
            SELECT u.uuid::text, u.revision, u.content,
                concat_ws(E'\n', u.title_norm, u.summary, u.recall_when,
                    (SELECT string_agg(k.keyword_norm, ' ' ORDER BY k.keyword_norm)
                     FROM memory_keywords k WHERE k.memory_uuid = u.uuid))
            FROM memory_units u WHERE u.status = 'active' AND ($1::uuid IS NULL OR u.uuid > $1::uuid)
            ORDER BY u.uuid LIMIT 1
        "#).bind(cursor.as_deref()).fetch_optional(pool).await?;
        let Some(source) = source else { break; };
        cursor = Some(source.0.clone());
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
    guard.commit().await?;
    if failed { return Err("部分记忆回填失败，未建索引或切换；重新执行将从头生成".into()); }
    Ok(())
}

async fn store_chunks(
    pool: &Pool<Postgres>,
    source: &Source,
    settings: &crate::config::EmbeddingSettings,
    chunks: &[GeneratedChunk],
) -> Result<(), MigrationError> {
    // What：核验完整块集合和源版本，按块起点补齐标题路径并事务写入。
    // Why：复用生成时的源快照和解析规则保持路径一致；源变化或任一块失败均拒绝发布。
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
    for (index, chunk) in chunks.iter().enumerate() {
        while headings.peek().is_some_and(|(offset, _)| *offset <= chunk.start) {
            title_path = &headings.next().unwrap().1;
        }
        let vector = format!("[{}]", chunk.vector.iter().map(f32::to_string).collect::<Vec<_>>().join(","));
        sqlx::query(r#"INSERT INTO memory_embeddings_next
            (memory_uuid,chunk_index,content_start_byte,content_end_byte,title_path,embedding,embedding_model,embedding_dimension,source_revision,embedded_at)
            VALUES ($1::uuid,$2,$3,$4,$9,$5::vector,$6,$7,$8,now())"#)
            .bind(&source.0).bind(i32::try_from(index)?).bind(i64::try_from(chunk.start)?)
            .bind(i64::try_from(chunk.end)?).bind(vector).bind(&settings.model)
            .bind(i32::try_from(settings.dimension)?).bind(source.1).bind(title_path).execute(&mut *tx).await?;
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
