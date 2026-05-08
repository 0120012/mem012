# CLI V10 工具合同（草案）

> 状态：**Draft / 可评审**
>
> 目标：保留 V9 的单可执行程序、统一 `tool + args`、统一 JSON 响应外壳；删除 URI 工具语义，改为 `memory_uuid / handle / recall`。

## 0. 设计原则（V10）

- 只有一个可执行程序：`mem012`
- 只支持 CLI 单次调用模式
- shared core 只接收一种 canonical schema：`tool + args`
- 一次工具调用只做一件事：请求里只允许 1 个 `tool`
- `profile` 是数据库隔离边界，只能由启动参数选择，不允许进入工具 `args`
- 禁止 URI：不再接受 `domain://path`
- `handle` 是人类和 Agent 的快速定位索引，不是数据库主键
- 精确读取优先使用 `memory_uuid`
- Agent 不使用人类自由搜索语法作为主要召回方式；Agent 使用 `lookup_memory` 与 `recall_memory`
- 所有输出必须结构化：禁止返回自然语言成功串作为正式合同
- 前端 HTTP 不参与工具调用，前端只走资源型 API

## 1. 运行模式

唯一运行模式：

```bash
mem012 --profile riko --args '<json_object>'
```

规则：

- `--profile` 必填，且必须存在于配置里的数据库 profile
- `--args` 必填，且必须是 JSON object
- `--args.tool` 必填，且必须是已注册工具名
- `--args.args` 必填，且必须是 JSON object
- `--profile` 只选择本次进程使用的数据库 profile，不进入工具语义
- 不支持 `--config`
- 不支持默认 profile
- 不支持 MCP 模式

CLI 示例：

```bash
mem012 --profile riko --args '{"tool":"lookup_memory_by_handle","args":{"handle":"core/backend/database/profile隔离"}}'
```

## 2. 请求合同

统一 canonical schema：

```json
{
  "tool": "lookup_memory",
  "args": {
    "...tool_specific_fields": "..."
  }
}
```

通用规则：

- 顶层只允许 `tool` 与 `args`
- `args` 内未声明字段一律拒绝
- 所有写入工具必须显式提供必填字段
- 空字符串不等于未提供，必填字段为空字符串必须拒绝
- 所有工具都不得接收 `profile`
- 所有工具都不得接收 URI

非法示例：

```json
{"tool":"lookup_memory","args":{"uri":"core://agent"}}
```

合法示例：

```json
{"tool":"lookup_memory","args":{"memory_uuid":"8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00"}}
```

## 3. 响应外壳

成功响应：

```json
{
  "state": "success",
  "tool": "lookup_memory",
  "data": {},
  "error": null,
  "meta": {
    "spec_version": "v10",
    "profile": "riko"
  }
}
```

失败响应：

```json
{
  "state": "failed",
  "tool": "lookup_memory",
  "data": null,
  "error": {
    "code": "VALIDATE_MEMORY_UUID_REQUIRED",
    "message": "memory_uuid is required"
  },
  "meta": {
    "spec_version": "v10",
    "profile": "riko"
  }
}
```

约束：

- `state`、`tool`、`data`、`error`、`meta` 必须始终存在
- `state` 只能是 `success` 或 `failed`
- `meta.spec_version` 必须固定回显 `v10`
- 失败时禁止把正式错误塞进 `data`
- 成功时禁止把正式结果塞进自然语言字符串

## 4. Handle 规则

`handle` 用于快速定位重要记忆。

它的定位：

- 给人类直接告诉 Agent
- 给 Agent 在已知名字时快速定位
- 可以有多个 handle 指向同一条 memory
- 不是 `memory_units` 主键
- 最终仍解析为 `memory_uuid`

格式由后端固定支持，不放进 TOML 配置：

```text
category/channel_name/message_title
category/channel_name/subarea/message_title
core/backend/profile隔离
book/Thinking_in_Systems/reflections/chapter_1_system_basics
```

规则：

- handle 自身就是完整可读定位路径
- handle 允许可变层级，但数据库不把中间段拆表或建树
- 第一段是 category；不存在白名单时后端按 slug 校验
- 每一段都不能为空，`core//instance/...` 非法
- 后端只对完整 `handle_norm` 做唯一约束
- `lookup_memory_by_handle` 要求唯一命中
- 如果 handle 命中多条，说明唯一约束失效，应作为数据错误处理
- 人类自由搜索可以借用类似文本，但不保证唯一命中

## 5. 工具清单（V10）

V10 第一版工具：

1. `create_memory`
2. `lookup_memory`
3. `lookup_memory_by_handle`
4. `recall_memory`
5. `search_memory`
6. `patch_memory`
7. `update_memory_meta`
8. `delete_memory`
9. `link_memory`
10. `unlink_memory`
11. `list_categories`
12. `review_changes`
13. `rollback_change`

Why：V10 把能力分成三类：精确读取、上下文召回、写入维护。`search_memory` 保留给人类/管理端的自由组合搜索；Agent 正常应使用 `lookup_memory` 或 `recall_memory`。

## 6. 各工具最小请求形状

### create_memory

创建记忆。该工具立即写入当前工作态，并同时记录用户二次确认所需的 change。

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
    "exclude_when": "当讨论普通 category 分类时不要召回",
    "handle": "core/backend/database/profile隔离"
  }
}
```

语义：

- 本调用返回后，记忆已写入 `memory_units`，Agent 可立即读取
- `memory_keywords` 和 `memory_handles` 与 `memory_units` 同事务写入
- `memory_changes` 会新增同一 `memory_uuid` 的 `create` 记录，表示等待用户二次确认
- `memory_uuid` 由后端生成，调用方不得传入
- `title` 只作为输入字段，后端必须调用数据库 `normalize_title(text)` 得到 `title_norm`
- `memory_changes.before_state` 为 `null`
- `memory_changes.after_state` 保存当前完整工作态，包含 `title_norm`，不保存 `title`

必填：

- `title`
- `content`
- `summary`
- `keywords`

可选：

- `recall_when`
- `exclude_when`
- `handle`

规则：

- `category` 未提供时默认为 `core`
- `category` 必须是 slug，且不能是 `share`
- `keywords` 必须是非空字符串数组
- `handle` 如果提供，必须是非空路径字符串
- `handle` 第一段必须等于 `category`
- `args` 内禁止出现 `profile`、`memory_uuid`、`title_norm`、`uri`

成功响应：

```json
{
  "state": "success",
  "tool": "create_memory",
  "data": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "result": "pending_review"
  },
  "error": null,
  "meta": {
    "spec_version": "v10",
    "profile": "riko"
  }
}
```

### lookup_memory

按 `memory_uuid` 精确读取。

```json
{
  "tool": "lookup_memory",
  "args": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00"
  }
}
```

返回必须包含：

- memory 本体
- keywords
- handles
- relations
- review/version 状态摘要

### lookup_memory_by_handle

按 handle 精确定位。

```json
{
  "tool": "lookup_memory_by_handle",
  "args": {
    "handle": "core/backend/database/profile隔离"
  }
}
```

规则：

- 命中 0 条返回 `HANDLE_NOT_FOUND`
- 命中多条是数据错误，必须停止并报告
- 命中 1 条时返回同 `lookup_memory`
- 后续修改、删除、建立关系时应使用返回的 `memory_uuid`

### recall_memory

Agent 上下文召回。用于“不知道具体 memory_uuid，但需要想起相关记忆”的场景。

```json
{
  "tool": "recall_memory",
  "args": {
    "query": "Rust 重写时 profile 和 category 怎么区分",
    "category": "core",
    "context_text": "正在设计 Rust 记忆系统的配置和数据库结构。",
    "task_mode": "design",
    "limit": 8
  }
}
```

规则：

- `query` 必填
- `category` 可选；传入时作为硬过滤
- `context_text` 可选；用于语义召回和可选的 `recall_when` 判断
- `limit` 不得超过配置 `search.max_limit`
- 返回必须带 `why_hit`

返回项最小字段：

```json
{
  "memory_uuid": "...",
  "title": "...",
  "content": "...",
  "score": 0,
  "why_hit": ["keyword: profile", "category: core"],
  "matched_keywords": ["profile"],
  "matched_signals": ["keyword", "fulltext"],
  "related_memory_uuids": []
}
```

### search_memory

人类/管理端自由组合搜索。Agent 默认不应使用它做上下文召回。

```json
{
  "tool": "search_memory",
  "args": {
    "query": "profile 隔离",
    "category": "core",
    "keywords": ["profile"],
    "include_inactive": false,
    "limit": 20
  }
}
```

规则：

- `query` 可接受人类快速搜索语法，具体解析规则待定
- 不要求唯一命中
- 用于探索、管理、前端筛选
- 返回候选列表和命中信号
- 候选方向是 Discord 风格 filter，而不是 URI 路径

### patch_memory

正文补丁。只按 `memory_uuid` 修改，避免 handle 漂移导致误改。

```json
{
  "tool": "patch_memory",
  "args": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "old_string": "旧文本",
    "new_string": "新文本",
    "change_reason": "修正 profile 与 category 的边界描述"
  }
}
```

规则：

- `old_string` 必须唯一命中
- `new_string` 可以为空字符串，用于删除片段
- 修改前必须记录 version 和 review item

### update_memory_meta

更新非正文元数据。

```json
{
  "tool": "update_memory_meta",
  "args": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "title": "Profile 隔离规则",
    "summary": "profile 只决定数据库连接，不参与搜索语义。",
    "keywords": ["profile", "数据库隔离"],
    "handles": ["core/backend/database/profile隔离"],
    "recall_when": "当讨论 profile、多库、数据库隔离时召回",
    "exclude_when": "当只讨论 category 展示分类时不要召回",
    "change_reason": "同步 V10 术语"
  }
}
```

规则：

- 只允许修改提交的字段
- 不得修改 `content`
- 修改前必须记录 version 和 review item

### delete_memory

删除或废弃记忆。

```json
{
  "tool": "delete_memory",
  "args": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "mode": "deprecate",
    "change_reason": "已被新规则替代"
  }
}
```

规则：

- 默认 `mode = deprecate`
- 第一版不开放硬删除给 Agent
- 删除前必须记录 version 和 review item

### link_memory

建立图关系。

```json
{
  "tool": "link_memory",
  "args": {
    "from_memory_uuid": "...",
    "to_memory_uuid": "...",
    "relation_type": "depends_on",
    "weight": 80,
    "note": "A 的判断依赖 B"
  }
}
```

### unlink_memory

删除图关系。

```json
{
  "tool": "unlink_memory",
  "args": {
    "relation_uuid": "..."
  }
}
```

### list_categories

列出当前 profile 数据库内已出现过的 category。

```json
{
  "tool": "list_categories",
  "args": {}
}
```

### review_changes

列出待审查变更。

```json
{
  "tool": "review_changes",
  "args": {
    "limit": 20
  }
}
```

### rollback_change

撤销一条 review item 或一个 batch。

```json
{
  "tool": "rollback_change",
  "args": {
    "review_item_uuid": "..."
  }
}
```

或：

```json
{
  "tool": "rollback_change",
  "args": {
    "review_batch_uuid": "..."
  }
}
```

规则：

- `review_item_uuid` 与 `review_batch_uuid` 二选一
- rollback 必须产生新的 review 记录

## 7. 与数据库设计的边界

- `memory_uuid` 是唯一强身份
- `handle` 是快速定位索引，可以变更，可以有多个
- `keywords` 是主要召回入口
- `relations` 服务图扩展
- `summary` 服务语义索引；第一版语义输入暂时使用 `title + summary + content + keywords`
- review/version 是强制能力，不做配置开关

## 8. 发布约束（V10）

- 任一工具若仍接收 URI，禁止发布
- 任一工具若返回自然语言成功串而非固定 JSON 外壳，禁止发布
- 任一工具若在 CLI 与 MCP 间字段不一致，禁止发布
- 任一写工具若不能记录 version/review，禁止发布
- 任一 Agent 写工具若绕过撤销能力，禁止发布
