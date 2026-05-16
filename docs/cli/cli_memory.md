# CLI Memory

## 1. 定位

CLI Memory 当前保留两个写入工具：

```text
create_memory
delete_memory
```

CLI 的职责是让 Agent 或人类用一次性 JSON 请求创建或删除记忆。读取、审核、项目选择和图谱展示不走 CLI，交给 HTTP API。

Why：CLI 先保持最小可执行面，避免 Rust 到 C++ 迁移前同时维护多套调用入口。

## 2. 运行方式

```bash
mem012 --profile riko --args '<json_object>'
```

规则：

- `--profile` 必填。
- `--args` 必填。
- `--args` 必须是完整 JSON object。
- JSON 外层必须用 shell 引号包住。
- `profile` 只能来自启动参数，不能放进 JSON args。

正确示例：

```bash
mem012 --profile riko --args '{"tool":"create_memory","args":{"title":"Profile 隔离规则","content":"profile 是数据库隔离边界。","summary":"profile 用于隔离数据库连接。","keywords":["profile"],"handle":"core/backend/database/profile隔离"}}'
```

错误示例：

```bash
mem012 --profile riko --args {"tool":"create_memory","args":{}}
```

## 3. 请求外壳

```json
{
  "tool": "create_memory",
  "args": {}
}
```

顶层规则：

- 只允许 `tool` 和 `args`。
- `tool` 必须是字符串。
- `args` 必须是 object。
- 当前合法 `tool` 是 `create_memory` 或 `delete_memory`。

## 4. 响应

成功响应：

```json
{
  "state": "success",
  "tool": "create_memory",
  "data": {},
  "error": null,
  "profile": "riko"
}
```

规则：

- `state` 成功时为 `success`。
- `profile` 直接放在顶层。
- 不再返回 `meta`。
- 不再返回 `spec_version`。
- 失败时当前进程直接返回错误；后续如果需要机器稳定解析，再单独补统一 failed JSON。

## 5. create_memory

创建一条记忆。调用成功后，记忆会以 `pending` 状态写入当前工作态，同时写入待用户二次确认的 `memory_changes`。

请求：

```json
{
  "tool": "create_memory",
  "args": {
    "category": "core",
    "title": "Profile 隔离规则",
    "content": "profile 是数据库隔离边界，不进入搜索参数。",
    "summary": "profile 用于隔离数据库连接，Agent 搜索时不传 profile。",
    "keywords": ["profile", "数据库隔离", "搜索参数"],
    "recall_when": "当讨论多库隔离、profile、数据库连接选择时召回",
    "handle": "core/backend/database/profile隔离"
  }
}
```

必填字段：

- `title`
- `content`
- `summary`
- `keywords`

可选字段：

- `category`
- `recall_when`
- `handle`

校验规则：

- `category` 未提供时默认为 `core`。
- `category` 必须是 slug。
- `category` 不能是 `share`。
- `title`、`content`、`summary` 不能为空字符串。
- `keywords` 必须是非空字符串数组。
- `keywords` 规范化后不能重复。
- `recall_when` 如果提供，不能是空字符串。
- `handle` 如果提供，必须是 2 到 4 段路径。
- `handle` 第一段必须等于 `category`。
- `handle` 每一段 trim 后都不能为空。
- `args` 内禁止出现 `profile`、`memory_uuid`、`title_norm`、`uri`。

成功响应：

```json
{
  "state": "success",
  "tool": "create_memory",
  "data": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "result": "pending"
  },
  "error": null,
  "profile": "riko"
}
```

## 6. handle

`handle` 是人类和 Agent 快速定位记忆用的可读路径，不是数据库主键。

结构：

```text
category/group/collection/item
```

允许 2 到 4 段：

```text
core/backend
core/backend/database
core/backend/database/profile隔离
book/思维/thinking_in_systems/chapter_1
```

不允许：

```text
book
book/思维/thinking_in_systems/chapter_1/note_1
/core/backend
core/backend/
core//backend
core/   /profile
```

数据库里保存的是规范化后的：

```text
memory_handles.handle_norm
```

## 7. 写入语义

`create_memory` 成功后会写入：

- `memory_units`
- `memory_keywords`
- `memory_handles`
- `memory_changes`

其中：

- `memory_units.status = pending`，表示已写入但未批准。
- `memory_changes` 是用户二次确认记录。
- `memory_changes.before_state` 为 `null`。
- `memory_changes.after_state` 保存完整工作态快照。
- `memory_uuid` 由后端生成。
- `title_norm` 由数据库 `normalize_title(text)` 生成。

`create_memory` 不应让 pending 记忆进入正式召回或 AGE 图谱；approve 后变为 `active` 再参与正式查询。

## 8. delete_memory

删除一条记忆。调用成功后，记忆进入 `trashed`，并返回后续网页/API 批准删除所需的 `change_uuid`。

请求：

```json
{
  "tool": "delete_memory",
  "args": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00"
  }
}
```

成功响应：

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

## 9. 非目标

CLI Memory 不提供：

- memory 查询
- memory 搜索
- memory 更新
- change approve / reject
- graph status
- graph rebuild
- graph neighbors
- relation 增删改

这些能力暂时由 HTTP API 承担。
