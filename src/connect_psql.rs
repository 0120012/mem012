use sqlx::postgres::PgPoolOptions;


// Why：初始化入口必须独立于服务启动，避免运行态自动修改 profile 私库或共享库。
pub async fn init_db(database_url: &str) -> Result<bool, sqlx::Error> {

    // 1\ connect_db
    let pool = match PgPoolOptions::new().connect(database_url).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("PostgreSQL 连接失败: {error}");
            return Err(error);
        }
    };

    if true {
        // Why：调试建表流程时需要回到未初始化状态，否则 schema_ready 会直接跳过初始化。
        sqlx::query("DROP TABLE IF EXISTS memory_embeddings")
            .execute(&pool)
            .await?;
        sqlx::query("DROP TABLE IF EXISTS memory_units CASCADE")
            .execute(&pool)
            .await?;
        println!("debug: memory_units 相关表已删除");
    }

    // ready == true  -> 不建表
    // ready == false -> 建表
    let ready = schema_ready(&pool).await?;
    if !ready {
        println!("开始初始化表");
        cr_memory_units_db(&pool).await?;
        cr_memory_embeddings_db(&pool).await?;
    }
    else {
        println!("跳过初始化");
    }

    Ok(true)
}

// Why：启动阶段只需要知道核心表是否存在，不应该把连通性探针误当成初始化判断。

async fn schema_ready(_pool: &sqlx::Pool<sqlx::Postgres>) -> Result<bool, sqlx::Error> {

    let exists: Option<String> = sqlx::query_scalar(
        "select to_regclass('public.memory_units')::text"
    )
        .fetch_one(_pool)
        .await?;

    Ok(exists.is_some())
}

async fn cr_memory_units_db(_pool: &sqlx::Pool<sqlx::Postgres>) -> Result<(), sqlx::Error> {
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

async fn cr_memory_embeddings_db(pool: &sqlx::Pool<sqlx::Postgres>) -> Result<(), sqlx::Error> {
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
