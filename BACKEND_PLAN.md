# V2 后端接口与代码计划

## Scope

- 本文件只负责后端接口设计和 Rust 后端实现。
- 不设计前端页面、组件、布局或交互细节。
- 旧 `src/api/*` 是旧 URI / node / review 占位接口，可以删除重建。

## API Contract

统一响应：

```json
{
  "state": "success",
  "data": {},
  "error": null,
  "meta": {
    "project": "riko"
  }
}
```

失败响应：

```json
{
  "state": "failed",
  "data": null,
  "error": {
    "code": "ERROR_CODE",
    "message": "human readable message"
  },
  "meta": {
    "project": "riko"
  }
}
```

当前 project 由前端请求头传递：

```text
X-Mem-Project: riko
```

后端必须校验 `X-Mem-Project` 是否存在于配置文件；不接受任意数据库名。

## Backend API

认证：

```text
POST /auth/verify
GET  /auth/session
```

project：

```text
GET /projects
```

记忆：

```text
GET /memories
```

二次确认：

```text
GET  /changes
GET  /changes/{memory_uuid}
POST /changes/{memory_uuid}/approve
POST /changes/{memory_uuid}/reject
```

## API Semantics

### POST /auth/verify

- 校验用户输入的密钥。
- 成功后建立 session。

### GET /auth/session

- 检查当前请求是否已有有效 session。

### GET /projects

- 返回配置文件中可选择的库。
- 不返回数据库密码或连接串。

最小字段：

```text
project_id
display_name
database_name
db_scope
is_share
```

### GET /memories

- 返回当前 project 的记忆列表。
- 直接读取 `memory_units.status`；`pending` 表示已写入但未批准。

最小字段：

```text
memory_uuid
category
title_norm
summary
status
has_open_change
change_action
created_at
updated_at
```

### GET /changes

- 返回当前 project 的待确认列表。
- 从 `memory_changes` join `memory_units` 取列表展示字段。

最小字段：

```text
memory_uuid
action
title_norm
summary
created_at
updated_at
```

### GET /changes/{memory_uuid}

- 返回待确认详情。
- create 时 `before_state = null`。
- update/delete/restore 时返回 before / after。

### POST /changes/{memory_uuid}/approve

- 删除对应 `memory_changes`。
- `action = update/restore` 时不重复写入 `memory_units`，因为当前工作态已经生效。
- `action = create` 时把 `memory_units.status` 改为 `active`，自动生成默认 `related_to` relations，并标记 graph dirty。
- `action = delete` 时硬删除对应 `memory_units`，依靠 cascade 清理派生表，并标记 graph dirty。

### POST /changes/{memory_uuid}/reject

- `action = create`：删除 change 后删除 memory，依靠 cascade 清理派生表。
- `action = update/delete/restore`：用 `before_state` 恢复工作态，再删除 change。

## Relation / Graph Semantics

- `memory_relations` 是关系主数据，AGE 只做派生图查询。
- create/update/delete/restore 改变工作态或关系时，必须在同一事务内标记 `memory_graph_meta.dirty = true`。
- approve create 会把 `pending` 改为 `active`，删除 `memory_changes` 并自动写默认 relations；如果写入 relation，则标记 `dirty`。
- approve delete 会删除 `memory_changes` 并硬删除 `memory_units`；必须标记 `dirty`。
- approve update/relation 只删除 `memory_changes`，不改工作态。
- reject 如果删除或恢复了 memory / relation，必须标记 `dirty = true`。
- AGE rebuild 只读取 SQL 当前工作态，不读取 `memory_changes`。
- 当前状态检查入口是 CLI tool：`graph_status`。
- 当前 rebuild 入口是 CLI tool：`rebuild_graph`。
- 当前默认图谱入口是 HTTP `GET /api/graph/overview`，返回 `nodes` 和 `relations`。
- 当前一跳查询入口是 CLI tool：`graph_neighbors` 和 HTTP `GET /api/graph/neighbors/{memory_uuid}`。
- relation 增删改入口是 CLI tool：`add_memory_relation` / `update_memory_relation` / `delete_memory_relation`。
- relation HTTP 入口：`POST /api/graph/relations`、`PATCH /api/graph/relations/{relation_uuid}`、`DELETE /api/graph/relations/{relation_uuid}`。
- relation 候选 HTTP 入口：`GET /api/graph/relations/suggest/{memory_uuid}`。

## Backend Rules

- 后端 API 不兼容旧 URI / node 接口。
- 旧 `src/api/*` 可以删除重建。
- 所有非 auth API 都必须要求 session 有效。
- handler 不直接拼复杂 SQL 业务逻辑；复杂查询放到 psql/service 函数。
- approve / reject 必须在事务内完成。
- 前端 approve / reject 只传 `memory_uuid`，不传 before_state 或 after_state。

## Checklist

- [ ] 删除旧 `src/api` 路由设计，只保留最小 health/auth 入口。
- [ ] 定义统一 API 响应 helper。
- [ ] 接入 `POST /auth/verify`。
- [ ] 接入 `GET /auth/session`。
- [ ] 接入 `GET /projects`。
- [ ] 接入 `GET /memories`。
- [ ] 实现 `GET /memories` 查询。
- [ ] 接入 `GET /changes`。
- [ ] 实现 `GET /changes` 查询。
- [ ] 接入 `GET /changes/{memory_uuid}`。
- [ ] 实现 change detail 查询。
- [ ] 接入 `POST /changes/{memory_uuid}/approve`。
- [ ] 实现 approve 事务。
- [ ] 接入 `POST /changes/{memory_uuid}/reject`。
- [ ] 实现 reject create 事务。
- [ ] 实现 reject update/delete/restore 事务。
- [x] 实现 graph dirty 标记函数。
- [x] create_memory 工作态写入后标记 graph dirty。
- [x] reject 回滚工作态后标记 graph dirty。
- [x] 实现 relation 写入入口。
- [x] 实现 graph status 只读入口。
- [x] 实现 AGE graph rebuild 入口。
- [x] 实现 relation 增删改工具。
- [x] 实现 relation 候选生成工具。
- [x] 实现 SQL 一跳 graph 查询工具。
- [x] 接入 graph status / rebuild / neighbors HTTP API。
- [x] 接入 relation 增删改 HTTP API。
- [x] 接入 relation 候选 HTTP API。
- [x] 补充 relation 入参本地测试。
- [x] 用 `cargo check -q` 验证后端。

## Later

- [ ] 分页、搜索、category/status 筛选。
- [ ] memory detail API。
- [ ] update_memory API。
- [ ] 完整审计表 `memory_events`。
