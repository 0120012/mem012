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
    new_summary: Option<String>,
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
    // 6. 输出 title_norm、status 和 data.hash，供后续 update 工具原样携带。
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
                "status": status,
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

async fn update_memory_replace(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：整字段替换和局部 patch 必须分开，避免 Agent 把片段替换误当成字段覆盖。
    // 1. 解析参数，确认本次替换请求形状合法。
    // 2. 校验 memory_uuid 和对应 expected_*_hash 不能为空。
    // 3. 根据 new_* 字段判断要替换的目标字段。
    // 4. 开启统一 update 事务并锁定目标 memory。
    // 5. 重新计算当前字段 hash，和 expected_*_hash 不一致时拒绝。
    // 6. 应用整字段替换，生成 next_state。
    // 7. 写回工作态，并写入或覆盖 memory_changes。
    // 8. 如果 active 记忆发生变化，标记 graph dirty。
    // 9. 输出 updated_fields 和 pending_review 结果。
    let _args = serde_json::from_value::<UpdateMemoryReplaceArgs>(args.clone())?;
    tool_not_implemented(context, "update_memory_replace")
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
    let _args = serde_json::from_value::<UpdateMemoryPatchContentArgs>(args.clone())?;
    tool_not_implemented(context, "update_memory_patch_content")
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
    let _args = serde_json::from_value::<UpdateMemoryAppendArgs>(args.clone())?;
    tool_not_implemented(context, "update_memory_append")
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
    let _args = serde_json::from_value::<UpdateMemoryKeywordsArgs>(args.clone())?;
    tool_not_implemented(context, "update_memory_add_keywords")
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
