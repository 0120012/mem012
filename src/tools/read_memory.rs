// What：承载 read_memory 系列只读工具。
// Why：读取内容和读取 hash 都是只读边界，不能继续挂在 update_memory 写入模块里。
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
struct ReadMemoryArgs {
    memory_uuid: String,
}

pub async fn run(
    context: &super::ToolContext<'_>,
    tool: &str,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    match tool {
        "read_memory" => read_memory(context, args).await,
        "read_memory_hash" => read_memory_hash(context, args).await,
        _ => Err(format!("未知 read 工具: {tool}").into()),
    }
}

async fn read_memory(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：读取一条记忆的完整当前工作态。
    // Why：read_memory 必须复用 memory_state 快照，避免主体、关键词和关系来自不同版本。
    let read_args = serde_json::from_value::<ReadMemoryArgs>(args.clone())?;
    let memory_uuid = validate_required_text("memory_uuid", &read_args.memory_uuid)?;
    let state = load_memory_state(context, memory_uuid).await?;
    let memory = memory_object(&state)?;
    reject_trashed(memory, "read_memory")?;
    let response = read_memory_response(context.profile, memory_uuid, &state)?;
    println!("{response}");
    Ok(())
}

async fn read_memory_hash(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：读取目标记忆当前字段 hash。
    // Why：hash 读取是所有 update 的前置步骤，独立在只读工具模块里避免写入入口承担读取职责。
    let read_args = serde_json::from_value::<ReadMemoryHashArgs>(args.clone())?;
    let memory_uuid = validate_required_text("memory_uuid", &read_args.memory_uuid)?;
    let state = load_memory_state(context, memory_uuid).await?;
    let memory = memory_object(&state)?;
    reject_trashed(memory, "read_memory_hash")?;

    let response = serde_json::json!({
        "state": "success",
        "tool": "read_memory_hash",
        "data": {
            "memory_uuid": memory_uuid,
            "title_norm": read_memory_text(memory, "title_norm")?,
            "revision": memory
                .get("revision")
                .and_then(serde_json::Value::as_i64)
                .ok_or("memory state 字段缺失或不是整数: revision")?,
            "hash": build_memory_hashes(&state)?,
        },
        "error": null,
        "profile": context.profile
    });
    println!("{response}");
    Ok(())
}

async fn load_memory_state(
    context: &super::ToolContext<'_>,
    memory_uuid: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut tx = context.profile_pool.begin().await?;
    let state_text = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    tx.commit().await?;
    Ok(serde_json::from_str::<serde_json::Value>(&state_text)?)
}

fn read_memory_response(
    profile: &str,
    memory_uuid: &str,
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(serde_json::json!({
        "state": "success",
        "tool": "read_memory",
        "data": {
            "memory_uuid": memory_uuid,
            "memory": state.get("memory").ok_or("memory state 缺少 memory")?,
            "keywords": state.get("keywords").ok_or("memory state 缺少 keywords")?,
            "relations": state.get("relations").ok_or("memory state 缺少 relations")?,
        },
        "error": null,
        "profile": profile
    }))
}

fn memory_object(
    state: &serde_json::Value,
) -> Result<&serde_json::Map<String, serde_json::Value>, Box<dyn std::error::Error>> {
    state
        .get("memory")
        .and_then(serde_json::Value::as_object)
        .ok_or("memory state 缺少 memory".into())
}

fn reject_trashed(
    memory: &serde_json::Map<String, serde_json::Value>,
    tool: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if read_memory_text(memory, "status")? == "trashed" {
        return Err(format!("{tool} 不支持 trashed memory").into());
    }
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
