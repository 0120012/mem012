# V2 计划索引

## Ownership

- 后端接口设计与后端代码：见 `BACKEND_PLAN.md`，由当前 Codex 线程负责。
- 前端工程与界面设计：见 `FRONTEND_PLAN.md`，由 ds 负责。

## Shared Constraints

- 前端必须使用 Vite + React + shadcn/ui。
- UI 只使用黑白灰配色。
- 后端 API 不兼容旧 URI / node 接口，旧 `src/api/*` 可以删除重建。
- `memory_units` 是当前工作态。
- `memory_changes` 是否存在表示该记忆等待用户二次确认。
- `create_memory` 已完成三表写入：`memory_units`、`memory_keywords`、`memory_changes`。
