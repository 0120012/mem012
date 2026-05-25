use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteMemoryArgs {
    memory_uuid: String,
}

enum DeleteMemoryCase {
    PendingCreate,
    ActiveWithoutOpenChange,
    HasOtherOpenChange,
}

pub async fn run(
    context: &super::ToolContext<'_>,
    args: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：先固定 delete_memory 的 CLI 边界，具体事务分支后续按 case 逐步实现。
    let delete_args = serde_json::from_value::<DeleteMemoryArgs>(args.clone())?;
    let memory_uuid = validate_memory_uuid(&delete_args)?;
    let delete_case = classify_delete_case(context, memory_uuid).await?;
    match delete_case {
        DeleteMemoryCase::PendingCreate => mark_pending_create_delete(context, memory_uuid).await,
        DeleteMemoryCase::ActiveWithoutOpenChange => {
            mark_pending_delete(context, memory_uuid).await
        }
        DeleteMemoryCase::HasOtherOpenChange => mark_open_change_delete(context, memory_uuid).await,
    }
}

fn validate_memory_uuid(args: &DeleteMemoryArgs) -> Result<&str, Box<dyn std::error::Error>> {
    // Why：删除是破坏性操作，只允许强身份 memory_uuid，避免标题等弱定位带来歧义。
    let memory_uuid = args.memory_uuid.trim();
    if memory_uuid.is_empty() {
        return Err("memory_uuid 不能为空".into());
    }
    Ok(memory_uuid)
}

async fn classify_delete_case(
    context: &super::ToolContext<'_>,
    memory_uuid: &str,
) -> Result<DeleteMemoryCase, Box<dyn std::error::Error>> {
    // Why：这里只做轻量分流，真正防并发的锁定仍由各 case 自己在事务内完成。
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        r#"
        SELECT u.status, c.action
        FROM memory_units u
        LEFT JOIN memory_changes c ON c.memory_uuid = u.uuid
        WHERE u.uuid = $1::uuid
        "#,
    )
    .bind(memory_uuid)
    .fetch_optional(context.profile_pool)
    .await?;
    let Some((status, action)) = row else {
        return Err("memory_uuid 不存在".into());
    };
    match (status.as_str(), action.as_deref()) {
        ("pending", Some("create")) => Ok(DeleteMemoryCase::PendingCreate),
        ("active", None) => Ok(DeleteMemoryCase::ActiveWithoutOpenChange),
        (_, Some(_)) => Ok(DeleteMemoryCase::HasOtherOpenChange),
        _ => Err("delete_memory 不支持当前 memory 状态".into()),
    }
}

async fn mark_pending_create_delete(
    context: &super::ToolContext<'_>,
    memory_uuid: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：pending create 删除也必须二次确认，不能绕过用户批准直接硬删。
    let mut tx = context.profile_pool.begin().await?;
    let change_exists: Option<String> = sqlx::query_scalar(
        r#"
        SELECT c.memory_uuid::text
        FROM memory_units u
        JOIN memory_changes c ON c.memory_uuid = u.uuid
        WHERE u.uuid = $1::uuid AND u.status = 'pending' AND c.action = 'create'
        FOR UPDATE OF u, c
        "#,
    )
    .bind(memory_uuid)
    .fetch_optional(&mut *tx)
    .await?;
    if change_exists.is_none() {
        tx.rollback().await?;
        return Err("delete_memory 只能软删除 pending create".into());
    }
    let before_state = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    sqlx::query("UPDATE memory_units SET status = 'trashed', trashed_at = now(), updated_at = now() WHERE uuid = $1::uuid")
        .bind(memory_uuid)
        .execute(&mut *tx)
        .await?;
    let after_state = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    sqlx::query("UPDATE memory_changes SET action = 'delete', before_state = $2::jsonb, after_state = $3::jsonb, updated_at = now() WHERE memory_uuid = $1::uuid")
        .bind(memory_uuid)
        .bind(before_state)
        .bind(after_state)
        .execute(&mut *tx)
        .await?;
    crate::psql::search_index::refresh_memory_search_index(&mut tx, memory_uuid).await?;
    tx.commit().await?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "delete_memory",
            "data": {
                "memory_uuid": memory_uuid,
                "action": "delete",
                "result": "trashed"
            },
            "error": null,
            "profile": context.profile
        })
    );
    Ok(())
}

async fn mark_pending_delete(
    context: &super::ToolContext<'_>,
    memory_uuid: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：case2 表示正式记忆软删除，后续要写 before_state 和 delete change。
    // 1. 开启事务，锁定 memory_units 中这条 active memory。
    // 2. 确认 memory_changes 中不存在同一 memory_uuid 的 open change。
    // 3. 读取删除前完整工作态，生成 before_state。
    // 4. 更新 memory_units.status = 'trashed'，并写入 trashed_at = now()。
    // 5. 读取删除后完整工作态，生成 after_state。
    // 6. 插入 memory_changes，action = 'delete'，保存 before_state 和 after_state。
    // 7. 刷新 memory_search_index，保证搜索投影同步变为 trashed。
    // 8. 标记 memory_graph_meta.dirty，因为 active memory 可见性已经变化。
    // 9. 提交事务。
    // 10. 返回 trashed，表示已软删除并等待用户二次确认硬删除。
    let mut tx = context.profile_pool.begin().await?;
    let locked_memory: Option<String> = sqlx::query_scalar(
        "SELECT uuid::text FROM memory_units WHERE uuid = $1::uuid AND status = 'active' FOR UPDATE",
    )
    .bind(memory_uuid)
    .fetch_optional(&mut *tx)
    .await?;
    if locked_memory.is_none() {
        tx.rollback().await?;
        return Err("delete_memory 只能删除 active memory".into());
    }

    let has_open_change: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM memory_changes WHERE memory_uuid = $1::uuid)",
    )
    .bind(memory_uuid)
    .fetch_one(&mut *tx)
    .await?;
    if has_open_change {
        tx.rollback().await?;
        return Err("memory has pending change".into());
    }

    let before_state = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    sqlx::query("UPDATE memory_units SET status = 'trashed', trashed_at = now(), updated_at = now() WHERE uuid = $1::uuid")
        .bind(memory_uuid)
        .execute(&mut *tx)
        .await?;
    let after_state = crate::psql::memory_state(&mut tx, memory_uuid)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    sqlx::query(
        "INSERT INTO memory_changes (uuid, memory_uuid, action, before_state, after_state, created_at, updated_at) VALUES ($1::uuid, $1::uuid, 'delete', $2::jsonb, $3::jsonb, now(), now())",
    )
    .bind(memory_uuid)
    .bind(before_state)
    .bind(after_state)
    .execute(&mut *tx)
    .await?;
    crate::psql::search_index::refresh_memory_search_index(&mut tx, memory_uuid).await?;
    crate::psql::mark_memory_graph_dirty(&mut tx).await?;
    tx.commit().await?;
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "delete_memory",
            "data": {
                "memory_uuid": memory_uuid,
                "action": "delete",
                "result": "trashed"
            },
            "error": null,
            "profile": context.profile
        })
    );
    Ok(())
}

async fn mark_open_change_delete(
    context: &super::ToolContext<'_>,
    memory_uuid: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Why：delete_memory 可以覆盖未确认 update/restore，但不能重复覆盖已经进入 delete 的状态。
    // 1. 开启事务，锁定 memory_units 和 memory_changes 中这条 memory_uuid。
    // 2. 如果当前已经是 status = 'trashed' 且 action = 'delete'，直接返回 trashed。
    // 3. 保留已有 before_state，作为唯一可靠回滚基线。
    // 4. 更新 memory_units.status = 'trashed'，trashed_at = COALESCE(trashed_at, now())。
    // 5. 读取删除后完整工作态，生成新的 after_state。
    // 6. 覆盖 memory_changes.action = 'delete'，只更新 after_state 和 updated_at。
    // 7. 刷新 memory_search_index，保证覆盖 open change 后搜索投影同步。
    // 8. 如果删除前是 active，标记 memory_graph_meta.dirty。
    // 9. 提交事务并返回 trashed。
    let mut tx = context.profile_pool.begin().await?;
    let row: Option<(String, String, Option<String>)> = sqlx::query_as(
        r#"
        SELECT u.status, c.action, c.before_state::text
        FROM memory_units u
        JOIN memory_changes c ON c.memory_uuid = u.uuid
        WHERE u.uuid = $1::uuid
        FOR UPDATE OF u, c
        "#,
    )
    .bind(memory_uuid)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((status, action, before_state)) = row else {
        tx.rollback().await?;
        return Err("memory has no pending change".into());
    };
    if status == "trashed" && action == "delete" {
        tx.commit().await?;
    } else {
        before_state.ok_or("before_state is required")?;
        sqlx::query("UPDATE memory_units SET status = 'trashed', trashed_at = COALESCE(trashed_at, now()), updated_at = now() WHERE uuid = $1::uuid")
            .bind(memory_uuid)
            .execute(&mut *tx)
            .await?;
        let after_state = crate::psql::memory_state(&mut tx, memory_uuid)
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        sqlx::query("UPDATE memory_changes SET action = 'delete', after_state = $2::jsonb, updated_at = now() WHERE memory_uuid = $1::uuid")
            .bind(memory_uuid)
            .bind(after_state)
            .execute(&mut *tx)
            .await?;
        crate::psql::search_index::refresh_memory_search_index(&mut tx, memory_uuid).await?;
        if status == "active" {
            crate::psql::mark_memory_graph_dirty(&mut tx).await?;
        }
        tx.commit().await?;
    }
    println!(
        "{}",
        serde_json::json!({
            "state": "success",
            "tool": "delete_memory",
            "data": {
                "memory_uuid": memory_uuid,
                "action": "delete",
                "result": "trashed"
            },
            "error": null,
            "profile": context.profile
        })
    );
    Ok(())
}
