use sqlx::{Acquire, Pool, Postgres};

fn reset_db_owner_error(profile: &str, database: &str, owner: &str) -> Option<String> {
    // What：生成 reset_db 作用域冲突错误。
    // Why：只有数据库 owner 才能安全清理自己的 profile 数据，成员角色不能触发共享库清理。
    (owner != profile).then(|| {
        format!(
            "reset_db 仅允许清理 profile 自有数据库：database `{database}` owner 是 `{owner}`，profile 是 `{profile}`"
        )
    })
}

fn memory_db_label(profile: &str) -> String {
    if profile == "share" {
        "share".to_string()
    } else {
        format!("profile {profile}")
    }
}

fn memory_embeddings_table_sql(embeddings_dimension: usize) -> String {
    // What：正常索引表沿用块表契约，以记忆和块序号共同定位独立向量。
    // Why：正常刷新与迁移需要相同的块字段和约束，复用定义避免两条链路结构漂移。
    memory_embeddings_next_table_sql(embeddings_dimension)
        .replacen("CREATE TABLE memory_embeddings_next", "CREATE TABLE memory_embeddings", 1)
}

fn memory_embeddings_next_table_sql(embeddings_dimension: usize) -> String {
    // What：定义块向量及策略，允许没有正常刷新代号的迁移块记录已知策略。
    // Why：策略是生成事实，代号用于正常任务发布保护；迁移不应为记录策略伪造代号。
    assert!(embeddings_dimension > 0, "embedding 维度必须为正数");
    format!(
        r#"
        CREATE TABLE memory_embeddings_next (
            memory_uuid UUID NOT NULL REFERENCES memory_units(uuid) ON DELETE CASCADE,
            chunk_index INT NOT NULL CHECK (chunk_index >= 0),
            content_start_byte BIGINT NOT NULL CHECK (content_start_byte >= 0),
            content_end_byte BIGINT NOT NULL CHECK (content_end_byte >= content_start_byte),
            title_path TEXT NOT NULL,
            embedding vector({embeddings_dimension}) NOT NULL,
            embedding_model TEXT NOT NULL CHECK (embedding_model <> ''),
            embedding_dimension INT NOT NULL CHECK (embedding_dimension = {embeddings_dimension}),
            source_revision BIGINT NOT NULL CHECK (source_revision > 0),
            embedding_policy TEXT CHECK (embedding_policy <> ''),
            target_generation BIGINT CHECK (target_generation > 0),
            embedded_at TIMESTAMPTZ NOT NULL,
            CHECK (target_generation IS NULL OR embedding_policy IS NOT NULL),
            PRIMARY KEY (memory_uuid, chunk_index)
        );
        "#
    )
}

pub(crate) async fn read_current_embedding_config(
    pool: &Pool<Postgres>,
) -> Result<(i64, String, i32, f64), sqlx::Error> {
    // What：读取文件中的向量配置，核验当前表模型及维度，并返回物理表 OID。
    // Why：配置只保留一个来源；不匹配时拒绝读写，OID 用于阻止在途任务跨代发布。
    let settings = crate::config::load_config("config.toml")
        .map_err(|_| sqlx::Error::protocol("无法读取 embedding 服务配置"))?
        .embedding_settings().ok_or_else(|| sqlx::Error::protocol("embedding 服务未配置"))?;
    let dimension = i32::try_from(settings.dimension).map_err(|_| sqlx::Error::protocol("配置中的 embedding 维度超出范围"))?;
    let mut tx = pool.begin().await?;
    sqlx::query("LOCK TABLE memory_embeddings IN ACCESS SHARE MODE").execute(&mut *tx).await?;
    let config: (i64, String, i32, f64) = sqlx::query_as(r#"
        SELECT 'memory_embeddings'::regclass::oid::bigint, $1::text, a.atttypmod, $3::double precision
        FROM pg_attribute a WHERE a.attrelid = 'memory_embeddings'::regclass
          AND a.attname = 'embedding' AND a.atttypid = 'vector'::regtype AND NOT a.attisdropped
          AND a.atttypmod = $2 AND a.atttypmod BETWEEN 1 AND 2000 AND btrim($1) <> '' AND $3 BETWEEN 0 AND 2
          AND NOT EXISTS (SELECT 1 FROM memory_embeddings WHERE embedding_model IS DISTINCT FROM $1
              OR embedding_dimension IS DISTINCT FROM $2 OR vector_dims(embedding) IS DISTINCT FROM $2)
    "#).bind(&settings.model).bind(dimension).bind(settings.fallback_max_distance)
        .fetch_optional(&mut *tx).await?
        .ok_or_else(|| sqlx::Error::protocol("embedding 配置无效或与当前向量表不一致"))?;
    tx.commit().await?;
    Ok(config)
}

pub async fn create_memory_embeddings_next(
    pool: &Pool<Postgres>,
    embeddings_dimension: usize,
    embedding_model: &str,
) -> Result<(), sqlx::Error> {
    // What：从当前向量表核验旧代身份，按内容、物理维度和模型准备下一代表。
    // Why：配置只来自文件，旧代身份取已存向量；空表不伪造模型，同维度异模型不混用。
    if !(1..=2000).contains(&embeddings_dimension) {
        return Err(sqlx::Error::protocol("embedding 维度必须在 1..=2000 范围内"));
    }
    if embedding_model.trim().is_empty() {
        return Err(sqlx::Error::protocol("embedding 模型不能为空"));
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('mem012:memory_embeddings_next', 0))")
        .execute(&mut *tx).await?;
    sqlx::query("LOCK TABLE memory_embeddings IN ACCESS SHARE MODE").execute(&mut *tx).await?;
    let (current_dimension, current_model, next_table): (i32, Option<String>, String) = sqlx::query_as(r#"
        SELECT a.atttypmod, (SELECT embedding_model FROM memory_embeddings LIMIT 1),
               quote_ident(n.nspname) || '.memory_embeddings_next'
        FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE c.oid = 'memory_embeddings'::regclass AND c.relkind = 'r' AND a.attname = 'embedding'
          AND a.atttypid = 'vector'::regtype AND a.atttypmod BETWEEN 1 AND 2000 AND NOT a.attisdropped
          AND NOT EXISTS (SELECT 1 FROM memory_embeddings e WHERE e.embedding_model IS NULL
              OR btrim(e.embedding_model) = ''
              OR e.embedding_model IS DISTINCT FROM (SELECT embedding_model FROM memory_embeddings LIMIT 1)
              OR e.embedding_dimension IS DISTINCT FROM a.atttypmod
              OR vector_dims(e.embedding) IS DISTINCT FROM a.atttypmod)
    "#).fetch_optional(&mut *tx).await?
        .ok_or_else(|| sqlx::Error::protocol("当前向量表模型或维度不一致，拒绝准备下一代"))?;
    if current_dimension == embeddings_dimension as i32 && current_model.as_deref() == Some(embedding_model) {
        return Err(sqlx::Error::protocol("目标模型和维度与当前代相同，无需换表"));
    }
    let next_dimension: Option<i32> = sqlx::query_scalar(r#"
        SELECT a.atttypmod FROM pg_attribute a
        WHERE a.attrelid = to_regclass($1) AND a.attname = 'embedding'
          AND a.atttypid = 'vector'::regtype AND NOT a.attisdropped
          AND EXISTS (SELECT 1 FROM pg_attribute m WHERE m.attrelid = a.attrelid
              AND m.attname = 'embedding_model' AND m.atttypid = 'text'::regtype AND NOT m.attisdropped)
    "#).bind(&next_table).fetch_optional(&mut *tx).await?;
    if next_dimension == Some(embeddings_dimension as i32) {
        sqlx::query(&format!("LOCK TABLE {next_table} IN SHARE MODE")).execute(&mut *tx).await?;
        let reusable: bool = sqlx::query_scalar(&format!(r#"
            SELECT EXISTS (SELECT 1 FROM {next_table}) AND NOT EXISTS (
                SELECT 1 FROM {next_table} WHERE embedding_model IS DISTINCT FROM $1
            )
        "#)).bind(embedding_model).fetch_one(&mut *tx).await?;
        if reusable {
            tx.commit().await?;
            return Ok(());
        }
    }
    sqlx::query(&format!("DROP TABLE IF EXISTS {next_table}")).execute(&mut *tx).await?;
    let create_sql = memory_embeddings_next_table_sql(embeddings_dimension)
        .replacen("CREATE TABLE memory_embeddings_next", &format!("CREATE TABLE {next_table}"), 1);
    sqlx::query(&create_sql).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

// Why：初始化入口必须独立于服务启动，避免运行态自动修改非当前 profile 库。
pub async fn init_db(
    pool: &Pool<Postgres>,
    profile: &str,
    reset_db: bool,
    embeddings_dimension: usize,
) -> Result<bool, sqlx::Error> {
    let db_label = memory_db_label(profile);
    if reset_db {
        // Why：init_db 只收敛当前 profile 对应库，避免隐式创建或清理其他 profile 库表结构。
        // Why：共享 profile 会继承目标 owner 权限，清理非自身 owner 的库会波及其他 profile。
        let (database, owner): (String, String) = sqlx::query_as(
            "SELECT current_database(), pg_get_userbyid(datdba) FROM pg_database WHERE datname = current_database()",
        )
        .fetch_one(pool)
        .await?;
        if let Some(error) = reset_db_owner_error(profile, &database, &owner) {
            return Err(sqlx::Error::protocol(error));
        }
        reset_memory_tables(pool, db_label.as_str()).await?;
    }

    migrate_memory_tables(pool, db_label.as_str(), true, embeddings_dimension).await?;

    Ok(true)
}

pub(crate) async fn init_profile_memory_tables(
    pool: &Pool<Postgres>,
    profile: &str,
    embeddings_dimension: usize,
) -> Result<(), sqlx::Error> {
    // What：只创建/迁移 mem012 主表和索引，不执行扩展或 AGE graph DDL。
    // Why：create_profile 已用 admin 完成扩展和权限设置，profile 连接只应验证运行期 schema 权限。
    let db_label = memory_db_label(profile);
    migrate_memory_tables(pool, db_label.as_str(), false, embeddings_dimension).await
}

async fn reset_memory_tables(pool: &Pool<Postgres>, db_label: &str) -> Result<(), sqlx::Error> {
    // What：调试重建清理本库源表、块表和生成目标。
    // Why：生成目标引用源表，须先于源表删除；运行参数继续由文件配置提供。
    let tables = [
        "memory_embeddings",
        "memory_embedding_targets",
        "memory_keywords",
        "memory_search_index",
        "memory_relations",
        "memory_usage",
        "memory_changes",
        "memory_graph_meta",
        "memory_units CASCADE",
    ];

    for table in tables {
        let sql = format!("DROP TABLE IF EXISTS {table}");
        sqlx::query(&sql).execute(pool).await?;
    }

    println!("debug: {db_label} memory_units 及派生表已删除");
    Ok(())
}

async fn migrate_memory_tables(
    pool: &Pool<Postgres>,
    db_label: &str,
    include_extension_setup: bool,
    embeddings_dimension: usize,
) -> Result<(), sqlx::Error> {
    // What：按调用方配置的维度初始化或检查本库结构。
    // Why：配置参数由文件入口提供，结构初始化不再维护另一份库内配置。
    drop_memory_handles(pool).await?;
    if schema_ready(pool).await? {
        ensure_memory_embeddings_dimension(pool, embeddings_dimension).await?;
        drop_memory_exclude_when(pool).await?;
        ensure_memory_summary_optional(pool).await?;
        ensure_memory_status_constraint(pool).await?;
        ensure_memory_change_identity(pool).await?;
        cr_memory_indexes(pool, db_label, include_extension_setup).await?;
        if include_extension_setup {
            ensure_memory_graph(pool).await?;
        }
        return Ok(());
    }

    println!("{db_label}: 开始初始化表");
    cr_normalize_title_function(pool).await?;
    cr_memory_units_table(pool, db_label).await?;
    ensure_memory_status_constraint(pool).await?;
    cr_memory_embeddings_table(pool, include_extension_setup, embeddings_dimension).await?;
    cr_memory_keywords_table(pool).await?;
    cr_memory_search_index_table(pool).await?;
    cr_memory_usage_table(pool).await?;
    cr_memory_relations_table(pool).await?;
    cr_memory_changes_table(pool).await?;
    ensure_memory_change_identity(pool).await?;
    cr_memory_graph_meta_table(pool).await?;
    cr_memory_indexes(pool, db_label, include_extension_setup).await?;
    if include_extension_setup {
        ensure_memory_graph(pool).await?;
    }
    Ok(())
}

async fn ensure_memory_embeddings_dimension(
    pool: &Pool<Postgres>,
    expected_dimension: usize,
) -> Result<(), sqlx::Error> {
    // What：按调用方传入的配置维度检查块表契约。
    // Why：文件配置是唯一参数来源，已有表不匹配时必须拒绝初始化，避免隐式覆盖。
    let expected_type = format!("vector({expected_dimension})");
    let check_pattern = format!("%embedding_dimension = {expected_dimension}%");
    let Some((actual_type, check_ok, chunks_ok)): Option<(String, bool, bool)> = sqlx::query_as(
        r#"
        SELECT format_type(a.atttypid, a.atttypmod), EXISTS (
            SELECT 1 FROM pg_constraint
            WHERE conrelid = 'public.memory_embeddings'::regclass
                AND contype = 'c'
                AND pg_get_constraintdef(oid) LIKE $1
        ), NOT EXISTS (
            SELECT 1 FROM (VALUES
                ('memory_uuid', 'uuid'), ('chunk_index', 'integer'),
                ('content_start_byte', 'bigint'), ('content_end_byte', 'bigint'),
                ('title_path', 'text'), ('embedding', $2), ('embedding_model', 'text'),
                ('embedding_dimension', 'integer'), ('source_revision', 'bigint'),
                ('embedding_policy', 'text'), ('target_generation', 'bigint'),
                ('embedded_at', 'timestamp with time zone')
            ) AS required(name, typ)
            LEFT JOIN pg_attribute col ON col.attrelid = 'public.memory_embeddings'::regclass
                AND col.attname = required.name AND NOT col.attisdropped
            WHERE col.attnum IS NULL OR (NOT col.attnotnull
                AND col.attname NOT IN ('embedding_policy', 'target_generation'))
                OR format_type(col.atttypid, col.atttypmod) <> required.typ
        ) AND EXISTS (
            SELECT 1 FROM pg_constraint c
            WHERE c.conrelid = 'public.memory_embeddings'::regclass AND c.contype = 'p'
                AND c.conkey = ARRAY[
                    (SELECT attnum FROM pg_attribute WHERE attrelid = c.conrelid AND attname = 'memory_uuid' AND NOT attisdropped),
                    (SELECT attnum FROM pg_attribute WHERE attrelid = c.conrelid AND attname = 'chunk_index' AND NOT attisdropped)
                ]
        )
        FROM pg_attribute a
        WHERE a.attrelid = 'public.memory_embeddings'::regclass
            AND a.attname = 'embedding'
            AND NOT a.attisdropped
        "#,
    )
    .bind(check_pattern)
    .bind(&expected_type)
    .fetch_optional(pool)
    .await?
    else {
        return Err(sqlx::Error::protocol(
            "memory_embeddings.embedding 列不存在，请先完成数据库迁移",
        ));
    };
    if !chunks_ok {
        return Err(sqlx::Error::protocol(
            "memory_embeddings 不是匹配配置的块表结构，请先完成数据库迁移",
        ));
    }
    if actual_type != expected_type || !check_ok {
        return Err(sqlx::Error::protocol(format!(
            "memory_embeddings 维度不匹配：数据库为 {actual_type}，配置要求 {expected_type}；请先清空旧向量并迁移表结构",
        )));
    }
    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS memory_embedding_targets (
            memory_uuid UUID PRIMARY KEY REFERENCES memory_units(uuid) ON DELETE CASCADE,
            generation BIGINT GENERATED ALWAYS AS IDENTITY,
            source_revision BIGINT NOT NULL CHECK (source_revision > 0),
            embedding_model TEXT NOT NULL CHECK (embedding_model <> ''),
            embedding_dimension INT NOT NULL CHECK (embedding_dimension BETWEEN 1 AND 2000),
            embedding_policy TEXT NOT NULL CHECK (embedding_policy <> '')
        )
    "#).execute(pool).await?;
    Ok(())
}

async fn ensure_memory_graph(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    // What：创建 AGE graph 运行所需的 memory_graph 结构。
    // Why：init_db 是 schema 收敛入口，不能让 rebuild 承担首次建 graph 的职责。
    let mut conn = pool.acquire().await?;
    sqlx::query("CREATE EXTENSION IF NOT EXISTS age")
        .execute(&mut *conn)
        .await?;
    match sqlx::query("LOAD 'age'").execute(&mut *conn).await {
        Ok(_) => {}
        Err(error)
            if error.as_database_error().is_some_and(|db_error| {
                db_error
                    .message()
                    .contains("access to library \"age\" is not allowed")
            }) => {}
        Err(error) => return Err(error),
    }
    let mut tx = conn.begin().await?;
    sqlx::query(r#"SET LOCAL search_path = ag_catalog, "$user", public"#)
        .execute(&mut *tx)
        .await?;
    let create_graph = sqlx::query(
        r#"
        DO $$
        BEGIN
            IF NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph') THEN
                PERFORM ag_catalog.create_graph('memory_graph');
            END IF;
        END $$;
        "#,
    )
    .execute(&mut *tx)
    .await;
    if let Err(error) = create_graph {
        let missing_database_create = error.as_database_error().is_some_and(|db_error| {
            db_error.code().as_deref() == Some("42501")
                && db_error
                    .message()
                    .contains("permission denied for database")
        });
        tx.rollback().await?;
        if missing_database_create {
            // Why：AGE 是可重建派生层，不能因为 graph schema 权限缺失阻塞记忆主数据写入。
            eprintln!("memory_graph 创建被数据库权限拒绝，跳过 AGE 派生图初始化: {error}");
            return Ok(());
        }
        return Err(error);
    }
    tx.commit().await?;
    Ok(())
}

async fn cr_memory_indexes(
    pool: &Pool<Postgres>,
    db_label: &str,
    include_extension_setup: bool,
) -> Result<(), sqlx::Error> {
    // Why：查询路径依赖不同访问模式，索引集中创建可以避免表结构和召回策略混在一起。
    if memory_indexes_ready(pool).await? {
        return Ok(());
    }

    if include_extension_setup {
        sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_trgm")
            .execute(pool)
            .await?;
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(pool)
            .await?;
    }

    let indexes = [
        "CREATE INDEX IF NOT EXISTS memory_units_category_status_idx ON memory_units (category, status)",
        "CREATE INDEX IF NOT EXISTS memory_units_status_updated_at_idx ON memory_units (status, updated_at)",
        "CREATE UNIQUE INDEX IF NOT EXISTS memory_units_pending_active_title_unique ON memory_units (category, title_norm) WHERE status IN ('pending', 'active')",
        "CREATE INDEX IF NOT EXISTS memory_embeddings_embedding_hnsw_idx ON memory_embeddings USING hnsw (embedding vector_cosine_ops)",
        "CREATE INDEX IF NOT EXISTS memory_embeddings_embedded_at_idx ON memory_embeddings (embedded_at)",
        "CREATE INDEX IF NOT EXISTS memory_usage_use_count_idx ON memory_usage (use_count)",
        "CREATE INDEX IF NOT EXISTS memory_usage_last_used_at_idx ON memory_usage (last_used_at)",
        "CREATE INDEX IF NOT EXISTS memory_relations_from_memory_uuid_idx ON memory_relations (from_memory_uuid)",
        "CREATE INDEX IF NOT EXISTS memory_relations_to_memory_uuid_idx ON memory_relations (to_memory_uuid)",
        "CREATE INDEX IF NOT EXISTS memory_relations_relation_type_idx ON memory_relations (relation_type)",
        "CREATE INDEX IF NOT EXISTS memory_changes_updated_at_idx ON memory_changes (updated_at)",
        "CREATE INDEX IF NOT EXISTS memory_keywords_keyword_norm_memory_uuid_idx ON memory_keywords (keyword_norm, memory_uuid)",
        "CREATE INDEX IF NOT EXISTS memory_keywords_keyword_norm_trgm_idx ON memory_keywords USING gin (keyword_norm gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_search_index_status_idx ON memory_search_index (status)",
        "CREATE INDEX IF NOT EXISTS memory_search_index_title_text_trgm_idx ON memory_search_index USING gin (title_text gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_search_index_summary_text_trgm_idx ON memory_search_index USING gin (summary_text gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_search_index_keywords_text_trgm_idx ON memory_search_index USING gin (keywords_text gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_search_index_content_text_trgm_idx ON memory_search_index USING gin (content_text gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_search_index_recall_when_text_trgm_idx ON memory_search_index USING gin (recall_when_text gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_search_index_all_text_trgm_idx ON memory_search_index USING gin (all_text gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_units_title_norm_trgm_idx ON memory_units USING gin (title_norm gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_units_summary_trgm_idx ON memory_units USING gin (summary gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_units_content_trgm_idx ON memory_units USING gin (content gin_trgm_ops)",
    ];

    for index_sql in indexes {
        sqlx::query(index_sql).execute(pool).await?;
    }

    println!("{db_label}: memory 索引创建成功");
    Ok(())
}

async fn drop_memory_exclude_when(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    // Why：exclude_when 已退出 schema，旧库启动时必须收敛表结构，避免代码和数据库长期漂移。
    sqlx::query("ALTER TABLE memory_units DROP COLUMN IF EXISTS exclude_when")
        .execute(pool)
        .await?;
    Ok(())
}

async fn drop_memory_handles(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    // Why：handle 已退出核心模型，旧表必须在启动迁移时清掉，避免状态快照继续依赖它。
    sqlx::query("DROP TABLE IF EXISTS memory_handles")
        .execute(pool)
        .await?;
    Ok(())
}

async fn ensure_memory_summary_optional(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    // Why：summary 只是辅助摘要，缺失时不能阻塞记忆本体写入。
    sqlx::query("ALTER TABLE memory_units ALTER COLUMN summary DROP NOT NULL")
        .execute(pool)
        .await?;
    Ok(())
}

async fn ensure_memory_status_constraint(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    // Why：pending 是新的写入态，旧库不迁移约束会在 create_memory 时被数据库拒绝。
    sqlx::query("ALTER TABLE memory_units DROP CONSTRAINT IF EXISTS memory_units_status_check")
        .execute(pool)
        .await?;
    sqlx::query(
        "ALTER TABLE memory_units ADD CONSTRAINT memory_units_status_check CHECK (status IN ('pending', 'active', 'trashed'))",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_memory_change_identity(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    // What：迁移 memory_changes 的审核 ID 约束。
    // Why：CLI 可能并发启动，约束 drop/add 必须串行化，否则两个 init_db 会重复 ADD CONSTRAINT。
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('mem012:init_db:memory_changes_identity'))")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE memory_changes SET uuid = memory_uuid WHERE uuid <> memory_uuid")
        .execute(&mut *tx)
        .await?;
    sqlx::query("ALTER TABLE memory_changes DROP CONSTRAINT IF EXISTS memory_changes_uuid_matches_memory_uuid")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "ALTER TABLE memory_changes ADD CONSTRAINT memory_changes_uuid_matches_memory_uuid CHECK (uuid = memory_uuid)",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn cr_memory_search_index_table(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    // What：创建 search_memory 使用的派生搜索投影表。
    // Why：字面搜索需要提前摊平当前工作态文本，避免查询时反复 join 和聚合关键词。
    sqlx::query(
        r#"
        CREATE TABLE memory_search_index (
            memory_uuid UUID PRIMARY KEY REFERENCES memory_units(uuid) ON DELETE CASCADE,
            status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'trashed')),
            title_text TEXT NOT NULL DEFAULT '',
            summary_text TEXT NOT NULL DEFAULT '',
            keywords_text TEXT NOT NULL DEFAULT '',
            content_text TEXT NOT NULL DEFAULT '',
            recall_when_text TEXT NOT NULL DEFAULT '',
            all_text TEXT NOT NULL DEFAULT '',
            indexed_at TIMESTAMPTZ NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn memory_indexes_ready(pool: &Pool<Postgres>) -> Result<bool, sqlx::Error> {
    // Why：CREATE INDEX IF NOT EXISTS 仍会逐条访问数据库，先检测完整性可以让已初始化库直接跳过索引阶段。
    let ready: bool = sqlx::query_scalar(
        r#"
        SELECT
            to_regclass('public.memory_units_category_status_idx') IS NOT NULL
            AND to_regclass('public.memory_units_status_updated_at_idx') IS NOT NULL
            AND to_regclass('public.memory_units_pending_active_title_unique') IS NOT NULL
            AND to_regclass('public.memory_embeddings_embedding_hnsw_idx') IS NOT NULL
            AND to_regclass('public.memory_embeddings_embedded_at_idx') IS NOT NULL
            AND to_regclass('public.memory_usage_use_count_idx') IS NOT NULL
            AND to_regclass('public.memory_usage_last_used_at_idx') IS NOT NULL
            AND to_regclass('public.memory_relations_from_memory_uuid_idx') IS NOT NULL
            AND to_regclass('public.memory_relations_to_memory_uuid_idx') IS NOT NULL
            AND to_regclass('public.memory_relations_relation_type_idx') IS NOT NULL
            AND to_regclass('public.memory_changes_updated_at_idx') IS NOT NULL
            AND to_regclass('public.memory_keywords_keyword_norm_memory_uuid_idx') IS NOT NULL
            AND to_regclass('public.memory_keywords_keyword_norm_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_search_index_status_idx') IS NOT NULL
            AND to_regclass('public.memory_search_index_title_text_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_search_index_summary_text_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_search_index_keywords_text_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_search_index_content_text_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_search_index_recall_when_text_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_search_index_all_text_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_units_title_norm_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_units_summary_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_units_content_trgm_idx') IS NOT NULL
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(ready)
}

async fn cr_normalize_title_function(pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    // Why：title_norm 的规范化必须由数据库兜底，否则不同写入入口会产生不同的同名判断。
    sqlx::query(
        r#"
        CREATE OR REPLACE FUNCTION normalize_title(input text)
        RETURNS text
        LANGUAGE sql
        IMMUTABLE
        AS $$
            SELECT regexp_replace(lower(trim(input)), '[[:space:]]+', ' ', 'g');
        $$;
    "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn cr_memory_graph_meta_table(_pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    let cr_graph_meta = sqlx::query(
        r#"
        CREATE TABLE memory_graph_meta (
            graph_name TEXT PRIMARY KEY,
            dirty BOOLEAN NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL,
            CHECK (graph_name <> '')
        );
    "#,
    )
    .execute(_pool)
    .await;

    match cr_graph_meta {
        Ok(_) => {
            println!("memory_graph_meta 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_graph_meta 表创建失败: {error}");
            Err(error)
        }
    }
}

async fn cr_memory_changes_table(_pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    let cr_changes = sqlx::query(
        r#"
        CREATE TABLE memory_changes (
        uuid UUID PRIMARY KEY,
        memory_uuid UUID NOT NULL,
        action TEXT NOT NULL,
        before_state JSONB,
        after_state JSONB,
        created_at TIMESTAMPTZ NOT NULL,
        updated_at TIMESTAMPTZ NOT NULL,
        CHECK (action IN ('create', 'update', 'delete', 'restore')),
        CONSTRAINT memory_changes_uuid_matches_memory_uuid CHECK (uuid = memory_uuid),
        UNIQUE(memory_uuid)
        );
    "#,
    )
    .execute(_pool)
    .await;

    match cr_changes {
        Ok(_) => {
            println!("memory_changes 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_changes 表创建失败: {error}");
            Err(error)
        }
    }
}

async fn cr_memory_relations_table(_pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    let cr_relations = sqlx::query(
        r#"
        CREATE TABLE memory_relations (
            uuid UUID PRIMARY KEY,
            from_memory_uuid UUID NOT NULL REFERENCES memory_units(uuid) ON DELETE CASCADE,
            to_memory_uuid UUID NOT NULL REFERENCES memory_units(uuid) ON DELETE CASCADE,
            relation_type TEXT NOT NULL,
            weight INT,
            note TEXT,
            created_at TIMESTAMPTZ NOT NULL,
            CHECK (from_memory_uuid != to_memory_uuid),
            UNIQUE(from_memory_uuid, to_memory_uuid, relation_type),
            CHECK (weight IS NULL OR weight BETWEEN 0 AND 100),
            CHECK (relation_type IN (
                'related_to',
                'supersedes',
                'depends_on',
                'conflicts_with',
                'elaborates',
                'applies_to'
            ))
        );
    "#,
    )
    .execute(_pool)
    .await;

    match cr_relations {
        Ok(_) => {
            println!("memory_relations 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_relations 表创建失败: {error}");
            Err(error)
        }
    }
}

async fn cr_memory_usage_table(_pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    let cr_usage = sqlx::query(
        r#"
        CREATE TABLE memory_usage (
            memory_uuid UUID PRIMARY KEY REFERENCES memory_units(uuid) ON DELETE CASCADE,
            use_count INT NOT NULL DEFAULT 0,
            last_used_at TIMESTAMPTZ,
            CHECK (use_count >= 0)
        );
    "#,
    )
    .execute(_pool)
    .await;

    match cr_usage {
        Ok(_) => {
            println!("memory_usage 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_usage 表创建失败: {error}");
            Err(error)
        }
    }
}

async fn cr_memory_keywords_table(_pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    let cr_keywords = sqlx::query(
        r#"
        CREATE TABLE memory_keywords (
            uuid UUID PRIMARY KEY,
            memory_uuid UUID NOT NULL REFERENCES memory_units(uuid) ON DELETE CASCADE,
            keyword_norm TEXT NOT NULL,
            weight INT,
            created_at TIMESTAMPTZ NOT NULL,
            UNIQUE(memory_uuid, keyword_norm),
            CHECK (keyword_norm <> ''),
            CHECK (weight IS NULL OR weight BETWEEN 0 AND 100)
        );"#,
    )
    .execute(_pool)
    .await;

    match cr_keywords {
        Ok(_) => {
            println!("memory_keywords 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_keywords 表创建失败: {error}");
            Err(error)
        }
    }
}

async fn schema_ready(_pool: &sqlx::Pool<sqlx::Postgres>) -> Result<bool, sqlx::Error> {
    // Why：只检查一张核心表会把半初始化状态误判为可用，启动阶段必须确认整套基础 schema 已存在。
    let ready: bool = sqlx::query_scalar(
        r#"
        SELECT
            to_regclass('public.memory_units') IS NOT NULL
            AND to_regclass('public.memory_embeddings') IS NOT NULL
            AND to_regclass('public.memory_keywords') IS NOT NULL
            AND to_regclass('public.memory_search_index') IS NOT NULL
            AND to_regclass('public.memory_usage') IS NOT NULL
            AND to_regclass('public.memory_relations') IS NOT NULL
            AND to_regclass('public.memory_changes') IS NOT NULL
            AND to_regclass('public.memory_graph_meta') IS NOT NULL
            AND to_regprocedure('public.normalize_title(text)') IS NOT NULL
        "#,
    )
    .fetch_one(_pool)
    .await?;

    Ok(ready)
}

async fn cr_memory_units_table(
    _pool: &sqlx::Pool<sqlx::Postgres>,
    db_label: &str,
) -> Result<(), sqlx::Error> {
    let category_scope_check = match db_label {
        "share" => "CHECK (category = 'share')",
        _ => "CHECK (category <> 'share')",
    };
    let create_sql = format!(
        r#"
        CREATE TABLE memory_units (
            uuid UUID PRIMARY KEY,
            category TEXT NOT NULL,
            title_norm TEXT NOT NULL,
            content TEXT NOT NULL,
            summary TEXT,
            revision BIGINT NOT NULL DEFAULT 1,
            status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'trashed')),
            recall_when TEXT,
            trashed_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL,
            CHECK (category <> ''),
            CHECK (category ~ '^[a-z][a-z0-9_]*$'),
            CHECK (title_norm <> ''),
            CHECK (title_norm = normalize_title(title_norm)),
            CHECK ((status = 'trashed') = (trashed_at IS NOT NULL)),
            {category_scope_check}
        );
        "#,
    );

    let cr_memory_units = sqlx::query(&create_sql).execute(_pool).await;

    match cr_memory_units {
        Ok(_) => {
            sqlx::query(
                r#"
                CREATE OR REPLACE FUNCTION bump_memory_units_revision()
                RETURNS trigger
                LANGUAGE plpgsql
                AS $$
                BEGIN
                    NEW.revision := OLD.revision + 1;
                    RETURN NEW;
                END;
                $$;
                "#,
            )
            .execute(_pool)
            .await?;
            sqlx::query(
                r#"
                CREATE TRIGGER memory_units_revision_bump
                BEFORE UPDATE ON memory_units
                FOR EACH ROW
                EXECUTE FUNCTION bump_memory_units_revision();
                "#,
            )
            .execute(_pool)
            .await?;
            println!("memory_units 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_units 表创建失败: {error}");
            Err(error)
        }
    }
}

async fn cr_memory_embeddings_table(
    pool: &sqlx::Pool<sqlx::Postgres>,
    include_extension_setup: bool,
    embeddings_dimension: usize,
) -> Result<(), sqlx::Error> {
    // What：在同一事务创建正常块表及每条记忆的当前生成目标。
    // Why：目标独立于已发布块，策略切换后的生成失败也不能放行旧任务覆盖有效索引。
    // Why：vector 类型来自 pgvector 扩展，建 embedding 表前必须先让当前数据库启用它。
    if include_extension_setup {
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(pool)
            .await?;
    }

    let create_sql = memory_embeddings_table_sql(embeddings_dimension);
    let mut tx = pool.begin().await?;
    let cr_memory_embeddings = sqlx::query(&create_sql).execute(&mut *tx).await;

    match cr_memory_embeddings {
        Ok(_) => {
            sqlx::query(r#"
                CREATE TABLE memory_embedding_targets (
                    memory_uuid UUID PRIMARY KEY REFERENCES memory_units(uuid) ON DELETE CASCADE,
                    generation BIGINT GENERATED ALWAYS AS IDENTITY,
                    source_revision BIGINT NOT NULL CHECK (source_revision > 0),
                    embedding_model TEXT NOT NULL CHECK (embedding_model <> ''),
                    embedding_dimension INT NOT NULL CHECK (embedding_dimension BETWEEN 1 AND 2000),
                    embedding_policy TEXT NOT NULL CHECK (embedding_policy <> '')
                )
            "#).execute(&mut *tx).await?;
            tx.commit().await?;
            println!("memory_embeddings 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_embeddings 表创建失败: {error}");
            Err(error)
        }
    }
}
