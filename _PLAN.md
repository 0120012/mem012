# update_memory_replace 计划

## Context conclusions

- `read_memory_hash` 已完成，返回字段 hash，hash 格式为 `0x...`。
- `update_memory_replace` 已接入路由，但当前仍返回 `tool_not_implemented`。
- `update_memory_replace` 只处理整字段替换，不处理 content 片段 patch、append、keywords。
- 一次请求可以传多个 `new_*` 字段；后端按固定顺序应用。
- 每个 `new_*` 字段必须携带对应的 `expected_*_hash`。
- 本轮不强制 `expected_state_hash`，避免扩大 CLI 协议；并发保护先按字段 hash 做。
- `memory_units` 是当前工作态，更新必须先落到 `memory_units`。
- `memory_changes` 是待用户二次确认记录；同一 `memory_uuid` 最多一条。
- `pending + create`：保留 `action = create`，只覆盖 `after_state`，不标记 graph dirty。
- `active + 无 change`：保存当前 state 为 `before_state`，插入 `action = update`。
- `active + update/restore`：保留原 `before_state` 和原 `action`，只覆盖 `after_state`。
- `delete` 或 `trashed` 状态拒绝更新。
- 写回 `memory_units` 使用 `next_state` 的静态 SQL，不做动态字段 SQL。
- `summary`、`recall_when` 允许用 `null` 清空；空字符串拒绝。
- `title` 写入前必须走数据库 `normalize_title`。
- 更新后必须执行重复检测，避免 title/content/summary 撞到其它 pending/active 记忆。

## Failure points

- 不能覆盖已有 `before_state`，否则用户拒绝时无法回到最早状态。
- 不能让 pending create 标记 graph dirty，因为 pending 不进入正式图谱。
- 不能在 hash 不匹配时写入任何表。
- 不能在没有实际变化时制造 `memory_changes`。
- 不能只改 `memory_units` 而不更新 `memory_changes.after_state`。
- 不能把 `new_title` 原文直接写入 `title_norm`。

## Checklist

- [x] 调整 `UpdateMemoryReplaceArgs`，让 `summary` 支持显式 `null`。
- [x] 提取 `validate_replace_args`，只校验 `memory_uuid`、字段配对、空字符串和至少一个更新字段。
- [x] 提取 `lock_replace_target`，在事务内锁定 `memory_units` 和可选 `memory_changes`，并拒绝 `trashed/delete`。
- [x] 提取 `assert_replace_hashes`，用当前 state 校验每个被修改字段的 `expected_*_hash`。
- [x] 移除 `read_memory_hash` 响应中的 `status`，状态只在后端内部判断。
- [x] 提取 `build_replace_next_state`，在内存中应用 `new_*`，并返回实际变化的 `updated_fields`。
- [x] 提取 `reject_replace_duplicates`，排除自身后检查 title/content/summary 是否重复。
- [x] 提取 `write_memory_unit_from_state`，用静态 SQL 从 `next_state` 写回 `memory_units`。
- [x] 提取 `upsert_update_change`，按 pending/create、active/new、active/existing 三类写入或覆盖 `memory_changes`。
- [x] 接入 `update_memory_replace` 写库路径但保持回滚，避免提交后仍返回未实现错误。
- [x] 提交事务并返回成功响应，返回 `action`、`result = pending_review`、`updated_fields`。
- [x] 运行 `cargo check -q`。
