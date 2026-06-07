# CLI Memory

## 1. 定位

CLI Memory 当前保留的 Rust CLI 工具有：

```text
create_memory
delete_memory
read_memory
read_memory_hash
update_memory_replace
update_memory_patch_content
update_memory_append
update_memory_add_keywords
update_memory_remove_keywords
```

CLI 的职责是让 Agent 或人类用一次性 JSON 请求创建、删除、读取和更新记忆。搜索、审核、项目选择和图谱展示不走 CLI，交给 HTTP API。

Why：CLI 先保持最小可执行面，避免 Rust 到 C++ 迁移前同时维护多套调用入口。

## 2. 运行方式

```bash
mem012 --profile riko --args '<json_object>'
```

初始化读取命令不使用 `--args`：

```bash
mem012 --profile riko init
```

规则：

- `--profile` 必填。
- JSON 工具请求必须传 `--args`。
- `init` 命令不传 `--args`。
- `--args` 必须是完整 JSON object。
- JSON 外层必须用 shell 引号包住。
- `profile` 只能来自启动参数，不能放进 JSON args。

`init` 命令读取当前 profile 库中 `category = init` 且 `status != trashed` 的记忆，只输出 `title_norm` 和 `content`：

```json
[
  {
    "title_norm": "agent bootstrap",
    "content": "初始化内容"
  }
]
```

正确示例：

```bash
mem012 --profile riko --args '{"tool":"create_memory","params":{"category":"core","title":"Profile 隔离规则","content":"profile 是数据库隔离边界。","summary":"profile 用于隔离数据库连接。","keywords":["profile"]}}'
```

错误示例：

```bash
mem012 --profile riko --args {"tool":"create_memory","params":{}}
```

## 3. 请求外壳

```json
{
  "tool": "create_memory",
  "params": {}
}
```

顶层规则：

- 只允许 `tool` 和 `params`。
- `tool` 必须是字符串。
- `params` 必须是 object。
- 当前合法 `tool` 是 `create_memory`、`delete_memory`、`read_memory`、`read_memory_hash`、`update_memory_replace`、`update_memory_patch_content`、`update_memory_append`、`update_memory_add_keywords` 或 `update_memory_remove_keywords`。

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
  "params": {
    "category": "core",
    "title": "Profile 隔离规则",
    "content": "profile 是数据库隔离边界，不进入搜索参数。",
    "summary": "profile 用于隔离数据库连接，Agent 搜索时不传 profile。",
    "keywords": ["profile", "数据库隔离", "搜索参数"],
    "recall_when": "当讨论多库隔离、profile、数据库连接选择时召回"
  }
}
```

字段要求：

- `title`、`content` 必填。
- 非 `init` 记忆的 `keywords` 必填。
- `category = init` 可省略 `keywords`；CLI 会自动确保最终包含 `init`。
- `category`、`summary`、`recall_when` 可选。

校验规则：

- `category` 未提供时默认为 `core`。
- `category` 必须来自 `[categories].index_list` 配置白名单，Agent 不能自造 category。
- `category = init` 用于初始化内容，写入前必须已有 `~/.auth/auth_file.mem`。
- 如果缺少 auth file，Agent 必须向用户申请授权；用户从 `/auth` 获取 token 后执行 `mem012 --auth <auth_token>`。
- 写入 `init` 的命令示例：

```bash
mem012 --profile riko --args '{"tool":"create_memory","params":{"category":"init","title":"标题","content":"正文","keywords":["init"]}}'
```

- `category = share` 只能在 `--profile share` 中使用。
- `title`、`content` 不能为空字符串。
- `summary` 如果省略或为空字符串，后端保存为 `null`。
- 非 `init` 记忆的 `keywords` 必须是非空字符串数组。
- `category = init` 可省略 `keywords` 或传空数组；如果提供元素，元素不能为空。
- `keywords` 规范化后不能重复。
- `recall_when` 如果提供，不能是空字符串。
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

## 6. 写入语义

`create_memory` 成功后会写入：

- `memory_units`
- `memory_keywords`
- `memory_changes`

其中：

- `memory_units.status = pending`，表示已写入但未批准。
- `memory_changes` 是用户二次确认记录。
- `memory_changes.before_state` 为 `null`。
- `memory_changes.after_state` 保存完整工作态快照。
- `memory_uuid` 由后端生成。
- `title_norm` 由数据库 `normalize_title(text)` 生成。

`create_memory` 不应让 pending 记忆进入正式召回或 AGE 图谱；approve 后变为 `active` 再参与正式查询。

## 7. delete_memory

删除一条记忆。调用成功后，记忆进入 `trashed`，后续网页/API 继续使用同一个 `memory_uuid` 批准删除。

请求：

```json
{
  "tool": "delete_memory",
  "params": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "expected_revision": 2
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
    "action": "delete",
    "result": "trashed"
  },
  "error": null,
  "profile": "riko"
}
```

## 9. 非目标

CLI Memory 不提供：

- memory 搜索
- change approve / reject
- graph status
- graph rebuild
- graph neighbors
- relation 增删改

这些能力暂时由 HTTP API 承担。
