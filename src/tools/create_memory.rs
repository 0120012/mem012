use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateMemoryArgs {
    category: Option<String>,
    title: String,
    content: String,
    summary: String,
    keywords: Vec<String>,
    recall_when: Option<String>,
    handle: Option<String>,
}

pub async fn run(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：create_memory 独立成文件，后续字段校验和写入 memory_changes 不污染工具路由层。
    let create_args = serde_json::from_value::<CreateMemoryArgs>(args.clone())?;
    validate_create_memory_args(&create_args)?;
    let title_norm: String = sqlx::query_scalar("SELECT normalize_title($1)")
        .bind(&create_args.title)
        .fetch_one(context.profile_pool)
        .await?;
    let memory_uuid: String = sqlx::query_scalar("SELECT gen_random_uuid()::text")
        .fetch_one(context.profile_pool)
        .await?;

    // state
    let after_state = build_after_state(&create_args, &title_norm, &memory_uuid)?;
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
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Why：after_state 是当前工作态快照，二次确认和回滚都必须基于同一份完整状态。
    let normalize_text = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let category = args
        .category
        .as_deref()
        .map(normalize_text)
        .unwrap_or_else(|| "core".to_string());
    let keywords = args
        .keywords
        .iter()
        .map(|keyword| serde_json::json!({ "keyword_norm": normalize_text(keyword), "weight": null }))
        .collect::<Vec<_>>();
    let handles = args
        .handle
        .as_deref()
        .map(|handle| {
            let handle_norm = handle
                .split('/')
                .map(normalize_text)
                .collect::<Vec<_>>()
                .join("/");
            serde_json::json!({ "handle_norm": handle_norm })
        })
        .into_iter()
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "memory": {
            "uuid": memory_uuid,
            "category": category,
            "title_norm": title_norm,
            "content": args.content.trim(),
            "summary": args.summary.trim(),
            "status": "pending",
            "recall_when": args.recall_when.as_deref().map(str::trim),
            "trashed_at": null
        },
        "keywords": keywords,
        "handles": handles,
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
    let summary = get_text("summary")?;
    let duplicate_message: Option<String> = sqlx::query_scalar(
        r#"
        SELECT CASE
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE title_norm = $1) THEN 'DUPLICATE_TITLE: title_norm 已存在'
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE content = $2) THEN 'DUPLICATE_CONTENT: content 已存在'
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE summary = $3) THEN 'DUPLICATE_SUMMARY: summary 已存在'
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
    insert_memory_handles(&mut tx, memory_uuid, after_state).await?;
    insert_memory_relations(&mut tx, after_state).await?;
    sqlx::query(
        r#"
        INSERT INTO memory_changes (
            uuid, memory_uuid, action, before_state, after_state, created_at, updated_at
        )
        VALUES (
            gen_random_uuid(), $1::uuid, 'create', NULL, $2::jsonb, now(), now()
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

async fn insert_memory_handles(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    after_state: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    // Why：handle 是人工定位入口，必须和 memory_units 同事务提交，避免出现悬空路径。
    sqlx::query(
        r#"
        INSERT INTO memory_handles (uuid, memory_uuid, handle_norm, created_at)
        SELECT
            gen_random_uuid(),
            $1::uuid,
            handle ->> 'handle_norm',
            now()
        FROM jsonb_array_elements($2::jsonb -> 'handles') AS handles(handle)
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

fn validate_create_memory_args(args: &CreateMemoryArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Why：serde 只保证类型正确，业务入口还必须拒绝空值和会破坏定位规则的参数。
    for (name, value) in [
        ("title", &args.title),
        ("content", &args.content),
        ("summary", &args.summary),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{name} 不能为空").into());
        }
    }

    let category = args.category.as_deref().unwrap_or("core");
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

    if args.keywords.is_empty() || args.keywords.iter().any(|item| item.trim().is_empty()) {
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

    if let Some(handle) = &args.handle {
        let handle = handle.trim();
        let segments = handle.split('/').collect::<Vec<_>>();
        if handle.is_empty()
            || handle.starts_with('/')
            || handle.ends_with('/')
            || handle.contains("//")
            || !(2..=4).contains(&segments.len())
            || segments.iter().any(|segment| segment.trim().is_empty())
            || segments.first().copied() != Some(category)
        {
            return Err("handle 必须是 2 到 4 段路径，且第一段等于 category".into());
        }
    }

    Ok(())
}
