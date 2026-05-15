use serde::Deserialize;

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

pub async fn refresh_memory_embedding(
    database_url: &str,
    settings: crate::config::EmbeddingSettings,
    memory_uuid: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Why：embedding 是 active memory 的派生索引，失败不能影响用户批准主流程。
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    let input = fetch_embedding_input(&pool, memory_uuid).await?;
    let embedding = request_embedding(&settings, &input).await?;
    upsert_embedding(&pool, memory_uuid, &settings.model, &embedding).await?;
    Ok(())
}

async fn fetch_embedding_input(
    pool: &sqlx::Pool<sqlx::Postgres>,
    memory_uuid: &str,
) -> Result<String, sqlx::Error> {
    // Why：向量只由语义内容生成，handle 和 usage 不应影响 embedding 稳定性。
    sqlx::query_scalar(
        r#"
        SELECT concat_ws(E'\n',
            u.title_norm,
            u.summary,
            u.content,
            (SELECT string_agg(k.keyword_norm, ' ' ORDER BY k.keyword_norm)
             FROM memory_keywords k WHERE k.memory_uuid = u.uuid)
        )
        FROM memory_units u
        WHERE u.uuid = $1::uuid AND u.status = 'active'
        "#,
    )
    .bind(memory_uuid)
    .fetch_one(pool)
    .await
}

async fn request_embedding(
    settings: &crate::config::EmbeddingSettings,
    input: &str,
) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
    // Why：远程模型必须返回 1024 维，和 pgvector 表结构保持硬一致。
    let response: EmbeddingResponse = reqwest::Client::new()
        .post(embedding_endpoint(&settings.api))
        .bearer_auth(&settings.key)
        .json(&serde_json::json!({ "model": settings.model, "input": input }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let embedding = response
        .data
        .into_iter()
        .next()
        .ok_or("embedding 响应为空")?
        .embedding;
    if embedding.len() != 1024 {
        return Err(format!("embedding 维度错误: {}", embedding.len()).into());
    }
    Ok(embedding)
}

async fn upsert_embedding(
    pool: &sqlx::Pool<sqlx::Postgres>,
    memory_uuid: &str,
    model: &str,
    embedding: &[f32],
) -> Result<(), sqlx::Error> {
    // Why：embedding 可重建，重复生成时直接覆盖同一条派生索引。
    let vector = format!(
        "[{}]",
        embedding
            .iter()
            .map(f32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    sqlx::query(
        r#"
        INSERT INTO memory_embeddings (memory_uuid, embedding, embedding_model, embedding_dimension, embedded_at)
        VALUES ($1::uuid, $2::vector, $3, 1024, now())
        ON CONFLICT (memory_uuid)
        DO UPDATE SET embedding = EXCLUDED.embedding,
            embedding_model = EXCLUDED.embedding_model,
            embedding_dimension = EXCLUDED.embedding_dimension,
            embedded_at = EXCLUDED.embedded_at
        "#,
    )
    .bind(memory_uuid)
    .bind(vector)
    .bind(model)
    .execute(pool)
    .await?;
    Ok(())
}

fn embedding_endpoint(api: &str) -> String {
    // Why：配置可以写 base URL 或完整 endpoint，部署时不必因为路径形式改代码。
    let api = api.trim_end_matches('/');
    if api.ends_with("/embeddings") {
        api.to_string()
    } else {
        format!("{api}/embeddings")
    }
}
