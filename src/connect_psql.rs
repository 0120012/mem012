use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};

// Why：初始化入口必须独立于服务启动，避免运行态自动修改 profile 私库或共享库。
pub async fn init_db(database_url: &str, share_database_url: &str) -> Result<bool, sqlx::Error> {
    // 1\ connect_db
    let pool = match PgPoolOptions::new().connect(database_url).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("PostgreSQL 连接失败: {error}");
            return Err(error);
        }
    };

    let share_pool = match PgPoolOptions::new().connect(share_database_url).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("PostgreSQL mem_share 连接失败: {error}");
            return Err(error);
        }
    };

    // DEBUG
    if true {
        reset_memory_tables(&pool, "profile").await?;
        reset_memory_tables(&share_pool, "share").await?;
    }

    migrate_memory_tables(&pool, "profile").await?;
    migrate_memory_tables(&share_pool, "share").await?;

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
        return Ok(());
    }

    println!("{db_label}: 开始初始化表");
    cr_memory_units_table(pool).await?;
    cr_memory_embeddings_table(pool).await?;
    cr_memory_keywords_table(pool).await?;
    cr_memory_handles_table(pool).await?;
    cr_memory_usage_table(pool).await?;
    cr_memory_relations_table(pool).await?;
    cr_memory_changes_table(pool).await?;
    cr_memory_graph_meta_table(pool).await?;
    Ok(())
}

async fn cr_memory_graph_meta_table(_pool: &Pool<Postgres>) -> Result<(), sqlx::Error> {
    let cr_graph_meta = sqlx::query(
        r#"
        CREATE TABLE memory_graph_meta (
            graph_name TEXT PRIMARY KEY,
            dirty BOOLEAN NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL
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
        updated_at TIMESTAMPTZ NOT NULL
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
            CHECK (weight IS NULL OR weight BETWEEN 0 AND 100)
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
            last_used_at TIMESTAMPTZ
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

// Why：启动阶段只需要知道核心表是否存在，不应该把连通性探针误当成初始化判断。

async fn schema_ready(_pool: &sqlx::Pool<sqlx::Postgres>) -> Result<bool, sqlx::Error> {
    let exists: Option<String> =
        sqlx::query_scalar("select to_regclass('public.memory_units')::text")
            .fetch_one(_pool)
            .await?;

    Ok(exists.is_some())
}

async fn cr_memory_units_table(_pool: &sqlx::Pool<sqlx::Postgres>) -> Result<(), sqlx::Error> {
    let cr_memory_units = sqlx::query(
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
            CHECK (title_norm <> '')
        );
        "#,
    )
    .execute(_pool)
    .await;

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
            embedded_at TIMESTAMPTZ NOT NULL
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
