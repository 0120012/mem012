use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateMemoryArgs {
    category: Option<String>,
    title: String,
    content: String,
    summary: String,
    keywords: Vec<String>,
    recall_when: Option<String>,
    exclude_when: Option<String>,
    handles: Option<Vec<String>>,
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

    let after_state = build_after_state(&create_args, &title_norm, &memory_uuid)?;
    reject_duplicate_memory(context.profile_pool, &after_state).await?;
    insert_memory_change(context.profile_pool, &memory_uuid, &after_state).await?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "create_memory",
            "data": {
                "memory_uuid": memory_uuid,
                "result": "pending_review"
            },
            "error": null,
            "meta": {
                "spec_version": "v10",
                "profile": context.profile
            }
        })
    );

    Ok(())
}

fn build_after_state(
    args: &CreateMemoryArgs,
    title_norm: &str,
    memory_uuid: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Why：after_state 是审查表和正式表之间的合同，先独立成函数避免写库逻辑混入状态组装。
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
        .handles
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|handle| {
            let handle_norm = handle
                .split('/')
                .map(normalize_text)
                .collect::<Vec<_>>()
                .join("/");
            serde_json::json!({ "handle_norm": handle_norm })
        })
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "memory": {
            "uuid": memory_uuid,
            "category": category,
            "title_norm": title_norm,
            "content": args.content.trim(),
            "summary": args.summary.trim(),
            "status": "active",
            "recall_when": args.recall_when.as_deref().map(str::trim),
            "exclude_when": args.exclude_when.as_deref().map(str::trim),
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
    // Why：create 只写 pending 提案，重复检测必须同时覆盖正式表和待审查提案。
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
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE title_norm = $1) OR EXISTS (SELECT 1 FROM memory_changes WHERE after_state #>> '{memory,title_norm}' = $1) THEN 'DUPLICATE_TITLE: title_norm 已存在或已有待审查提案'
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE content = $2) OR EXISTS (SELECT 1 FROM memory_changes WHERE after_state #>> '{memory,content}' = $2) THEN 'DUPLICATE_CONTENT: content 已存在或已有待审查提案'
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE summary = $3) OR EXISTS (SELECT 1 FROM memory_changes WHERE after_state #>> '{memory,summary}' = $3) THEN 'DUPLICATE_SUMMARY: summary 已存在或已有待审查提案'
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

async fn insert_memory_change(
    pool: &sqlx::Pool<sqlx::Postgres>,
    memory_uuid: &str,
    after_state: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    // Why：create_memory 只创建待审查提案，正式表写入必须留给 review 确认阶段。
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
    .execute(pool)
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

    for (name, value) in [
        ("recall_when", &args.recall_when),
        ("exclude_when", &args.exclude_when),
    ] {
        if value.as_deref().is_some_and(|text| text.trim().is_empty()) {
            return Err(format!("{name} 不能是空字符串").into());
        }
    }

    if let Some(handles) = &args.handles {
        if handles.is_empty() {
            return Err("handles 如果提供，不能为空数组".into());
        }
        for handle in handles {
            let handle = handle.trim();
            if handle.is_empty()
                || handle.starts_with('/')
                || handle.ends_with('/')
                || handle.contains("//")
                || handle.split('/').next() != Some(category)
            {
                return Err("handles 必须是非空路径，且第一段等于 category".into());
            }
        }
    }

    Ok(())
}
