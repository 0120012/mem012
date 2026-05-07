# create_memory 最小实现计划

## Context

- `docs/CLI_V10.md` 已定义入口：`mem012 --profile riko --args '<json_object>'`。
- `create_memory` 的请求体是 `{"tool":"create_memory","args":{...}}`。
- 当前数据库初始化已具备 `memory_changes`、`memory_units`、`memory_keywords`、`memory_handles`、`memory_graph_meta`。
- 本阶段只实现 `create_memory`，不提前实现 lookup、search、patch、rollback。
- `docs/DB_PLAN.md` 的数据库合同优先于 CLI 草案：Agent 写入先进入 `memory_changes`，不直接修改正式表。
- 逻辑顺序固定为：`create_memory` 写 `memory_changes`；审查确认后才写 `memory_units`；确认事务内再标记 graph dirty。
- graph 当前只通过 `memory_graph_meta.dirty = true` 表示需要重建，不直接写 AGE 图。
- 工具执行需要两个长期连接池：当前 profile 私库 pool 和 `mem_share` pool。
- `init_db` 不应该再自己创建临时 pool；它应接收 main 已创建好的 pool 引用，完成 schema 检查/迁移后把同一组 pool 留给 tools 使用。

## Runtime Flow

```text
main.rs
-> load config
-> resolve profile database_url + share database_url
-> create profile_pool + share_pool
-> psql::init_db(&profile_pool, &share_pool)
-> parse --args JSON
-> build ToolContext { profile, profile_pool, share_pool }
-> tools::dispatch_tool_request(&context, request).await
-> match tool
-> create_memory::run(&context, args).await
```

边界：

- `main.rs` 负责配置、连接池生命周期、启动模式。
- `psql::init_db` 只负责确认 schema 可用，不拥有运行态连接。
- `tools/mod.rs` 负责 canonical request 校验和 `match tool` 分发。
- `tools/create_memory.rs` 负责 create_memory 的字段校验和 `memory_changes` 写入。

## Constraints

- 每次只改一个最小函数或一个最小模块边界。
- `create_memory` 只产生提案，不写 `memory_units`、`memory_keywords`、`memory_handles`、`memory_relations`、`memory_graph_meta`。
- 确认提案时才需要把正式表写入和 graph dirty 放进同一个数据库事务。
- `profile` 只能来自 CLI 参数，不能进入工具 `args`。
- `create_memory.args` 必须拒绝缺字段、空字符串、非数组 keywords、非字符串 handles。
- `category` 若未提供，先使用 `core`；share 库写入另行处理，不混入本步骤。
- `memory_uuid` 由后端生成，调用方不能传入。
- `title_norm` 必须调用数据库 `normalize_title(text)` 生成，后端不复制 normalize 规则。
- `after_state` 不保存 `title`，只保存 `title_norm`。
- `main.rs` 负责创建并持有 profile/share 两个 pool，tools 层只接收上下文引用，不自己重新读取配置或重新连接数据库。
- 默认写入当前 profile pool；只有明确 share 入口才使用 share pool。
- `dispatch_tool_request` 和具体工具函数必须是 async，因为 match 分发后的工具会调用 SQL。
- `ToolContext` 只保存引用：`profile`、`profile_pool`、`share_pool`；不要把 pool move 进单个工具。

## Checklist

- [x] 调整 `psql::init_db`：从接收 database_url 改为接收 profile/share 两个 pool 引用。
- [x] 在 `main.rs` 创建 profile/share 两个长期 `PgPool`，并复用它们执行 `init_db`。
- [x] 通过 `ToolContext` 把 profile/share 两个 pool 传入工具调用。
- [x] 定义 `ToolContext`：包含 `profile`、profile pool、share pool。
- [x] 把 `dispatch_tool_request` 改成 async，并接收 `&ToolContext` 和 canonical request。
- [x] 解析并校验 canonical request：只允许顶层 `tool` 和 `args`，且 `tool == "create_memory"`。
- [x] 定义 `create_memory` 的最小输入结构，并完成 serde 类型解析。
- [x] 校验 `create_memory` 的必填字符串、keywords、handles、category、禁止字段。
- [x] 使用 `context.profile_pool` 调用数据库 `normalize_title(text)` 得到 `title_norm`。
- [x] 规范化并校验 `category`、`keywords`、`handles`，并拒绝 `profile`、URI、空字符串。
- [x] 由 PostgreSQL 生成 `memory_uuid`，组装完整 `after_state`：`memory`、`keywords`、`handles`、`relations`。
- [x] 写入前检查 `title_norm`、`content`、`summary` 是否已存在于 `memory_units` 或 `memory_changes`。
- [x] 插入 `memory_changes`：`action='create'`、`before_state=NULL`、`after_state`、`memory_uuid`。
- [x] 输出符合 CLI V10 的 JSON 响应外壳，返回 `memory_uuid` 和 `pending_review` 状态。
- [ ] 用一次 `cargo check -q` 和一次本地 `cargo run -- ...create_memory...` 验证提案写入。

## Later

- [ ] 实现确认提案：把 `after_state` 写入 `memory_units`、`memory_keywords`、`memory_handles`。
- [ ] 确认事务内标记 `memory_graph_meta.dirty = true`，然后删除对应 `memory_changes`。
