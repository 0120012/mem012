# Update Memory

## 1. 目标

`update_memory` 用于修改一条已经存在的记忆。

当前状态：设计文档，Rust CLI 尚未接入。

核心原则：

```text
update_memory = 先更新当前工作态，再写入或覆盖 memory_changes
approve update = 删除 memory_changes，保留当前工作态
reject update = 用 before_state 恢复旧工作态，再删除 memory_changes
```

固定状态：

```text
pending = 已写入，等待用户批准 create
active = 已批准，进入正式召回和图谱
trashed = 已软删除，等待用户批准 delete 或撤销
```

## 2. 请求

```json
{
  "tool": "update_memory",
  "args": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "title": "新的标题",
    "summary": "新的摘要",
    "keywords": ["profile", "database"],
    "handle": "core/backend/database/profile"
  }
}
```

规则：

- 只允许通过 `memory_uuid` 定位。
- `memory_uuid` 必填且不能为空。
- 至少提供一个修改字段。
- 未提供的字段保持不变。
- `recall_when`、`handle` 传 `null` 表示清空。
- 禁止传 `profile`、`title_norm`、`status`、`trashed_at`、`created_at`、`updated_at`。

允许修改字段：

- `category`
- `title`
- `content`
- `summary`
- `keywords`
- `recall_when`
- `handle`

## 3. 校验

- `category` 必须是非 `share` 的 slug。
- `title`、`content`、`summary` 如果提供，不能是空字符串。
- `keywords` 如果提供，必须是非空字符串数组。
- `keywords` 规范化后不能重复。
- `recall_when` 如果提供字符串，不能是空字符串。
- `handle` 如果提供字符串，必须是 2 到 4 段路径。
- `handle` 第一段必须等于最终 `category`。
- `memory_units.status = trashed` 时禁止 update，必须先走 restore。

## 4. case 1: pending create

判断条件：

```text
memory_units.status = pending
memory_changes.action = create
```

动作：

```text
更新 memory_units / memory_keywords / memory_handles
after_state = 更新后的 pending 工作态
UPDATE memory_changes SET after_state, updated_at
```

不修改 `memory_changes.action`，仍然保持 `create`。

不标记 graph dirty，因为 pending memory 不进入正式图谱。

## 5. case 2: active 且没有 open change

判断条件：

```text
memory_units.status = active
不存在 memory_changes.memory_uuid = memory_uuid
```

动作：

```text
before_state = 更新前 active 工作态
更新 memory_units / memory_keywords / memory_handles
after_state = 更新后 active 工作态
INSERT memory_changes(action = update, before_state, after_state)
标记 graph dirty
```

## 6. case 3: active 且已有 open change

如果当前 open change 是 `update` 或 `restore`：

```text
保留已有 before_state
更新 memory_units / memory_keywords / memory_handles
after_state = 更新后工作态
UPDATE memory_changes SET after_state, updated_at
```

不覆盖原 action。`restore` 后继续 update，仍然保留 `restore` action，因为拒绝时要回到 restore 前的基线。

如果当前 open change 是 `delete`：

```text
拒绝 update
```

## 7. 写入范围

`update_memory` 可以写：

- `memory_units`
- `memory_keywords`
- `memory_handles`
- `memory_changes`
- `memory_graph_meta`

`update_memory` 不直接写：

- `memory_embeddings`
- `memory_usage`
- AGE 内部图数据

如果修改了 `title`、`content`、`summary`、`keywords`，embedding 属于派生索引，由 approve update 或后续重建任务刷新。

## 8. 成功响应

```json
{
  "state": "success",
  "tool": "update_memory",
  "data": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "change_uuid": "6a0b1b34-ac8b-4b78-9896-6779c94e7b33",
    "action": "update",
    "result": "pending_review"
  },
  "error": null,
  "profile": "riko"
}
```

`action` 返回当前 `memory_changes.action`，所以 pending create 场景可能返回 `create`，restore 场景可能返回 `restore`。

## 9. 非目标

- 通过 title / handle 更新
- 批量更新
- 修改 relation
- 修改 usage
- 直接刷新 embedding
- 直接 approve / reject
