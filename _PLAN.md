# read_memory 计划

## Context conclusions

- 当前 CLI 已有 `create_memory`、`delete_memory`、`read_memory_hash` 和 `update_memory_*` 工具，但没有 `read_memory`。
- `read_memory_hash` 位于 `src/tools/update_memory.rs`，通过 `crate::psql::memory_state` 读取同一份稳定快照，只返回标题和字段 hash。
- `crate::psql::memory_state` 已能返回完整当前工作态：`memory`、`keywords`、`relations`。
- `memory.summary` 和 `memory.recall_when` 可以是 `null`，读取工具必须原样返回，不能转成空字符串。
- `memory.status = trashed` 目前被 `read_memory_hash` 拒绝；`read_memory` 默认沿用这个边界，避免把已删除记忆当成可读工作态。
- `read_memory` 是只读工具，不应写入 `memory_units`、`memory_keywords`、`memory_changes`，不应标记 graph dirty，也不应处理 embedding。
- 工具输出应沿用现有 envelope：`state`、`tool`、`data`、`error`、`profile`。
- 当前工作区已有未提交的 `docs/cli/skill.md` 修改；开发 `read_memory` 时必须避开或单独提交，不能混进代码提交。

## Failure points

- 不能手工拼多个表的局部字段，避免和 `memory_state` 的快照结构分叉。
- 不能在读取时修改 `memory_changes`，否则只读工具会影响 approve/reject 流程。
- 不能吞掉 `null` 字段，否则调用方无法区分“未设置”和“空文本”。
- 不能让未知参数静默通过，Public CLI 入参必须继续 `deny_unknown_fields`。
- 不能把不存在的 `memory_uuid` 包装成成功空对象，必须返回明确错误。

## Checklist

- [x] 将 `read_memory_hash` 从 `update_memory` 模块移动到独立 `read_memory` 模块，保持现有响应不变；验证 `cargo fmt --check` 和 `cargo check`。
- [x] 接入 `read_memory` 的 CLI 路由和参数结构，只做 `memory_uuid` 入参校验并保持最小可编译切片；验证 `cargo fmt --check` 和 `cargo check`。
- [x] 实现 `read_memory` 读取路径：复用 `memory_state`，拒绝不存在或 `trashed` 记忆，返回完整 `memory`、`keywords`、`relations`；验证 `cargo fmt --check` 和 `cargo check`。
- [ ] 用一条已存在或新建记忆执行 CLI smoke：`create_memory` 后调用 `read_memory`，确认返回 `memory_uuid`、正文、摘要、召回条件和关键词；失败时只修正本工具范围。
- [x] 增加 `read_memory` 的文档示例，说明用途、调用命令和验证成功方式；验证 `git diff --check`。
