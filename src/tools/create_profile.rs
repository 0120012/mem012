pub(crate) async fn run(
    config: &crate::config::Config,
    profile: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：执行 --create_profile 的数据库创建、schema 初始化和配置写入。
    // Why：配置只在数据库和 schema 都成功后落盘，避免 CLI 指向半初始化 profile。
    if config.database_url(profile).is_some() {
        return Err(format!("profile 已存在于 [database]: {profile}").into());
    }
    let admin_database_url = crate::config::admin_database_url_from_env_value(std::env::var_os(
        "MEM012_ADMIN_DATABASE_URL",
    ))?;
    let admin_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_database_url)
        .await?;
    crate::psql::ensure_profile_admin_resources_absent(&admin_pool, profile).await?;
    let password = crate::config::generate_profile_password();
    let profile_database_url =
        crate::config::derive_profile_database_url(&admin_database_url, profile, &password)?;
    let admin_profile_database_url =
        crate::config::derive_admin_profile_database_url(&admin_database_url, profile)?;
    let config_path = crate::config::config_path("config.toml");
    let updated_config = crate::config::append_database_profile_text(
        &std::fs::read_to_string(&config_path)?,
        profile,
        &profile_database_url,
    )?;
    crate::psql::apply_profile_admin_setup_sql(&admin_pool, profile, &password).await?;
    let finish_result: Result<(), Box<dyn std::error::Error>> = async {
        let admin_profile_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_profile_database_url)
            .await?;
        let profile_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&profile_database_url)
            .await?;
        crate::psql::initialize_profile_database_schema(
            &admin_profile_pool,
            &profile_pool,
            profile,
        )
        .await?;
        crate::config::write_config_text_atomic(&config_path, &updated_config)?;
        Ok(())
    }
    .await;
    if let Err(error) = finish_result {
        if let Err(cleanup_error) =
            crate::psql::cleanup_profile_admin_resources(&admin_pool, profile, true, true).await
        {
            return Err(std::io::Error::other(format!(
                "create_profile failed after admin setup: {error}; cleanup failed: {cleanup_error}"
            ))
            .into());
        }
        return Err(std::io::Error::other(format!(
            "create_profile failed after admin setup and cleanup succeeded: {error}"
        ))
        .into());
    }
    let response = serde_json::json!({
        "state": "success",
        "tool": "create_profile",
        "data": { "profile": profile, "database": format!("mem_{profile}"), "config_path": config_path },
        "error": null
    });
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}
