// Why：update_memory 的工具边界先独立成模块，后续实现事务时不会继续膨胀路由层。
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadMemoryHashArgs {
    memory_uuid: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateMemoryReplaceArgs {
    memory_uuid: String,
    expected_title_hash: Option<String>,
    expected_summary_hash: Option<String>,
    expected_recall_when_hash: Option<String>,
    expected_category_hash: Option<String>,
    expected_content_hash: Option<String>,
    new_title: Option<String>,
    new_summary: Option<Option<String>>,
    new_recall_when: Option<Option<String>>,
    new_category: Option<String>,
    new_content: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateMemoryPatchContentArgs {
    memory_uuid: String,
    expected_content_hash: String,
    match_content: String,
    replace_content: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateMemoryAppendArgs {
    memory_uuid: String,
    expected_content_hash: Option<String>,
    expected_recall_when_hash: Option<String>,
    append_content: Option<String>,
    append_recall_when: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateMemoryKeywordsArgs {
    memory_uuid: String,
    expected_keywords_hash: String,
    keywords: Vec<String>,
}

pub async fn run(
    context: &super::ToolContext<'_>,
    tool: &str,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：同一组 update 工具共享入口，先固定参数外壳，具体数据库动作后续逐个补。
    match tool {
        "read_memory_hash" => read_memory_hash(context, args).await,
        "update_memory_replace" => update_memory_replace(context, args).await,
        "update_memory_patch_content" => update_memory_patch_content(context, args).await,
        "update_memory_append" => update_memory_append(context, args).await,
        "update_memory_add_keywords" => update_memory_add_keywords(context, args).await,
        "update_memory_remove_keywords" => update_memory_remove_keywords(context, args).await,
        _ => Err(format!("未知 update 工具: {tool}").into()),
    }
}

async fn read_memory_hash(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：hash 读取是所有 update 的前置步骤，先单独固定入口避免更新工具直接信任列表结果。
    // 1. 解析并校验 memory_uuid 不能为空。
    // 2. 查询 memory_units，确认目标记忆存在且未进入 trashed。
    // 3. 查询 memory_keywords，按规范化顺序组装关键词列表。
    // 4. 构建稳定的 memory state，用于计算 state_hash。
    // 5. 分别计算 title/content/summary/recall_when/category/keywords hash。
    // 6. 输出 title_norm 和 data.hash，供后续 update 工具原样携带。
    let read_args = serde_json::from_value::<ReadMemoryHashArgs>(args.clone())?;
    let memory_uuid = validate_required_text("memory_uuid", &read_args.memory_uuid)?;
    let mut tx = context.profile_pool.begin().await?;
    let state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    tx.commit().await?;
    let state = serde_json::from_str::<serde_json::Value>(&state_text)?;
    let memory = state
        .get("memory")
        .and_then(serde_json::Value::as_object)
        .ok_or("memory state 缺少 memory")?;
    let status = read_memory_text(memory, "status")?;
    if status == "trashed" {
        return Err("read_memory_hash 不支持 trashed memory".into());
    }

    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "read_memory_hash",
            "data": {
                "memory_uuid": memory_uuid,
                "title_norm": read_memory_text(memory, "title_norm")?,
                "hash": build_memory_hashes(&state)?,
            },
            "error": null,
            "profile": context.profile
        })
    );
    Ok(())
}

fn build_memory_hashes(
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Why：所有字段 hash 必须来自同一份稳定 state，避免 update 前置校验出现跨版本拼接。
    let memory = state.get("memory").ok_or("memory state 缺少 memory")?;
    Ok(serde_json::json!({
        "state_hash": sha256_json(state)?,
        "title_hash": sha256_json(&memory["title_norm"])?,
        "content_hash": sha256_json(&memory["content"])?,
        "summary_hash": sha256_json(&memory["summary"])?,
        "recall_when_hash": sha256_json(&memory["recall_when"])?,
        "category_hash": sha256_json(&memory["category"])?,
        "keywords_hash": sha256_json(&state["keywords"])?,
    }))
}

fn sha256_json(value: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    // Why：hash 输入使用 JSON 表示，才能区分 null、空字符串、数组和普通字符串。
    let text = serde_json::to_string(value)?;
    Ok(format!("0x{:x}", Sha256::digest(text.as_bytes())))
}

fn read_memory_text<'a>(
    memory: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    // Why：read_memory_hash 是后续写入的信任基线，缺少核心字段必须立即失败。
    memory
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("memory state 字段缺失或不是字符串: {key}").into())
}

fn validate_required_text<'a>(
    name: &str,
    value: &'a str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    // Why：空字符串会让数据库 uuid 报错变得不清晰，工具入口先给出明确错误。
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{name} 不能为空").into());
    }
    Ok(value)
}

fn validate_replace_args(args: &UpdateMemoryReplaceArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Why：replace 接受多字段组合，入口需要先挡住缺 hash 的不可信写入请求。
    validate_required_text("memory_uuid", &args.memory_uuid)?;
    let updates = [
        validate_required_replace_field(
            "title",
            args.new_title.as_deref(),
            args.expected_title_hash.as_deref(),
        )?,
        validate_required_replace_field(
            "category",
            args.new_category.as_deref(),
            args.expected_category_hash.as_deref(),
        )?,
        validate_required_replace_field(
            "content",
            args.new_content.as_deref(),
            args.expected_content_hash.as_deref(),
        )?,
        validate_nullable_replace_field(
            "summary",
            &args.new_summary,
            args.expected_summary_hash.as_deref(),
        )?,
        validate_nullable_replace_field(
            "recall_when",
            &args.new_recall_when,
            args.expected_recall_when_hash.as_deref(),
        )?,
    ];
    if !updates.into_iter().any(|updated| updated) {
        return Err("至少需要一个 new_* 字段".into());
    }
    Ok(())
}

fn validate_required_replace_field(
    field: &str,
    value: Option<&str>,
    hash: Option<&str>,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Why：必填型字段不能用 null 表示清空，出现 new_* 时必须同时带字段 hash。
    let Some(value) = value else {
        return Ok(false);
    };
    validate_required_text(&format!("new_{field}"), value)?;
    validate_expected_hash(field, hash)?;
    Ok(true)
}

fn validate_nullable_replace_field(
    field: &str,
    value: &Option<Option<String>>,
    hash: Option<&str>,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Why：可空字段允许显式 null，但非 null 文本仍要拒绝空字符串。
    let Some(value) = value else {
        return Ok(false);
    };
    if let Some(text) = value.as_deref() {
        validate_required_text(&format!("new_{field}"), text)?;
    }
    validate_expected_hash(field, hash)?;
    Ok(true)
}

fn validate_expected_hash(
    field: &str,
    hash: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：字段 hash 是 update 的版本锁，缺失时不能进入事务。
    let name = format!("expected_{field}_hash");
    validate_required_text(&name, hash.ok_or_else(|| format!("{name} 缺失"))?)?;
    Ok(())
}

fn validate_patch_content_args(
    args: &UpdateMemoryPatchContentArgs,
) -> Result<&str, Box<dyn std::error::Error>> {
    // Why：片段替换依赖精确文本，校验只能挡空值，不能改写用户传入的空格和换行。
    let memory_uuid = validate_required_text("memory_uuid", &args.memory_uuid)?;
    validate_required_text("expected_content_hash", &args.expected_content_hash)?;
    if args.match_content.trim().is_empty() {
        return Err("match_content 不能为空".into());
    }
    if args.replace_content.trim().is_empty() {
        return Err("replace_content 不能为空".into());
    }
    if args.match_content == args.replace_content {
        return Err("NO_CHANGE".into());
    }
    Ok(memory_uuid)
}

fn validate_append_args(
    args: &UpdateMemoryAppendArgs,
) -> Result<(&str, &'static str, &str, &str), Box<dyn std::error::Error>> {
    // What：校验 append 请求只选择 content 或 recall_when 其中一个目标。
    // Why：字段追加依赖对应字段 hash 做版本锁，入口必须先拒绝歧义写入。
    let memory_uuid = validate_required_text("memory_uuid", &args.memory_uuid)?;
    match (
        args.append_content.as_deref(),
        args.append_recall_when.as_deref(),
    ) {
        (Some(text), None) => {
            validate_required_text("append_content", text)?;
            let hash = validate_required_text(
                "expected_content_hash",
                args.expected_content_hash
                    .as_deref()
                    .ok_or("expected_content_hash 缺失")?,
            )?;
            Ok((memory_uuid, "content", hash, text))
        }
        (None, Some(text)) => {
            validate_required_text("append_recall_when", text)?;
            let hash = validate_required_text(
                "expected_recall_when_hash",
                args.expected_recall_when_hash
                    .as_deref()
                    .ok_or("expected_recall_when_hash 缺失")?,
            )?;
            Ok((memory_uuid, "recall_when", hash, text))
        }
        (None, None) => Err("必须提供 append_content 或 append_recall_when".into()),
        (Some(_), Some(_)) => Err("每次只能追加一个字段".into()),
    }
}

fn normalize_keyword_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn validate_add_keywords_args(
    args: &UpdateMemoryKeywordsArgs,
) -> Result<(&str, &str, Vec<String>), Box<dyn std::error::Error>> {
    // What：校验 add_keywords 的 memory_uuid、keywords_hash 和待新增关键词集合。
    // Why：关键词写入有唯一约束，入口先拒绝空值和请求内部重复，避免进入事务后才报库错误。
    let memory_uuid = validate_required_text("memory_uuid", &args.memory_uuid)?;
    let expected_hash =
        validate_required_text("expected_keywords_hash", &args.expected_keywords_hash)?;
    if args.keywords.is_empty() {
        return Err("keywords 必须是非空字符串数组".into());
    }
    let mut keywords = Vec::new();
    for keyword in &args.keywords {
        let keyword_norm = normalize_keyword_text(keyword);
        if keyword_norm.is_empty() {
            return Err("keywords 必须是非空字符串数组".into());
        }
        if keywords.iter().any(|existing| existing == &keyword_norm) {
            return Err("keywords 规范化后不能重复".into());
        }
        keywords.push(keyword_norm);
    }
    Ok((memory_uuid, expected_hash, keywords))
}

async fn lock_update_target(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
) -> Result<(String, Option<String>), Box<dyn std::error::Error>> {
    // Why：更新必须先锁定工作态和待确认记录，否则 hash 校验后的写入仍可能踩到并发变更。
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM memory_units WHERE uuid = $1::uuid FOR UPDATE")
            .bind(memory_uuid)
            .fetch_optional(&mut **tx)
            .await?;
    let Some(status) = status else {
        return Err("memory_uuid 不存在".into());
    };
    let action: Option<String> = sqlx::query_scalar(
        "SELECT action FROM memory_changes WHERE memory_uuid = $1::uuid FOR UPDATE",
    )
    .bind(memory_uuid)
    .fetch_optional(&mut **tx)
    .await?;
    if status == "trashed" || action.as_deref() == Some("delete") {
        return Err("update_memory 不支持已删除记忆".into());
    }
    Ok((status, action))
}

fn assert_replace_hashes(
    args: &UpdateMemoryReplaceArgs,
    state: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：hash 必须在锁定后的当前 state 上重算，才能阻止 Agent 基于过期读取继续写入。
    let hashes = build_memory_hashes(state)?;
    let check = |name: &str, expected: Option<&String>| -> Result<(), Box<dyn std::error::Error>> {
        let actual = hashes[name]
            .as_str()
            .ok_or_else(|| format!("{name} 缺失"))?;
        let expected = expected
            .map(String::as_str)
            .ok_or_else(|| format!("expected_{name} 缺失"))?;
        if actual != expected {
            return Err(format!("{name} 不匹配").into());
        }
        Ok(())
    };
    if args.new_title.is_some() {
        check("title_hash", args.expected_title_hash.as_ref())?;
    }
    if args.new_summary.is_some() {
        check("summary_hash", args.expected_summary_hash.as_ref())?;
    }
    if args.new_recall_when.is_some() {
        check("recall_when_hash", args.expected_recall_when_hash.as_ref())?;
    }
    if args.new_category.is_some() {
        check("category_hash", args.expected_category_hash.as_ref())?;
    }
    if args.new_content.is_some() {
        check("content_hash", args.expected_content_hash.as_ref())?;
    }
    Ok(())
}

fn assert_content_hash(
    expected_content_hash: &str,
    state: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：局部替换也必须基于锁定后的正文，避免 Agent 用过期片段覆盖新内容。
    let hashes = build_memory_hashes(state)?;
    let actual = hashes["content_hash"].as_str().ok_or("content_hash 缺失")?;
    if actual != expected_content_hash {
        return Err("content_hash 不匹配".into());
    }
    Ok(())
}

fn assert_append_hash(
    field: &str,
    expected_hash: &str,
    state: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：按 append 目标字段校验锁定后 state 的字段 hash。
    // Why：append 只修改一个字段，必须拒绝基于过期字段快照的追加请求。
    let hashes = build_memory_hashes(state)?;
    let hash_name = format!("{field}_hash");
    let actual = hashes
        .get(&hash_name)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("{hash_name} 缺失"))?;
    if actual != expected_hash {
        return Err(format!("{hash_name} 不匹配").into());
    }
    Ok(())
}

fn assert_keywords_hash(
    expected_keywords_hash: &str,
    state: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：校验锁定后 state 中 keywords 集合的稳定 hash。
    // Why：关键词增删必须基于最新集合，否则会把并发新增或删除覆盖掉。
    let hashes = build_memory_hashes(state)?;
    let actual = hashes["keywords_hash"]
        .as_str()
        .ok_or("keywords_hash 缺失")?;
    if actual != expected_keywords_hash {
        return Err("keywords_hash 不匹配".into());
    }
    Ok(())
}

fn reject_existing_keywords(
    state: &serde_json::Value,
    keywords: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    // What：检查待新增关键词不能已存在于当前 memory state。
    // Why：add_keywords 是增量新增语义，重复关键词必须明确拒绝而不是静默跳过。
    let existing = state
        .get("keywords")
        .and_then(serde_json::Value::as_array)
        .ok_or("memory state 缺少 keywords")?;
    for keyword in keywords {
        let exists = existing.iter().any(|item| {
            item.get("keyword_norm").and_then(serde_json::Value::as_str) == Some(keyword.as_str())
        });
        if exists {
            return Err(format!("keyword 已存在: {keyword}").into());
        }
    }
    Ok(())
}

fn build_replace_next_state(
    state: &serde_json::Value,
    args: &UpdateMemoryReplaceArgs,
    title_norm: Option<&str>,
) -> Result<(serde_json::Value, Vec<&'static str>), Box<dyn std::error::Error>> {
    // Why：写库前先形成完整快照，后续 memory_units 和 memory_changes 才能共享同一个结果。
    let mut next_state = state.clone();
    let memory = next_state
        .get_mut("memory")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("memory state 缺少 memory")?;
    let mut updated_fields = Vec::new();
    let mut set = |key: &str, output: &'static str, value: serde_json::Value| {
        if memory.get(key) != Some(&value) {
            memory.insert(key.to_string(), value);
            updated_fields.push(output);
        }
    };
    if args.new_title.is_some() {
        set(
            "title_norm",
            "title",
            serde_json::json!(title_norm.ok_or("title_norm 缺失")?),
        );
    }
    if let Some(value) = args.new_category.as_deref() {
        set("category", "category", serde_json::json!(value.trim()));
    }
    if let Some(value) = args.new_content.as_deref() {
        set("content", "content", serde_json::json!(value.trim()));
    }
    if let Some(value) = &args.new_summary {
        set(
            "summary",
            "summary",
            value
                .as_deref()
                .map(|text| serde_json::json!(text.trim()))
                .unwrap_or(serde_json::Value::Null),
        );
    }
    if let Some(value) = &args.new_recall_when {
        set(
            "recall_when",
            "recall_when",
            value
                .as_deref()
                .map(|text| serde_json::json!(text.trim()))
                .unwrap_or(serde_json::Value::Null),
        );
    }
    drop(set);
    if updated_fields.is_empty() {
        return Err("NO_CHANGE".into());
    }
    Ok((next_state, updated_fields))
}

fn build_patch_content_next_state(
    state: &serde_json::Value,
    match_content: &str,
    replace_content: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Why：片段替换必须先在内存快照中完成，主表和 change 才能写入同一个结果。
    let memory = state
        .get("memory")
        .and_then(serde_json::Value::as_object)
        .ok_or("memory state 缺少 memory")?;
    let content = read_memory_text(memory, "content")?;
    let count = content.match_indices(match_content).take(2).count();
    match count {
        0 => return Err("match_content 未找到".into()),
        1 => {}
        _ => return Err("match_content 出现多次".into()),
    }
    let mut next_state = state.clone();
    let memory = next_state
        .get_mut("memory")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("memory state 缺少 memory")?;
    memory.insert(
        "content".to_string(),
        serde_json::json!(content.replacen(match_content, replace_content, 1)),
    );
    Ok(next_state)
}

fn build_append_next_state(
    state: &serde_json::Value,
    field: &str,
    append_text: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // What：在 memory state 中构造追加后的 content 或 recall_when。
    // Why：append 后续写库和 change 记录必须共享同一份快照，不能分别拼接。
    let memory = state
        .get("memory")
        .and_then(serde_json::Value::as_object)
        .ok_or("memory state 缺少 memory")?;
    let current = match memory.get(field) {
        Some(serde_json::Value::String(text)) => text.as_str(),
        Some(serde_json::Value::Null) if field == "recall_when" => "",
        _ => return Err(format!("memory state 字段缺失或不是可追加文本: {field}").into()),
    };
    let mut next_state = state.clone();
    let memory = next_state
        .get_mut("memory")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("memory state 缺少 memory")?;
    memory.insert(
        field.to_string(),
        serde_json::json!(format!("{current}{append_text}")),
    );
    Ok(next_state)
}

async fn reject_replace_duplicates(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    next_state: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：更新后的工作态也必须维持唯一性，否则 approve/reject 之外的读路径会先看到冲突数据。
    let memory = next_state
        .get("memory")
        .and_then(serde_json::Value::as_object)
        .ok_or("memory state 缺少 memory")?;
    let duplicate_message: Option<String> = sqlx::query_scalar(
        r#"
        SELECT CASE
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE uuid <> $1::uuid AND status IN ('pending', 'active') AND title_norm = $2) THEN 'DUPLICATE_TITLE: title_norm 已存在'
            WHEN EXISTS (SELECT 1 FROM memory_units WHERE uuid <> $1::uuid AND status IN ('pending', 'active') AND content = $3) THEN 'DUPLICATE_CONTENT: content 已存在'
            WHEN $4::text IS NOT NULL AND EXISTS (SELECT 1 FROM memory_units WHERE uuid <> $1::uuid AND status IN ('pending', 'active') AND summary = $4) THEN 'DUPLICATE_SUMMARY: summary 已存在'
        END
        "#,
    )
    .bind(memory_uuid)
    .bind(read_memory_text(memory, "title_norm")?)
    .bind(read_memory_text(memory, "content")?)
    .bind(memory.get("summary").and_then(serde_json::Value::as_str))
    .fetch_one(&mut **tx)
    .await?;
    if let Some(message) = duplicate_message {
        return Err(message.into());
    }
    Ok(())
}

async fn write_memory_unit_from_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    next_state: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    // Why：写回只信任后端构建的完整快照，避免多字段更新时漏改工作态字段。
    sqlx::query(
        r#"
        UPDATE memory_units
        SET category = input.state #>> '{memory,category}',
            title_norm = input.state #>> '{memory,title_norm}',
            content = input.state #>> '{memory,content}',
            summary = input.state #>> '{memory,summary}',
            recall_when = input.state #>> '{memory,recall_when}',
            updated_at = now()
        FROM (SELECT $2::jsonb AS state) input
        WHERE uuid = $1::uuid
        "#,
    )
    .bind(memory_uuid)
    .bind(next_state.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_added_keywords(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    keywords: &[String],
) -> Result<(), sqlx::Error> {
    // What：把已经规范化的新关键词写入 memory_keywords。
    // Why：add_keywords 只新增增量关键词，不能重建整张关键词集合。
    sqlx::query(
        r#"
        INSERT INTO memory_keywords (uuid, memory_uuid, keyword_norm, weight, created_at)
        SELECT gen_random_uuid(), $1::uuid, keyword, NULL::int, now()
        FROM jsonb_array_elements_text($2::jsonb) AS keywords(keyword)
        "#,
    )
    .bind(memory_uuid)
    .bind(serde_json::json!(keywords).to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_update_change(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    memory_uuid: &str,
    status: &str,
    action: Option<&str>,
    before_state: &serde_json::Value,
    after_state: &serde_json::Value,
) -> Result<String, Box<dyn std::error::Error>> {
    // Why：更新确认记录必须保留最早回滚基线，否则连续修改后拒绝会回不到原状态。
    let after_state = after_state.to_string();
    match (status, action) {
        ("pending", Some("create")) => {
            sqlx::query("UPDATE memory_changes SET after_state = $2::jsonb, updated_at = now() WHERE memory_uuid = $1::uuid AND action = 'create'")
                .bind(memory_uuid)
                .bind(&after_state)
                .execute(&mut **tx)
                .await?;
            Ok("create".to_string())
        }
        ("active", None) => {
            let before_state = before_state.to_string();
            sqlx::query("INSERT INTO memory_changes (uuid, memory_uuid, action, before_state, after_state, created_at, updated_at) VALUES ($1::uuid, $1::uuid, 'update', $2::jsonb, $3::jsonb, now(), now())")
                .bind(memory_uuid)
                .bind(before_state)
                .bind(&after_state)
                .execute(&mut **tx)
                .await?;
            Ok("update".to_string())
        }
        ("active", Some(action @ ("update" | "restore"))) => {
            sqlx::query("UPDATE memory_changes SET after_state = $2::jsonb, updated_at = now() WHERE memory_uuid = $1::uuid")
                .bind(memory_uuid)
                .bind(&after_state)
                .execute(&mut **tx)
                .await?;
            Ok(action.to_string())
        }
        _ => Err("update_memory 不支持当前 memory 状态".into()),
    }
}

async fn update_memory_replace(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：整字段替换和局部 patch 必须分开，避免 Agent 把片段替换误当成字段覆盖。
    let replace_args = serde_json::from_value::<UpdateMemoryReplaceArgs>(args.clone())?;
    validate_replace_args(&replace_args)?;
    let memory_uuid = validate_required_text("memory_uuid", &replace_args.memory_uuid)?;

    // Why：hash 校验和写入必须共用同一个锁定窗口，否则中途并发更新会绕过版本保护。
    let mut tx = context.profile_pool.begin().await?;
    let (status, action) = lock_update_target(&mut tx, memory_uuid).await?;
    let state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let state = serde_json::from_str::<serde_json::Value>(&state_text)?;
    assert_replace_hashes(&replace_args, &state)?;

    // Why：title_norm 必须由数据库函数生成，避免 Rust 和 PostgreSQL 的规范化规则分叉。
    let title_norm = match replace_args.new_title.as_deref() {
        Some(title) => Some(
            sqlx::query_scalar::<_, String>("SELECT normalize_title($1)")
                .bind(title)
                .fetch_one(&mut *tx)
                .await?,
        ),
        None => None,
    };

    // Why：先生成完整 next_state，后续主表写入和变更记录才能使用同一份结果。
    let (next_state, updated_fields) =
        build_replace_next_state(&state, &replace_args, title_norm.as_deref())?;
    reject_replace_duplicates(&mut tx, memory_uuid, &next_state).await?;
    write_memory_unit_from_state(&mut tx, memory_uuid, &next_state).await?;
    let after_state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let after_state = serde_json::from_str::<serde_json::Value>(&after_state_text)?;

    // Why：memory_changes 是用户二次确认入口，必须和工作态写入保持同一事务。
    let action = upsert_update_change(
        &mut tx,
        memory_uuid,
        &status,
        action.as_deref(),
        &state,
        &after_state,
    )
    .await?;
    if status == "active" {
        crate::psql::mark_memory_graph_dirty(&mut tx).await?;
    }
    tx.commit().await?;

    // Why：提交后再输出成功，避免调用方看到成功但数据库事务实际失败。
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "update_memory_replace",
            "data": {
                "memory_uuid": memory_uuid,
                "action": action,
                "result": "pending_review",
                "updated_fields": updated_fields
            },
            "error": null,
            "profile": context.profile
        })
    );
    Ok(())
}

async fn update_memory_patch_content(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：正文片段替换需要唯一匹配语义，不能混进整字段 replace 的参数集合。
    // 1. 解析并校验 memory_uuid、expected_content_hash、match_content、replace_content。
    // 2. 开启统一 update 事务并锁定目标 memory。
    // 3. 重新计算当前 content_hash，和 expected_content_hash 不一致时拒绝。
    // 4. 检查 match_content 在当前 content 中必须只出现一次。
    // 5. 用 replace_content 替换唯一匹配片段，生成 next_state。
    // 6. 写回 memory_units.content，并写入或覆盖 memory_changes。
    // 7. 如果 active 记忆发生变化，标记 graph dirty。
    // 8. 输出 updated_fields = ["content"] 和 pending_review 结果。
    let patch_args = serde_json::from_value::<UpdateMemoryPatchContentArgs>(args.clone())?;
    let memory_uuid = validate_patch_content_args(&patch_args)?;
    let mut tx = context.profile_pool.begin().await?;
    let (status, action) = lock_update_target(&mut tx, memory_uuid).await?;
    let state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let state = serde_json::from_str::<serde_json::Value>(&state_text)?;
    assert_content_hash(&patch_args.expected_content_hash, &state)?;
    let next_state = build_patch_content_next_state(
        &state,
        &patch_args.match_content,
        &patch_args.replace_content,
    )?;
    reject_replace_duplicates(&mut tx, memory_uuid, &next_state).await?;
    write_memory_unit_from_state(&mut tx, memory_uuid, &next_state).await?;
    let after_state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let after_state = serde_json::from_str::<serde_json::Value>(&after_state_text)?;
    let action = upsert_update_change(
        &mut tx,
        memory_uuid,
        &status,
        action.as_deref(),
        &state,
        &after_state,
    )
    .await?;
    if status == "active" {
        crate::psql::mark_memory_graph_dirty(&mut tx).await?;
    }
    tx.commit().await?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "update_memory_patch_content",
            "data": {
                "memory_uuid": memory_uuid,
                "action": action,
                "result": "pending_review",
                "updated_fields": ["content"]
            },
            "error": null,
            "profile": context.profile
        })
    );
    Ok(())
}

async fn update_memory_append(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：append 只表达文本追加，不承担关键词增删或字段覆盖，降低工具选择歧义。
    // 1. 解析参数，确认只出现 append_content 或 append_recall_when 其中一种。
    // 2. 校验 memory_uuid、追加内容和对应 expected_*_hash。
    // 3. 开启统一 update 事务并锁定目标 memory。
    // 4. 重新计算目标字段 hash，和 expected_*_hash 不一致时拒绝。
    // 5. 将追加内容拼接到目标字段末尾，生成 next_state。
    // 6. 写回目标字段，并写入或覆盖 memory_changes。
    // 7. 如果 active 记忆发生变化，标记 graph dirty。
    // 8. 输出 updated_fields 和 pending_review 结果。
    let append_args = serde_json::from_value::<UpdateMemoryAppendArgs>(args.clone())?;
    let (memory_uuid, field, expected_hash, append_text) = validate_append_args(&append_args)?;
    let mut tx = context.profile_pool.begin().await?;
    let (status, action) = lock_update_target(&mut tx, memory_uuid).await?;
    let state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let state = serde_json::from_str::<serde_json::Value>(&state_text)?;
    assert_append_hash(field, expected_hash, &state)?;
    let next_state = build_append_next_state(&state, field, append_text)?;
    reject_replace_duplicates(&mut tx, memory_uuid, &next_state).await?;
    write_memory_unit_from_state(&mut tx, memory_uuid, &next_state).await?;
    let after_state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let after_state = serde_json::from_str::<serde_json::Value>(&after_state_text)?;
    let action = upsert_update_change(
        &mut tx,
        memory_uuid,
        &status,
        action.as_deref(),
        &state,
        &after_state,
    )
    .await?;
    if status == "active" {
        crate::psql::mark_memory_graph_dirty(&mut tx).await?;
    }
    tx.commit().await?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "update_memory_append",
            "data": {
                "memory_uuid": memory_uuid,
                "action": action,
                "result": "pending_review",
                "updated_fields": [field]
            },
            "error": null,
            "profile": context.profile
        })
    );
    Ok(())
}

async fn update_memory_add_keywords(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：关键词增加独立成工具，避免同一请求同时表达 add/remove/set 多种意图。
    // 1. 解析并校验 memory_uuid、expected_keywords_hash、keywords。
    // 2. 规范化传入 keywords，并拒绝空值或内部重复。
    // 3. 开启统一 update 事务并锁定目标 memory。
    // 4. 重新计算当前 keywords_hash，和 expected_keywords_hash 不一致时拒绝。
    // 5. 检查新增关键词不能和已有关键词重复。
    // 6. 写入 memory_keywords，并构建更新后的 next_state。
    // 7. 写入或覆盖 memory_changes。
    // 8. 如果 active 记忆发生变化，标记 graph dirty。
    // 9. 输出 updated_fields = ["keywords"] 和 pending_review 结果。
    let keyword_args = serde_json::from_value::<UpdateMemoryKeywordsArgs>(args.clone())?;
    let (memory_uuid, expected_hash, keywords) = validate_add_keywords_args(&keyword_args)?;
    let mut tx = context.profile_pool.begin().await?;
    let (status, action) = lock_update_target(&mut tx, memory_uuid).await?;
    let state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let state = serde_json::from_str::<serde_json::Value>(&state_text)?;
    assert_keywords_hash(expected_hash, &state)?;
    reject_existing_keywords(&state, &keywords)?;
    insert_added_keywords(&mut tx, memory_uuid, &keywords).await?;
    let after_state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let after_state = serde_json::from_str::<serde_json::Value>(&after_state_text)?;
    let action = upsert_update_change(
        &mut tx,
        memory_uuid,
        &status,
        action.as_deref(),
        &state,
        &after_state,
    )
    .await?;
    if status == "active" {
        crate::psql::mark_memory_graph_dirty(&mut tx).await?;
    }
    tx.commit().await?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "update_memory_add_keywords",
            "data": {
                "memory_uuid": memory_uuid,
                "action": action,
                "result": "pending_review",
                "updated_fields": ["keywords"]
            },
            "error": null,
            "profile": context.profile
        })
    );
    Ok(())
}

async fn update_memory_remove_keywords(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：关键词删除独立成工具，后续可以单独校验最终 keywords 不能被删空。
    // 1. 解析并校验 memory_uuid、expected_keywords_hash、keywords。
    // 2. 规范化传入 keywords，并拒绝空值或内部重复。
    // 3. 开启统一 update 事务并锁定目标 memory。
    // 4. 重新计算当前 keywords_hash，和 expected_keywords_hash 不一致时拒绝。
    // 5. 检查要删除的关键词必须全部存在。
    // 6. 删除 memory_keywords，并确认最终关键词列表非空。
    // 7. 构建更新后的 next_state，并写入或覆盖 memory_changes。
    // 8. 如果 active 记忆发生变化，标记 graph dirty。
    // 9. 输出 updated_fields = ["keywords"] 和 pending_review 结果。
    let _args = serde_json::from_value::<UpdateMemoryKeywordsArgs>(args.clone())?;
    tool_not_implemented(context, "update_memory_remove_keywords")
}

fn tool_not_implemented(
    context: &super::ToolContext<'_>,
    tool: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：架构接入后必须显式失败，避免调用方误以为 update 已经完成数据库写入。
    let _ = context.profile;
    Err(format!("{tool} 已接入参数解析，具体实现尚未完成").into())
}

#[cfg(test)]
mod tests {
    use super::build_patch_content_next_state;

    fn sample_state(content: &str) -> serde_json::Value {
        // Why：patch_content 的核心风险在内存快照构造，测试用最小 state 就能覆盖。
        serde_json::json!({
            "memory": {
                "content": content
            }
        })
    }

    // Why：正常片段替换必须保留非目标文本，避免局部更新退化成整字段覆盖。
    #[test]
    fn patch_content_replaces_unique_fragment() {
        let state = sample_state("before old after");
        let next = build_patch_content_next_state(&state, "old", "new").unwrap();
        assert_eq!(next["memory"]["content"], "before new after");
    }

    // Why：找不到片段时继续写库会制造假成功，必须在构造 next_state 阶段拒绝。
    #[test]
    fn patch_content_rejects_missing_fragment() {
        let state = sample_state("before old after");
        assert!(build_patch_content_next_state(&state, "missing", "new").is_err());
    }

    // Why：多次匹配无法确定用户意图，不能让后端自行选择第一处修改。
    #[test]
    fn patch_content_rejects_duplicate_fragment() {
        let state = sample_state("old before old after");
        assert!(build_patch_content_next_state(&state, "old", "new").is_err());
    }
}
