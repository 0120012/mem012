pub(crate) fn quoted_pg_identifier(identifier: &str) -> Result<String, Box<dyn std::error::Error>> {
    // What：把受限 identifier 包成 PostgreSQL 双引号标识符。
    // Why：PostgreSQL 不支持绑定 role/database/schema 名，所有动态 identifier 必须先收敛到安全字符集。
    let valid = identifier
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_lowercase)
        && identifier
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if !valid {
        return Err("PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*".into());
    }
    Ok(format!("\"{identifier}\""))
}

fn validate_profile_name(profile: &str) -> Result<(), Box<dyn std::error::Error>> {
    quoted_pg_identifier(profile)?;
    if profile.len() > crate::PROFILE_NAME_MAX_LEN {
        return Err(format!("profile 名称长度不能超过 {}", crate::PROFILE_NAME_MAX_LEN).into());
    }
    if matches!(profile, "postgres" | "template0" | "template1") {
        return Err("profile 名称是保留名".into());
    }
    Ok(())
}

const PROFILE_ADMIN_RESOURCES_SQL: &str = "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = $1), EXISTS(SELECT 1 FROM pg_database WHERE datname = $2)";

fn profile_admin_resources_conflict_error(
    profile: &str,
    role_exists: bool,
    database_exists: bool,
) -> Option<String> {
    match (role_exists, database_exists) {
        (true, true) => Some(format!(
            "远端资源已存在: role `{profile}`, database `mem_{profile}`"
        )),
        (true, false) => Some(format!("远端资源已存在: role `{profile}`")),
        (false, true) => Some(format!("远端资源已存在: database `mem_{profile}`")),
        (false, false) => None,
    }
}

pub(crate) async fn ensure_profile_admin_resources_absent(
    pool: &sqlx::Pool<sqlx::Postgres>,
    profile: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：在 admin DB 中预检目标 role 和 database 是否已存在。
    // Why：提前失败可以避免生成新密码、拼配置文本或执行部分 DDL 后再撞上远端冲突。
    validate_profile_name(profile)?;
    let database_name = format!("mem_{profile}");
    let (role_exists, database_exists): (bool, bool) = sqlx::query_as(PROFILE_ADMIN_RESOURCES_SQL)
        .bind(profile)
        .bind(&database_name)
        .fetch_one(pool)
        .await?;
    if let Some(error) =
        profile_admin_resources_conflict_error(profile, role_exists, database_exists)
    {
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn create_role_sql(
    profile: &str,
    password: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成单个 profile role 的 PostgreSQL DDL。
    // Why：role 名不能参数绑定，必须先校验 identifier；密码用 SQL 字面量转义，避免拼接注入。
    if password.is_empty() || password.contains('\0') {
        return Err("profile 密码不能为空且不能包含 NUL".into());
    }
    let role = quoted_pg_identifier(profile)?;
    let password = password.replace('\'', "''");
    Ok(format!("CREATE ROLE {role} LOGIN PASSWORD '{password}'"))
}

pub(crate) fn create_database_sql(profile: &str) -> Result<String, Box<dyn std::error::Error>> {
    // What：按 profile 生成对应 mem_<profile> database 的 PostgreSQL DDL。
    // Why：database 名和 owner 都是动态 identifier，必须在同一边界内校验后再拼接。
    let owner = quoted_pg_identifier(profile)?;
    let database_name = format!("mem_{profile}");
    let database = quoted_pg_identifier(&database_name)?;
    Ok(format!("CREATE DATABASE {database} OWNER {owner}"))
}

pub(crate) fn terminate_profile_database_connections_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成终止目标 profile database 现有连接的 SQL。
    // Why：DROP DATABASE 需要独占目标库，补偿清理必须先断开本轮初始化留下的连接。
    validate_profile_name(profile)?;
    let database_name = format!("mem_{profile}").replace('\'', "''");
    Ok(format!(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{database_name}' AND pid <> pg_backend_pid()"
    ))
}

pub(crate) fn drop_profile_database_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成删除 profile database 的补偿 DDL。
    // Why：create_profile 失败时只能清理由 profile 派生的新库，不能接收任意 database 名。
    validate_profile_name(profile)?;
    let database = quoted_pg_identifier(&format!("mem_{profile}"))?;
    Ok(format!("DROP DATABASE IF EXISTS {database}"))
}

pub(crate) fn drop_profile_role_sql(profile: &str) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成删除 profile role 的补偿 DDL。
    // Why：role 名来自用户输入且不能参数绑定，删除前必须复用同一 identifier 边界。
    validate_profile_name(profile)?;
    let role = quoted_pg_identifier(profile)?;
    Ok(format!("DROP ROLE IF EXISTS {role}"))
}

pub(crate) fn revoke_public_connect_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成撤销 PUBLIC 连接 profile database 的 PostgreSQL DDL。
    // Why：数据库名由 profile 派生，先校验 profile 再拼接 mem_ 前缀，避免空 profile 绕过边界。
    quoted_pg_identifier(profile)?;
    let database_name = format!("mem_{profile}");
    let database = quoted_pg_identifier(&database_name)?;
    Ok(format!("REVOKE CONNECT ON DATABASE {database} FROM PUBLIC"))
}

pub(crate) fn grant_connect_sql(profile: &str) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 连接自身 database 的 PostgreSQL DDL。
    // Why：授权目标 role 和 database 都来自 profile，必须在同一边界校验后再拼接。
    let role = quoted_pg_identifier(profile)?;
    let database_name = format!("mem_{profile}");
    let database = quoted_pg_identifier(&database_name)?;
    Ok(format!("GRANT CONNECT ON DATABASE {database} TO {role}"))
}

pub(crate) fn grant_public_schema_usage_create_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 使用并创建 public schema 对象的 DDL。
    // Why：mem012 表结构由 profile 连接初始化，必须显式授予 public schema 写入边界。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!("GRANT USAGE, CREATE ON SCHEMA public TO {role}"))
}

pub(crate) fn grant_public_tables_dml_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 读写 public 现有表的 DDL。
    // Why：mem012 主表创建在 public schema，profile 初始化和正常运行都需要最小 DML 权限。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {role}"
    ))
}

pub(crate) fn grant_public_sequences_usage_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 使用 public 现有 sequence 的 DDL。
    // Why：mem012 表可能依赖 public sequence，profile 写入时需要读取并推进序列。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO {role}"
    ))
}

pub(crate) fn grant_public_tables_default_privileges_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 未来读写 public 表的 default privileges DDL。
    // Why：后续由 profile 创建的新表也应继承同一 DML 权限，避免 schema 初始化后的权限漂移。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "ALTER DEFAULT PRIVILEGES FOR ROLE {role} IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {role}"
    ))
}

pub(crate) fn grant_public_sequences_default_privileges_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 未来使用 public sequence 的 default privileges DDL。
    // Why：后续由 profile 创建的新 sequence 也应继承同一使用权限，避免写入路径权限漂移。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "ALTER DEFAULT PRIVILEGES FOR ROLE {role} IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO {role}"
    ))
}

pub(crate) fn grant_ag_catalog_schema_usage_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 使用 ag_catalog schema 的 DDL。
    // Why：AGE 查询和图初始化依赖 ag_catalog 对象，profile role 需要最小可见权限。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!("GRANT USAGE ON SCHEMA ag_catalog TO {role}"))
}

pub(crate) fn grant_agtype_usage_sql(profile: &str) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 使用 AGE agtype 类型的 DDL。
    // Why：mem012 的 AGE 查询会读写 agtype 值，role 需要类型使用权限才能执行 schema 初始化。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!("GRANT USAGE ON TYPE ag_catalog.agtype TO {role}"))
}

pub(crate) fn grant_ag_catalog_functions_execute_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 执行 ag_catalog 函数的 DDL。
    // Why：AGE 的 create_graph 和查询函数位于 ag_catalog，profile 初始化图结构时需要执行权限。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO {role}"
    ))
}

pub(crate) fn grant_memory_graph_schema_usage_create_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 使用并创建 memory_graph schema 对象的 DDL。
    // Why：memory_graph 由 AGE create_graph 创建，profile 后续写图对象需要限定在该 schema 内的权限。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "GRANT USAGE, CREATE ON SCHEMA memory_graph TO {role}"
    ))
}

pub(crate) fn grant_memory_graph_tables_dml_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 读写 memory_graph 现有表的 DDL。
    // Why：AGE 图结构表由 create_graph 创建，profile 需要 DML 权限才能维护图数据。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO {role}"
    ))
}

pub(crate) fn grant_memory_graph_sequences_usage_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 使用 memory_graph 现有 sequence 的 DDL。
    // Why：AGE 图结构会通过 sequence 维护内部标识，profile 写图数据时需要读取并推进序列。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO {role}"
    ))
}

pub(crate) fn grant_memory_graph_tables_default_privileges_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 未来读写 memory_graph 表的 default privileges DDL。
    // Why：后续 AGE 图表若由 profile 创建，也应继承同一 DML 权限以保持图写入路径稳定。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "ALTER DEFAULT PRIVILEGES FOR ROLE {role} IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {role}"
    ))
}

pub(crate) fn grant_memory_graph_sequences_default_privileges_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成授权 profile 未来使用 memory_graph sequence 的 default privileges DDL。
    // Why：后续 AGE sequence 若由 profile 创建，也应继承同一使用权限以保持图写入路径稳定。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        "ALTER DEFAULT PRIVILEGES FOR ROLE {role} IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO {role}"
    ))
}

pub(crate) fn create_extension_sql(extension: &str) -> Result<String, Box<dyn std::error::Error>> {
    // What：生成 profile database 内允许扩展的安装 DDL。
    // Why：扩展名是动态 SQL identifier，但本流程只允许 mem012 所需的固定扩展集合。
    match extension {
        "vector" | "pg_trgm" | "age" => Ok(format!("CREATE EXTENSION IF NOT EXISTS {extension}")),
        _ => Err(format!("不支持的 PostgreSQL extension: {extension}").into()),
    }
}

pub(crate) fn create_memory_graph_sql() -> &'static str {
    // What：生成创建 AGE memory_graph 的幂等 SQL。
    // Why：memory_graph 必须由 AGE create_graph 建立，不能用普通 CREATE SCHEMA 替代。
    r#"
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph') THEN
        PERFORM ag_catalog.create_graph('memory_graph');
    END IF;
END $$;
"#
}

pub(crate) fn reassign_memory_graph_owner_sql(
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：把 create_graph 生成的 memory_graph 对象 owner 转给 profile。
    // Why：AGE 建标签/写点要求表 owner；只 GRANT DML 会在 rebuild 时报 must be owner of table _ag_label_vertex。
    let role = quoted_pg_identifier(profile)?;
    Ok(format!(
        r#"
DO $reassign$
DECLARE
    obj record;
BEGIN
    EXECUTE 'ALTER SCHEMA memory_graph OWNER TO {role}';
    FOR obj IN
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'memory_graph'
          AND c.relkind IN ('r', 'p')
    LOOP
        EXECUTE format('ALTER TABLE memory_graph.%I OWNER TO {role}', obj.relname);
    END LOOP;
    FOR obj IN
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'memory_graph'
          AND c.relkind = 'S'
    LOOP
        EXECUTE format('ALTER SEQUENCE memory_graph.%I OWNER TO {role}', obj.relname);
    END LOOP;
END
$reassign$;
"#
    ))
}

pub(crate) fn load_age_sql() -> &'static str {
    "LOAD 'age'"
}

pub(crate) fn set_age_search_path_sql() -> &'static str {
    r#"SET LOCAL search_path = ag_catalog, "$user", public"#
}

pub(crate) fn profile_database_setup_sql(
    profile: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    // What：按执行顺序生成 profile database 内的扩展、AGE graph 和权限 SQL。
    // Why：先集中固定顺序和校验边界，后续执行函数只负责事务与错误阶段，不再拼接动态 SQL。
    validate_profile_name(profile)?;
    Ok(vec![
        create_extension_sql("vector")?,
        create_extension_sql("pg_trgm")?,
        create_extension_sql("age")?,
        load_age_sql().to_string(),
        set_age_search_path_sql().to_string(),
        create_memory_graph_sql().to_string(),
        reassign_memory_graph_owner_sql(profile)?,
        grant_public_schema_usage_create_sql(profile)?,
        grant_ag_catalog_schema_usage_sql(profile)?,
        grant_agtype_usage_sql(profile)?,
        grant_ag_catalog_functions_execute_sql(profile)?,
        grant_memory_graph_schema_usage_create_sql(profile)?,
        grant_public_tables_dml_sql(profile)?,
        grant_memory_graph_tables_dml_sql(profile)?,
        grant_public_sequences_usage_sql(profile)?,
        grant_memory_graph_sequences_usage_sql(profile)?,
        grant_public_tables_default_privileges_sql(profile)?,
        grant_public_sequences_default_privileges_sql(profile)?,
        grant_memory_graph_tables_default_privileges_sql(profile)?,
        grant_memory_graph_sequences_default_privileges_sql(profile)?,
    ])
}

pub(crate) async fn apply_profile_database_setup_sql(
    pool: &sqlx::Pool<sqlx::Postgres>,
    profile: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：在已连接的 profile database 上按顺序执行 setup SQL。
    // Why：先做本地 profile 校验再开事务，避免非法输入触发数据库连接或执行半截 DDL。
    let statements = profile_database_setup_sql(profile)?;
    let mut tx = pool.begin().await?;
    for statement in statements {
        sqlx::query(&statement).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn cleanup_profile_admin_resources(
    pool: &sqlx::Pool<sqlx::Postgres>,
    profile: &str,
    database_created: bool,
    role_created: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：按本轮已创建资源执行 create_profile 失败补偿清理。
    // Why：CREATE DATABASE 不可事务化，只能用显式反向 DDL 收敛半初始化状态。
    if database_created {
        sqlx::query(&terminate_profile_database_connections_sql(profile)?)
            .execute(pool)
            .await?;
        sqlx::query(&drop_profile_database_sql(profile)?)
            .execute(pool)
            .await?;
    }
    if role_created {
        sqlx::query(&drop_profile_role_sql(profile)?)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub(crate) async fn apply_profile_admin_setup_sql(
    pool: &sqlx::Pool<sqlx::Postgres>,
    profile: &str,
    password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：在 admin 连接上创建 profile role/database，并限制 database 连接权限。
    // Why：CREATE DATABASE 不能放入事务；失败时必须用补偿清理避免留下不可用 profile。
    validate_profile_name(profile)?;
    let database_name = format!("mem_{profile}");
    let statements = [
        ("create role", create_role_sql(profile, password)?),
        ("create database", create_database_sql(profile)?),
        ("revoke public connect", revoke_public_connect_sql(profile)?),
        ("grant profile connect", grant_connect_sql(profile)?),
    ];
    let mut role_created = false;
    let mut database_created = false;
    for (stage, statement) in statements {
        if let Err(error) = sqlx::query(&statement).execute(pool).await {
            let original = format!(
                "create_profile admin step `{stage}` failed for role `{profile}` database `{database_name}`: {error}"
            );
            if role_created || database_created {
                if let Err(cleanup_error) =
                    cleanup_profile_admin_resources(pool, profile, database_created, role_created)
                        .await
                {
                    return Err(std::io::Error::other(format!(
                        "{original}; cleanup failed: {cleanup_error}"
                    ))
                    .into());
                }
            }
            return Err(std::io::Error::other(original).into());
        }
        match stage {
            "create role" => role_created = true,
            "create database" => database_created = true,
            _ => {}
        }
    }
    Ok(())
}

pub(crate) async fn initialize_profile_database_schema(
    admin_profile_pool: &sqlx::Pool<sqlx::Postgres>,
    profile_pool: &sqlx::Pool<sqlx::Postgres>,
    profile: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：初始化新 profile database 内的扩展、权限和 mem012 表结构。
    // Why：扩展和权限需要 admin 执行，mem012 表结构必须用 profile 连接创建以匹配运行期权限。
    apply_profile_database_setup_sql(admin_profile_pool, profile).await?;
    super::init_profile_memory_tables(profile_pool, profile).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        PROFILE_ADMIN_RESOURCES_SQL, apply_profile_admin_setup_sql,
        apply_profile_database_setup_sql, cleanup_profile_admin_resources, create_database_sql,
        create_extension_sql, create_memory_graph_sql, create_role_sql, drop_profile_database_sql,
        drop_profile_role_sql, ensure_profile_admin_resources_absent,
        grant_ag_catalog_functions_execute_sql, grant_ag_catalog_schema_usage_sql,
        grant_agtype_usage_sql, grant_connect_sql, grant_memory_graph_schema_usage_create_sql,
        grant_memory_graph_sequences_default_privileges_sql,
        grant_memory_graph_sequences_usage_sql, grant_memory_graph_tables_default_privileges_sql,
        grant_memory_graph_tables_dml_sql, grant_public_schema_usage_create_sql,
        grant_public_sequences_default_privileges_sql, grant_public_sequences_usage_sql,
        grant_public_tables_default_privileges_sql, grant_public_tables_dml_sql,
        initialize_profile_database_schema, load_age_sql, profile_admin_resources_conflict_error,
        profile_database_setup_sql, quoted_pg_identifier, reassign_memory_graph_owner_sql,
        revoke_public_connect_sql, set_age_search_path_sql,
        terminate_profile_database_connections_sql,
    };

    #[test]
    fn quoted_pg_identifier_quotes_valid_identifier() {
        assert_eq!(quoted_pg_identifier("rikocodex").unwrap(), "\"rikocodex\"");
        assert_eq!(
            quoted_pg_identifier("mem_rikocodex").unwrap(),
            "\"mem_rikocodex\""
        );
    }

    #[test]
    fn quoted_pg_identifier_rejects_invalid_identifier() {
        for identifier in ["", "Riko", "1riko", "riko-codex", "riko\"codex"] {
            let error = quoted_pg_identifier(identifier).unwrap_err();

            assert_eq!(
                error.to_string(),
                "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
            );
        }
    }

    #[test]
    fn create_role_sql_quotes_role_and_password_literal() {
        assert_eq!(
            create_role_sql("rikocodex", "pa'ss").unwrap(),
            "CREATE ROLE \"rikocodex\" LOGIN PASSWORD 'pa''ss'"
        );
    }

    #[test]
    fn create_role_sql_rejects_invalid_role_or_password() {
        assert!(create_role_sql("Riko", "password").is_err());
        assert_eq!(
            create_role_sql("rikocodex", "").unwrap_err().to_string(),
            "profile 密码不能为空且不能包含 NUL"
        );
    }

    #[test]
    fn create_database_sql_uses_mem_profile_database_and_owner() {
        assert_eq!(
            create_database_sql("rikocodex").unwrap(),
            "CREATE DATABASE \"mem_rikocodex\" OWNER \"rikocodex\""
        );
        assert_eq!(
            create_database_sql("share").unwrap(),
            "CREATE DATABASE \"mem_share\" OWNER \"share\""
        );
    }

    #[test]
    fn create_database_sql_rejects_invalid_profile() {
        let error = create_database_sql("Riko").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn cleanup_sql_targets_profile_database_and_role() {
        assert_eq!(
            terminate_profile_database_connections_sql("rikocodex").unwrap(),
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'mem_rikocodex' AND pid <> pg_backend_pid()"
        );
        assert_eq!(
            drop_profile_database_sql("rikocodex").unwrap(),
            "DROP DATABASE IF EXISTS \"mem_rikocodex\""
        );
        assert_eq!(
            drop_profile_role_sql("rikocodex").unwrap(),
            "DROP ROLE IF EXISTS \"rikocodex\""
        );
    }

    #[test]
    fn cleanup_sql_rejects_invalid_or_reserved_profile() {
        assert!(drop_profile_database_sql("riko-codex").is_err());
        assert_eq!(
            drop_profile_role_sql("postgres").unwrap_err().to_string(),
            "profile 名称是保留名"
        );
    }

    #[test]
    fn revoke_public_connect_sql_uses_mem_profile_database() {
        assert_eq!(
            revoke_public_connect_sql("rikocodex").unwrap(),
            "REVOKE CONNECT ON DATABASE \"mem_rikocodex\" FROM PUBLIC"
        );
    }

    #[test]
    fn revoke_public_connect_sql_rejects_invalid_profile() {
        let error = revoke_public_connect_sql("").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_connect_sql_uses_mem_profile_database_and_role() {
        assert_eq!(
            grant_connect_sql("rikocodex").unwrap(),
            "GRANT CONNECT ON DATABASE \"mem_rikocodex\" TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_connect_sql_rejects_invalid_profile() {
        let error = grant_connect_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_public_schema_usage_create_sql_grants_profile_on_public_schema() {
        assert_eq!(
            grant_public_schema_usage_create_sql("rikocodex").unwrap(),
            "GRANT USAGE, CREATE ON SCHEMA public TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_public_schema_usage_create_sql_rejects_invalid_profile() {
        let error = grant_public_schema_usage_create_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_public_tables_dml_sql_grants_profile_on_public_tables() {
        assert_eq!(
            grant_public_tables_dml_sql("rikocodex").unwrap(),
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_public_tables_dml_sql_rejects_invalid_profile() {
        let error = grant_public_tables_dml_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_public_sequences_usage_sql_grants_profile_on_public_sequences() {
        assert_eq!(
            grant_public_sequences_usage_sql("rikocodex").unwrap(),
            "GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_public_sequences_usage_sql_rejects_invalid_profile() {
        let error = grant_public_sequences_usage_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_public_tables_default_privileges_sql_grants_profile_on_future_public_tables() {
        assert_eq!(
            grant_public_tables_default_privileges_sql("rikocodex").unwrap(),
            "ALTER DEFAULT PRIVILEGES FOR ROLE \"rikocodex\" IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_public_tables_default_privileges_sql_rejects_invalid_profile() {
        let error = grant_public_tables_default_privileges_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_public_sequences_default_privileges_sql_grants_profile_on_future_public_sequences() {
        assert_eq!(
            grant_public_sequences_default_privileges_sql("rikocodex").unwrap(),
            "ALTER DEFAULT PRIVILEGES FOR ROLE \"rikocodex\" IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_public_sequences_default_privileges_sql_rejects_invalid_profile() {
        let error = grant_public_sequences_default_privileges_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_ag_catalog_schema_usage_sql_grants_profile_on_ag_catalog_schema() {
        assert_eq!(
            grant_ag_catalog_schema_usage_sql("rikocodex").unwrap(),
            "GRANT USAGE ON SCHEMA ag_catalog TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_ag_catalog_schema_usage_sql_rejects_invalid_profile() {
        let error = grant_ag_catalog_schema_usage_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_agtype_usage_sql_grants_profile_on_agtype() {
        assert_eq!(
            grant_agtype_usage_sql("rikocodex").unwrap(),
            "GRANT USAGE ON TYPE ag_catalog.agtype TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_agtype_usage_sql_rejects_invalid_profile() {
        let error = grant_agtype_usage_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_ag_catalog_functions_execute_sql_grants_profile_on_ag_catalog_functions() {
        assert_eq!(
            grant_ag_catalog_functions_execute_sql("rikocodex").unwrap(),
            "GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_ag_catalog_functions_execute_sql_rejects_invalid_profile() {
        let error = grant_ag_catalog_functions_execute_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_memory_graph_schema_usage_create_sql_grants_profile_on_memory_graph_schema() {
        assert_eq!(
            grant_memory_graph_schema_usage_create_sql("rikocodex").unwrap(),
            "GRANT USAGE, CREATE ON SCHEMA memory_graph TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_memory_graph_schema_usage_create_sql_rejects_invalid_profile() {
        let error = grant_memory_graph_schema_usage_create_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_memory_graph_tables_dml_sql_grants_profile_on_memory_graph_tables() {
        assert_eq!(
            grant_memory_graph_tables_dml_sql("rikocodex").unwrap(),
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_memory_graph_tables_dml_sql_rejects_invalid_profile() {
        let error = grant_memory_graph_tables_dml_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_memory_graph_sequences_usage_sql_grants_profile_on_memory_graph_sequences() {
        assert_eq!(
            grant_memory_graph_sequences_usage_sql("rikocodex").unwrap(),
            "GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_memory_graph_sequences_usage_sql_rejects_invalid_profile() {
        let error = grant_memory_graph_sequences_usage_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_memory_graph_tables_default_privileges_sql_grants_profile_on_future_memory_graph_tables()
     {
        assert_eq!(
            grant_memory_graph_tables_default_privileges_sql("rikocodex").unwrap(),
            "ALTER DEFAULT PRIVILEGES FOR ROLE \"rikocodex\" IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_memory_graph_tables_default_privileges_sql_rejects_invalid_profile() {
        let error = grant_memory_graph_tables_default_privileges_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn grant_memory_graph_sequences_default_privileges_sql_grants_profile_on_future_memory_graph_sequences()
     {
        assert_eq!(
            grant_memory_graph_sequences_default_privileges_sql("rikocodex").unwrap(),
            "ALTER DEFAULT PRIVILEGES FOR ROLE \"rikocodex\" IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO \"rikocodex\""
        );
    }

    #[test]
    fn grant_memory_graph_sequences_default_privileges_sql_rejects_invalid_profile() {
        let error = grant_memory_graph_sequences_default_privileges_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn create_extension_sql_accepts_required_extensions() {
        assert_eq!(
            create_extension_sql("vector").unwrap(),
            "CREATE EXTENSION IF NOT EXISTS vector"
        );
        assert_eq!(
            create_extension_sql("pg_trgm").unwrap(),
            "CREATE EXTENSION IF NOT EXISTS pg_trgm"
        );
        assert_eq!(
            create_extension_sql("age").unwrap(),
            "CREATE EXTENSION IF NOT EXISTS age"
        );
    }

    #[test]
    fn create_extension_sql_rejects_unknown_extension() {
        let error = create_extension_sql("hstore").unwrap_err();

        assert_eq!(error.to_string(), "不支持的 PostgreSQL extension: hstore");
    }

    #[test]
    fn create_memory_graph_sql_uses_age_create_graph_guarded_by_namespace_check() {
        let sql = create_memory_graph_sql();

        assert!(sql.contains("nspname = 'memory_graph'"));
        assert!(sql.contains("PERFORM ag_catalog.create_graph('memory_graph')"));
        assert!(!sql.contains("CREATE SCHEMA"));
    }

    #[test]
    fn reassign_memory_graph_owner_sql_transfers_schema_and_objects_to_profile() {
        let sql = reassign_memory_graph_owner_sql("rikocodex").unwrap();

        assert!(sql.contains("ALTER SCHEMA memory_graph OWNER TO \"rikocodex\""));
        assert!(sql.contains("ALTER TABLE memory_graph.%I OWNER TO \"rikocodex\""));
        assert!(sql.contains("ALTER SEQUENCE memory_graph.%I OWNER TO \"rikocodex\""));
        assert!(sql.contains("nspname = 'memory_graph'"));
        assert!(sql.contains("AND c.relkind IN ('r', 'p')"));
        assert!(sql.contains("AND c.relkind = 'S'"));
        assert!(sql.find("AND c.relkind IN ('r', 'p')") < sql.find("AND c.relkind = 'S'"));
    }

    #[test]
    fn reassign_memory_graph_owner_sql_rejects_invalid_profile() {
        let error = reassign_memory_graph_owner_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[test]
    fn load_age_sql_matches_existing_age_load_statement() {
        assert_eq!(load_age_sql(), "LOAD 'age'");
    }

    #[test]
    fn set_age_search_path_sql_matches_existing_search_path_statement() {
        assert_eq!(
            set_age_search_path_sql(),
            r#"SET LOCAL search_path = ag_catalog, "$user", public"#
        );
    }

    #[test]
    fn profile_database_setup_sql_orders_extension_age_and_permission_sql() {
        let sql = profile_database_setup_sql("rikocodex").unwrap();

        assert_eq!(sql.len(), 20);
        assert_eq!(sql[0], "CREATE EXTENSION IF NOT EXISTS vector");
        assert_eq!(sql[1], "CREATE EXTENSION IF NOT EXISTS pg_trgm");
        assert_eq!(sql[2], "CREATE EXTENSION IF NOT EXISTS age");
        assert_eq!(sql[3], "LOAD 'age'");
        assert_eq!(
            sql[4],
            r#"SET LOCAL search_path = ag_catalog, "$user", public"#
        );
        assert!(sql[5].contains("PERFORM ag_catalog.create_graph('memory_graph')"));
        assert!(sql[6].contains("ALTER SCHEMA memory_graph OWNER TO \"rikocodex\""));
        assert_eq!(
            sql[19],
            "ALTER DEFAULT PRIVILEGES FOR ROLE \"rikocodex\" IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO \"rikocodex\""
        );
        assert!(sql.iter().all(|statement| !statement.contains("uutest")));
    }

    #[test]
    fn profile_database_setup_sql_rejects_invalid_profile() {
        let error = profile_database_setup_sql("riko-codex").unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );

        let error = profile_database_setup_sql("abcdefghijklmnopqrstuvwxyzabcde").unwrap_err();

        assert_eq!(error.to_string(), "profile 名称长度不能超过 30");
    }

    #[test]
    fn profile_database_setup_sql_rejects_reserved_profile() {
        let error = profile_database_setup_sql("postgres").unwrap_err();

        assert_eq!(error.to_string(), "profile 名称是保留名");
    }

    #[test]
    fn profile_database_setup_sql_accepts_share_profile() {
        let sql = profile_database_setup_sql("share").unwrap();

        assert!(sql.iter().any(|statement| statement.contains("\"share\"")));
    }

    #[test]
    fn admin_resources_precheck_sql_uses_bound_parameters() {
        assert!(PROFILE_ADMIN_RESOURCES_SQL.contains("rolname = $1"));
        assert!(PROFILE_ADMIN_RESOURCES_SQL.contains("datname = $2"));
        assert!(!PROFILE_ADMIN_RESOURCES_SQL.contains("rikocodex"));
        assert!(!PROFILE_ADMIN_RESOURCES_SQL.contains("mem_"));
    }

    #[test]
    fn profile_admin_resources_conflict_error_names_existing_resources() {
        assert_eq!(
            profile_admin_resources_conflict_error("rikocodex", true, false).unwrap(),
            "远端资源已存在: role `rikocodex`"
        );
        assert_eq!(
            profile_admin_resources_conflict_error("rikocodex", false, true).unwrap(),
            "远端资源已存在: database `mem_rikocodex`"
        );
        assert_eq!(
            profile_admin_resources_conflict_error("rikocodex", true, true).unwrap(),
            "远端资源已存在: role `rikocodex`, database `mem_rikocodex`"
        );
        assert!(profile_admin_resources_conflict_error("rikocodex", false, false).is_none());
    }

    #[tokio::test]
    async fn ensure_profile_admin_resources_absent_rejects_invalid_profile_before_querying() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
            .unwrap();

        let error = ensure_profile_admin_resources_absent(&pool, "riko-codex")
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[tokio::test]
    async fn apply_profile_database_setup_sql_rejects_invalid_profile_before_connecting() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
            .unwrap();

        let error = apply_profile_database_setup_sql(&pool, "riko-codex")
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[tokio::test]
    async fn apply_profile_admin_setup_sql_rejects_invalid_profile_before_connecting() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
            .unwrap();

        let error = apply_profile_admin_setup_sql(&pool, "riko-codex", "password")
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[tokio::test]
    async fn apply_profile_admin_setup_sql_rejects_reserved_profile_before_connecting() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
            .unwrap();

        let error = apply_profile_admin_setup_sql(&pool, "postgres", "password")
            .await
            .unwrap_err();

        assert_eq!(error.to_string(), "profile 名称是保留名");
    }

    #[tokio::test]
    async fn cleanup_profile_admin_resources_rejects_invalid_profile_before_connecting() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
            .unwrap();

        let error = cleanup_profile_admin_resources(&pool, "riko-codex", true, true)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }

    #[tokio::test]
    async fn initialize_profile_database_schema_rejects_invalid_profile_before_connecting() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
            .unwrap();

        let error = initialize_profile_database_schema(&pool, &pool, "riko-codex")
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "PostgreSQL identifier 必须匹配 [a-z][a-z0-9_]*"
        );
    }
}
