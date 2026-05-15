# Delete Memory

## 1. 目标

`delete_memory` 用于把一条记忆进入待确认删除阶段。

核心原则：

```text
delete_memory = 软删除，写入或覆盖 delete change
approve delete = 硬删除 memory_units，并级联清理派生表
重复 delete = 幂等返回当前 delete change
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
  "tool": "delete_memory",
  "args": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00"
  }
}
```

规则：

- 只允许通过 `memory_uuid` 删除。
- `memory_uuid` 必填且不能为空。
- 禁止传 `profile`、`handle`、`title`、`uri`。

## 3. case 1: pending create

判断条件：

```text
memory_units.status = pending
memory_changes.action = create
```

动作：

```text
before_state = 当前 pending 工作态
memory_units.status = trashed
memory_units.trashed_at = now()
after_state = 删除后 trashed 工作态
UPDATE memory_changes SET action = delete, before_state, after_state
```

不硬删 `memory_units`，也不标记 graph dirty，因为 pending memory 不进入正式图谱。

## 4. case 2: active 且没有 open change

判断条件：

```text
memory_units.status = active
不存在 memory_changes.memory_uuid = memory_uuid
```

动作：

```text
before_state = 删除前 active 工作态
memory_units.status = trashed
memory_units.trashed_at = now()
after_state = 删除后 trashed 工作态
INSERT memory_changes(action = delete, before_state, after_state)
标记 graph dirty
```

## 5. case 3: 已有 open change

如果当前是 `active + update/restore`：

```text
保留已有 before_state
memory_units.status = trashed
memory_units.trashed_at = COALESCE(trashed_at, now())
after_state = 删除后 trashed 工作态
UPDATE memory_changes SET action = delete, after_state, updated_at
```

如果当前已经是 `trashed + delete`：

```text
直接返回当前 change_uuid
不修改 before_state / after_state / trashed_at
不标记 graph dirty
```

## 6. 成功响应

```json
{
  "state": "success",
  "tool": "delete_memory",
  "data": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "change_uuid": "6a0b1b34-ac8b-4b78-9896-6779c94e7b33",
    "action": "delete",
    "result": "trashed"
  },
  "error": null,
  "profile": "riko"
}
```

`change_uuid` 是后续网页/API 批准 delete 的审核任务 ID。

## 7. approve delete

判断条件：

```text
memory_changes.uuid = change_uuid
memory_changes.action = delete
```

动作：

```text
DELETE FROM memory_changes WHERE uuid = change_uuid
DELETE FROM memory_units WHERE uuid = memory_changes.memory_uuid
```

`memory_keywords`、`memory_handles`、`memory_relations`、`memory_usage`、`memory_embeddings` 依赖外键级联清理。

## 8. reject delete

第一版还未完成。目标语义：

```text
active/delete 被拒绝：用 before_state 恢复 active 工作态，并删除 memory_changes
pending/create/delete 被拒绝：恢复 pending 工作态，并把 memory_changes.action 改回 create
```

## 9. 非目标

- 批量删除
- 自动 purge
- 级联删除关联 memory
- handle / title 删除入口
