use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateMemoryArgs {
    category: Option<String>,
    title: String,
    content: String,
    summary: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
    recall_when: Option<String>,
}

pub async fn run(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：create_memory 独立成文件，后续字段校验和写入 memory_changes 不污染工具路由层。
    let create_args = serde_json::from_value::<CreateMemoryArgs>(args.clone())?;
    validate_create_memory_args(&create_args, context.profile, context.category_index_list)?;
    if is_init_create(&create_args, context.profile) {
        consume_init_auth_grant(context.api_base_url).await?;
    }
    let title_norm: String = sqlx::query_scalar("SELECT normalize_title($1)")
        .bind(&create_args.title)
        .fetch_one(context.profile_pool)
        .await?;
    let memory_uuid: String = sqlx::query_scalar("SELECT gen_random_uuid()::text")
        .fetch_one(context.profile_pool)
        .await?;

    // state
    let after_state = build_after_state(&create_args, &title_norm, &memory_uuid, context.profile)?;
    reject_duplicate_memory(context.profile_pool, &after_state).await?;

    // database writes
    create_memory_transaction(context.profile_pool, &memory_uuid, &after_state).await?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "create_memory",
            "data": {
                "memory_uuid": memory_uuid,
                "result": "pending"
            },
            "error": null,
            "profile": context.profile
        })
    );

    Ok(())
}

fn build_after_state(
    args: &CreateMemoryArgs,
    title_norm: &str,
    memory_uuid: &str,
    profile: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Why：after_state 是当前工作态快照，二次确认和回滚都必须基于同一份完整状态。
    let normalize_text = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let category = if profile == "share" {
        "share".to_string()
    } else {
        args.category
            .as_deref()
            .map(normalize_text)
            .unwrap_or_else(|| "core".to_string())
    };
    let mut keywords = args
        .keywords
        .iter()
        .map(|keyword| serde_json::json!({ "keyword_norm": normalize_text(keyword), "weight": null }))
        .collect::<Vec<_>>();
    if category == "init"
        && !keywords
            .iter()
            .any(|keyword| keyword["keyword_norm"].as_str() == Some("init"))
    {
        keywords.push(serde_json::json!({ "keyword_norm": "init", "weight": null }));
    }
    Ok(serde_json::json!({
        "memory": {
            "uuid": memory_uuid,
            "category": category,
            "title_norm": title_norm,
            "content": args.content.trim(),
            "summary": args.summary.as_deref().map(str::trim).filter(|text| !text.is_empty()),
            "status": "pending",
            "recall_when": args.recall_when.as_deref().map(str::trim),
            "trashed_at": null
        },
        "keywords": keywords,
        "relations": []
    }))
}

async fn reject_duplicate_memory(
    pool: &sqlx::Pool<sqlx::Postgres>,
    after_state: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：memory_units 已包含待确认工作态，重复检测只需要检查当前可用记忆。
    let get_text = |key: &str| {
        after_state["memory"][key]
            .as_str()
            .ok_or("after_state.memory 字段缺失")
    };
    let title_norm = get_text("title_norm")?;
    let content = get_text("content")?;
    let summary = after_state["memory"]["summary"].as_str();
    let duplicate_message: Option<String> = sqlx::query_scalar(
        r#"
        SELECT CASE
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE title_norm = $1) THEN 'DUPLICATE_TITLE: title_norm 已存在'
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE content = $2) THEN 'DUPLICATE_CONTENT: content 已存在'
            WHEN $3::text IS NOT NULL AND EXISTS (SELECT 1 FROM memory_units WHERE summary = $3) THEN 'DUPLICATE_SUMMARY: summary 已存在'
        END
        "#,
    )
    .bind(title_norm)
    .bind(content)
    .bind(summary)
    .fetch_one(pool)
    .await?;

    if let Some(message) = duplicate_message {
        return Err(message.into());
    }

    Ok(())
}

async fn create_memory_transaction(
    pool: &sqlx::Pool<sqlx::Postgres>,
    memory_uuid: &str,
    after_state: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    // Why：create 要同时写工作态和回滚记录，先建立事务边界避免后续出现半写入状态。
    let mut tx = pool.begin().await?;
    insert_memory_unit(&mut tx, memory_uuid, after_state).await?;
    insert_memory_keywords(&mut tx, memory_uuid, after_state).await?;
    crate::psql::search_index::refresh_memory_search_index(&mut tx, memory_uuid).await?;
    insert_memory_relations(&mut tx, after_state).await?;
    sqlx::query(
        r#"
        INSERT INTO memory_changes (
            uuid, memory_uuid, action, before_state, after_state, created_at, updated_at
        )
        VALUES (
            $1::uuid, $1::uuid, 'create', NULL, $2::jsonb, now(), now()
        )
        "#,
    )
    .bind(memory_uuid)
    .bind(after_state.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(())
}

async fn insert_memory_unit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    after_state: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    // Why：memory_units 是 Agent 可回读的工作态，create 不能只留下待确认变更。
    sqlx::query(
        r#"
        INSERT INTO memory_units (
            uuid, category, title_norm, content, summary, status,
            recall_when, trashed_at, created_at, updated_at
        )
        SELECT
            $1::uuid,
            state #>> '{memory,category}',
            state #>> '{memory,title_norm}',
            state #>> '{memory,content}',
            state #>> '{memory,summary}',
            state #>> '{memory,status}',
            state #>> '{memory,recall_when}',
            (state #>> '{memory,trashed_at}')::timestamptz,
            now(),
            now()
        FROM (SELECT $2::jsonb AS state) input
        "#,
    )
    .bind(memory_uuid)
    .bind(after_state.to_string())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn insert_memory_keywords(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    after_state: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    // Why：keywords 是检索入口的一部分，必须和 memory_units 在同一事务内保持一致。
    sqlx::query(
        r#"
        INSERT INTO memory_keywords (uuid, memory_uuid, keyword_norm, weight, created_at)
        SELECT
            gen_random_uuid(),
            $1::uuid,
            keyword ->> 'keyword_norm',
            (keyword ->> 'weight')::int,
            now()
        FROM jsonb_array_elements($2::jsonb -> 'keywords') AS keywords(keyword)
        "#,
    )
    .bind(memory_uuid)
    .bind(after_state.to_string())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn insert_memory_relations(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    after_state: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    // Why：memory_relations 是 AGE 图的 SQL 源，必须和工作态记忆在同一事务内落库。
    sqlx::query(
        r#"
        INSERT INTO memory_relations (
            uuid, from_memory_uuid, to_memory_uuid, relation_type, weight, note, created_at
        )
        SELECT
            gen_random_uuid(),
            (relation ->> 'from_memory_uuid')::uuid,
            (relation ->> 'to_memory_uuid')::uuid,
            relation ->> 'relation_type',
            (relation ->> 'weight')::int,
            NULLIF(relation ->> 'note', ''),
            now()
        FROM jsonb_array_elements($1::jsonb -> 'relations') AS relations(relation)
        ON CONFLICT (from_memory_uuid, to_memory_uuid, relation_type) DO NOTHING
        "#,
    )
    .bind(after_state.to_string())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

fn validate_create_memory_args(
    args: &CreateMemoryArgs,
    profile: &str,
    category_index_list: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：serde 只保证类型正确，业务入口还必须拒绝空值和会破坏分类边界的参数。
    for (name, value) in [("title", &args.title), ("content", &args.content)] {
        if value.trim().is_empty() {
            return Err(format!("{name} 不能为空").into());
        }
    }

    validate_create_category(args.category.as_deref(), profile, category_index_list)?;

    if (!is_init_create(args, profile) && args.keywords.is_empty())
        || args.keywords.iter().any(|item| item.trim().is_empty())
    {
        return Err("keywords 必须是非空字符串数组".into());
    }
    let mut keyword_set = HashSet::new();
    for keyword in &args.keywords {
        let keyword_norm = keyword
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !keyword_set.insert(keyword_norm) {
            return Err("keywords 规范化后不能重复".into());
        }
    }

    if args
        .recall_when
        .as_deref()
        .is_some_and(|text| text.trim().is_empty())
    {
        return Err("recall_when 不能是空字符串".into());
    }

    Ok(())
}

fn validate_create_category(
    category: Option<&str>,
    profile: &str,
    category_index_list: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    // What：校验 create_memory 的 category 与目标 profile 是否一致。
    // Why：数据库路由只由 --profile 决定，category 只能表达目标库允许的分类值。
    if profile == "share" {
        return match category {
            Some("share") => Ok(()),
            Some(_) => Err("profile share 的 category 只能是 share".into()),
            None => Err("profile share must have category share".into()),
        };
    }

    let category = category.unwrap_or("core");
    let category_slug = category
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_lowercase)
        && category
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if category == "share" || !category_slug {
        return Err("category 必须是非 share 的 slug".into());
    }
    if category_index_list.is_empty() {
        return Err("categories.index_list 不能为空".into());
    }
    if !category_index_list
        .iter()
        .any(|allowed| allowed == category)
    {
        return Err(format!(
            "category 不在 categories.index_list 中: {category}；category_list: {}",
            category_index_list.join(", ")
        )
        .into());
    }
    Ok(())
}

fn is_init_create(args: &CreateMemoryArgs, profile: &str) -> bool {
    profile != "share" && args.category.as_deref().unwrap_or("core") == "init"
}

fn init_auth_file_path() -> Result<std::path::PathBuf, String> {
    crate::init_auth_file_path().map_err(|_| init_auth_file_help())
}

fn read_init_auth_file(
    token_path: &std::path::Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let token = std::fs::read_to_string(token_path).map_err(|_| init_auth_file_help())?;
    if token.trim().is_empty() {
        return Err("auth file 为空；请向用户申请授权后重试".into());
    }
    serde_json::from_str(&token)
        .map_err(|_| "auth file 不是合法 JSON；请向用户申请授权后重试".into())
}

async fn consume_init_auth_grant(api_base_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let token_path = init_auth_file_path()?;
    consume_init_auth_grant_file(api_base_url, &token_path).await
}

async fn consume_init_auth_grant_file(
    api_base_url: &str,
    token_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let grant = match read_init_auth_file(token_path) {
        Ok(grant) => grant,
        Err(error) => {
            super::auth::remove_init_auth_file(token_path)?;
            return Err(error);
        }
    };
    let consume_result = post_init_auth_grant_consume(api_base_url, &grant).await;
    let remove_result = super::auth::remove_init_auth_file(token_path);
    match (consume_result, remove_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(remove_error)) => {
            Err(format!("已消费 init grant，但删除 auth file 失败: {remove_error}").into())
        }
        (Err(error), Err(remove_error)) => {
            Err(format!("{error}; 同时删除 auth file 失败: {remove_error}").into())
        }
    }
}

async fn post_init_auth_grant_consume(
    api_base_url: &str,
    grant: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/auth/grant/consume",
            api_base_url.trim_end_matches('/')
        ))
        .json(grant)
        .send()
        .await?;
    let status = response.status();
    let body = response.json::<serde_json::Value>().await?;
    if !status.is_success() {
        let message = body
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| status.to_string());
        return Err(format!("消费 init grant 失败: {message}").into());
    }
    if body
        .pointer("/data/consumed")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err("消费 init grant 响应缺少 consumed=true".into());
    }
    Ok(())
}

fn init_auth_file_help() -> String {
    "写入 category=init 需要 auth file: ~/.auth/auth_file.mem；请向用户申请授权后重试".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        CreateMemoryArgs, build_after_state, consume_init_auth_grant_file, validate_create_category,
    };

    #[test]
    fn validate_create_category_rejects_unknown_category() {
        let allowed = vec!["core".to_string(), "book".to_string()];

        assert!(validate_create_category(Some("book"), "riko", &allowed).is_ok());
        let error = validate_create_category(Some("unknown"), "riko", &allowed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown"));
        assert!(error.contains("category_list: core, book"));
    }

    #[test]
    fn validate_create_category_requires_configured_init_category() {
        let allowed = vec!["core".to_string(), "init".to_string()];
        let no_init = vec!["core".to_string()];

        let missing_category = validate_create_category(Some("init"), "riko", &no_init)
            .unwrap_err()
            .to_string();
        assert!(missing_category.contains("category_list: core"));

        assert!(validate_create_category(Some("init"), "riko", &allowed).is_ok());
    }

    #[test]
    fn build_after_state_adds_init_keyword_for_init_category() {
        let args = serde_json::from_value::<CreateMemoryArgs>(serde_json::json!({
            "category": "init",
            "title": "标题",
            "content": "正文"
        }))
        .unwrap();

        let state = build_after_state(&args, "标题", "memory-uuid", "riko").unwrap();
        let keywords = state["keywords"].as_array().unwrap();
        assert!(
            keywords
                .iter()
                .any(|keyword| keyword["keyword_norm"].as_str() == Some("init"))
        );
    }

    #[tokio::test]
    async fn consume_init_auth_grant_file_removes_invalid_json_file() {
        let root = std::env::temp_dir().join(format!(
            "mem012_invalid_auth_file_test_{}",
            std::process::id()
        ));
        let path = root.join("auth_file.mem");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, "not-json").unwrap();

        let error = consume_init_auth_grant_file("http://127.0.0.1:37777", &path)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("auth file 不是合法 JSON"));
        assert!(!path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
