use sqlx::{Pool, Postgres};

// Why：初始化入口必须独立于服务启动，避免运行态自动修改 profile 私库或共享库。
pub async fn init_db(
    pool: &Pool<Postgres>,
    share_pool: &Pool<Postgres>,
) -> Result<bool, sqlx::Error> {
    // DEBUG
    // if true {
    //     reset_memory_tables(pool, "profile").await?;
    //     reset_memory_tables(share_pool, "share").await?;
    // }

    migrate_memory_tables(pool, "profile").await?;
    migrate_memory_tables(share_pool, "share").await?;

    Ok(true)
}

async fn reset_memory_tables(pool: &Pool<Postgres>, db_label: &str) -> Result<(), sqlx::Error> {
    // Why：调试阶段两个库都要回到空 schema，否则 share 库会被旧表状态误判为已初始化。
    let tables = [
        "memory_embeddings",
        "memory_keywords",
        "memory_handles",
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

async fn migrate_memory_tables(pool: &Pool<Postgres>, db_label: &str) -> Result<(), sqlx::Error> {
    // Why：profile 库和 share 库结构一致，复用同一套建表顺序可以避免 schema 漂移。
    if schema_ready(pool).await? {
        println!("{db_label}: 跳过初始化");
        cr_memory_indexes(pool, db_label).await?;
        return Ok(());
    }

    println!("{db_label}: 开始初始化表");
    cr_normalize_title_function(pool).await?;
    cr_memory_units_table(pool, db_label).await?;
    cr_memory_embeddings_table(pool).await?;
    cr_memory_keywords_table(pool).await?;
    cr_memory_handles_table(pool).await?;
    cr_memory_usage_table(pool).await?;
    cr_memory_relations_table(pool).await?;
    cr_memory_changes_table(pool).await?;
    cr_memory_graph_meta_table(pool).await?;
    cr_memory_indexes(pool, db_label).await?;
    Ok(())
}

async fn cr_memory_indexes(pool: &Pool<Postgres>, db_label: &str) -> Result<(), sqlx::Error> {
    // Why：查询路径依赖不同访问模式，索引集中创建可以避免表结构和召回策略混在一起。
    if memory_indexes_ready(pool).await? {
        println!("{db_label}: memory 索引已完整，跳过创建");
        return Ok(());
    }

    sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_trgm")
        .execute(pool)
        .await?;
    sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
        .execute(pool)
        .await?;

    let indexes = [
        "CREATE INDEX IF NOT EXISTS memory_units_category_status_idx ON memory_units (category, status)",
        "CREATE INDEX IF NOT EXISTS memory_units_status_updated_at_idx ON memory_units (status, updated_at)",
        "CREATE UNIQUE INDEX IF NOT EXISTS memory_units_active_title_unique ON memory_units (category, title_norm) WHERE status = 'active'",
        "CREATE INDEX IF NOT EXISTS memory_embeddings_embedding_hnsw_idx ON memory_embeddings USING hnsw (embedding vector_cosine_ops)",
        "CREATE INDEX IF NOT EXISTS memory_embeddings_embedded_at_idx ON memory_embeddings (embedded_at)",
        "CREATE INDEX IF NOT EXISTS memory_usage_use_count_idx ON memory_usage (use_count)",
        "CREATE INDEX IF NOT EXISTS memory_usage_last_used_at_idx ON memory_usage (last_used_at)",
        "CREATE INDEX IF NOT EXISTS memory_relations_from_memory_uuid_idx ON memory_relations (from_memory_uuid)",
        "CREATE INDEX IF NOT EXISTS memory_relations_to_memory_uuid_idx ON memory_relations (to_memory_uuid)",
        "CREATE INDEX IF NOT EXISTS memory_relations_relation_type_idx ON memory_relations (relation_type)",
        "CREATE INDEX IF NOT EXISTS memory_changes_updated_at_idx ON memory_changes (updated_at)",
        "CREATE INDEX IF NOT EXISTS memory_keywords_keyword_norm_memory_uuid_idx ON memory_keywords (keyword_norm, memory_uuid)",
        "CREATE INDEX IF NOT EXISTS memory_handles_handle_norm_trgm_idx ON memory_handles USING gin (handle_norm gin_trgm_ops)",
        "CREATE INDEX IF NOT EXISTS memory_keywords_keyword_norm_trgm_idx ON memory_keywords USING gin (keyword_norm gin_trgm_ops)",
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

async fn memory_indexes_ready(pool: &Pool<Postgres>) -> Result<bool, sqlx::Error> {
    // Why：CREATE INDEX IF NOT EXISTS 仍会逐条访问数据库，先检测完整性可以让已初始化库直接跳过索引阶段。
    let ready: bool = sqlx::query_scalar(
        r#"
        SELECT
            to_regclass('public.memory_units_category_status_idx') IS NOT NULL
            AND to_regclass('public.memory_units_status_updated_at_idx') IS NOT NULL
            AND to_regclass('public.memory_units_active_title_unique') IS NOT NULL
            AND to_regclass('public.memory_embeddings_embedding_hnsw_idx') IS NOT NULL
            AND to_regclass('public.memory_embeddings_embedded_at_idx') IS NOT NULL
            AND to_regclass('public.memory_usage_use_count_idx') IS NOT NULL
            AND to_regclass('public.memory_usage_last_used_at_idx') IS NOT NULL
            AND to_regclass('public.memory_relations_from_memory_uuid_idx') IS NOT NULL
            AND to_regclass('public.memory_relations_to_memory_uuid_idx') IS NOT NULL
            AND to_regclass('public.memory_relations_relation_type_idx') IS NOT NULL
            AND to_regclass('public.memory_changes_updated_at_idx') IS NOT NULL
            AND to_regclass('public.memory_keywords_keyword_norm_memory_uuid_idx') IS NOT NULL
            AND to_regclass('public.memory_handles_handle_norm_trgm_idx') IS NOT NULL
            AND to_regclass('public.memory_keywords_keyword_norm_trgm_idx') IS NOT NULL
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

async fn cr_memory_handles_table(_pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    let cr_handles = sqlx::query(
        r#"
        CREATE TABLE memory_handles(
            uuid UUID PRIMARY KEY,
            memory_uuid UUID NOT NULL REFERENCES memory_units(uuid) ON DELETE CASCADE,
            handle_norm TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL,
            CHECK (handle_norm <> ''),
            CHECK (handle_norm !~ '(^/|/$|//)'),
            UNIQUE(handle_norm)
        );
    "#,
    )
    .execute(_pool)
    .await;

    match cr_handles {
        Ok(_) => {
            println!("memory_handles 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_handles 表创建失败: {error}");
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
            AND to_regclass('public.memory_handles') IS NOT NULL
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
            summary TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('active', 'trashed')),
            recall_when TEXT,
            exclude_when TEXT,
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
            println!("memory_units 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_units 表创建失败: {error}");
            Err(error)
        }
    }
}

async fn cr_memory_embeddings_table(pool: &sqlx::Pool<sqlx::Postgres>) -> Result<(), sqlx::Error> {
    // Why：vector 类型来自 pgvector 扩展，建 embedding 表前必须先让当前数据库启用它。
    sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
        .execute(pool)
        .await?;

    let cr_memory_embeddings = sqlx::query(
        r#"
        CREATE TABLE memory_embeddings (
            memory_uuid UUID PRIMARY KEY REFERENCES memory_units(uuid) ON DELETE CASCADE,
            embedding vector(1024) NOT NULL,
            embedding_model TEXT NOT NULL,
            embedding_dimension INT NOT NULL CHECK (embedding_dimension = 1024),
            embedded_at TIMESTAMPTZ NOT NULL,
            CHECK (embedding_model <> '')
        );
        "#,
    )
    .execute(pool)
    .await;

    match cr_memory_embeddings {
        Ok(_) => {
            println!("memory_embeddings 表创建成功");
            Ok(())
        }
        Err(error) => {
            eprintln!("memory_embeddings 表创建失败: {error}");
            Err(error)
        }
    }
}
