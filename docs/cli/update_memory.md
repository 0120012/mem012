# Update Memory

## 1. 目标

更新记忆对 Agent 暴露 1 个读取工具和 5 个更新工具；后端内部必须走同一个更新事务。

当前状态：`read_memory_hash`、`update_memory_replace`、`update_memory_patch_content`、`update_memory_append`、`update_memory_add_keywords` 和 `update_memory_remove_keywords` 已接入 Rust CLI。

```text
read_memory_hash        = 更新前读取目标身份、revision 和字段 hash
update_memory_replace   = 替换整个字段
update_memory_patch_content = 替换 content 中唯一匹配片段
update_memory_append    = 追加 content / recall_when
update_memory_add_keywords = 增加 keywords
update_memory_remove_keywords = 删除 keywords
```

后端统一入口：

```text
apply_memory_update
```

结果规则：

```text
一次用户意图只生成一条 memory_changes
```

## 2. 目标确认

`title` 只能用于搜索候选，不能直接执行更新。

执行任意更新工具前必须完成：

```text
1. 用 title 搜索候选
2. 展示 memory_uuid、title_norm
3. 用户确认具体目标
4. 调用 read_memory_hash
5. 展示返回的 title_norm 做最后确认
6. 拿到 revision 和字段 hash
7. 调用更新工具
```

如果用户直接提供 `memory_uuid`，仍然要先调用 `read_memory_hash` 并拿到 revision 和字段 hash。

## 3. read_memory_hash

用途：读取目标记忆的轻量身份摘要和当前版本指纹。

请求：

```json
{
  "tool": "read_memory_hash",
  "params": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00"
  }
}
```

成功响应：

```json
{
  "state": "success",
  "tool": "read_memory_hash",
  "data": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "title_norm": "profile 隔离规则",
    "revision": 2,
    "hash": {
      "state_hash": "0x...",
      "title_hash": "0x...",
      "content_hash": "0x...",
      "summary_hash": "0x...",
      "recall_when_hash": "0x...",
      "category_hash": "0x...",
      "keywords_hash": "0x..."
    }
  },
  "error": null,
  "profile": "riko"
}
```

规则：

- 所有 hash 都由后端计算。
- `revision` 是 `memory_units` 当前工作态行版本。
- `hash.state_hash` 基于完整 state。
- 字段 hash 只用于对应字段更新前校验。
- `read_memory_hash` 不返回完整 `content`。
- `read_memory_hash` 不返回 `summary` 正文，只返回 `hash.summary_hash`。
- Agent 只能原样转交 hash，不能自己生成。

## 4. 通用参数

每个更新工具都必须带 `memory_uuid`、`expected_revision` 和本次修改字段对应的 `expected_*_hash`：

```json
{
  "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
  "expected_revision": 2,
  "expected_title_hash": "read_memory_hash 返回的 hash.title_hash"
}
```

规则：

- `memory_uuid` 必须来自用户确认后的精确读取结果。
- `expected_revision` 必须来自同一次 `read_memory_hash`。
- `expected_*_hash` 必须来自同一次 `read_memory_hash`。
- 更新哪个字段，就必须带哪个字段的 `expected_*_hash`。
- 后端先校验当前 revision，不一致时拒绝更新。
- 后端重新计算当前字段 hash，不一致时拒绝更新。
- `memory_units.status = trashed` 时拒绝更新。
- 不能从 search/list 结果里直接拿 uuid 更新。
- 候选超过一条时必须让用户选择。

## 5. update_memory_replace

用途：替换整个字段。

后端通过传入的参数判断替换类型，Agent 不需要传 `mode` / `field`。

```json
{
  "tool": "update_memory_replace",
  "params": {
    "memory_uuid": "xxx",
    "expected_revision": 2,
    "expected_title_hash": "0x...",
    "new_title": "新标题"
  }
}
```

可用参数：

```text
new_title
new_summary
new_recall_when
new_category
new_content
```

规则：

- 可以一次提交多个 `new_*` 字段，后端按固定字段顺序处理。
- `new_title`、`new_content`、`new_summary` 不能是空字符串。
- `new_recall_when` 可以是 `null`，表示清空。
- 每个 `new_*` 字段都必须有对应的 `expected_*_hash`。

替换整个 content：

```json
{
  "tool": "update_memory_replace",
  "params": {
    "memory_uuid": "xxx",
    "expected_revision": 2,
    "expected_content_hash": "0x...",
    "new_content": "新的完整正文"
  }
}
```

## 6. update_memory_patch_content

替换 `content` 中唯一匹配的文本片段。

```json
{
  "tool": "update_memory_patch_content",
  "params": {
    "memory_uuid": "xxx",
    "expected_revision": 2,
    "expected_content_hash": "0x...",
    "match_content": "旧文本",
    "replace_content": "新文本"
  }
}
```

规则：

- `match_content` 必须在当前 `content` 中出现一次。
- `match_content` 找不到时拒绝。
- `match_content` 出现多次时拒绝。
- `replace_content` 不能为空。
- 如果需要同时修改摘要，另行调用 `update_memory_replace`。
- 不支持模糊匹配。

## 7. update_memory_append

用途：对允许追加的文本字段追加内容。

追加 content：

```json
{
  "tool": "update_memory_append",
  "params": {
    "memory_uuid": "xxx",
    "expected_revision": 2,
    "expected_content_hash": "0x...",
    "append_content": "\n\n补充内容"
  }
}
```

追加 recall_when：

```json
{
  "tool": "update_memory_append",
  "params": {
    "memory_uuid": "xxx",
    "expected_revision": 2,
    "expected_recall_when_hash": "0x...",
    "append_recall_when": "；当讨论更新记忆时召回"
  }
}
```

规则：

- 只允许 `append_content` 或 `append_recall_when`。
- 每次只能执行一种追加意图。
- 追加内容不能为空。
- 如果需要同步修改摘要，另行调用 `update_memory_replace`。

## 8. update_memory_add_keywords

用途：增加关键词。

```json
{
  "tool": "update_memory_add_keywords",
  "params": {
    "memory_uuid": "xxx",
    "expected_revision": 2,
    "expected_keywords_hash": "0x...",
    "keywords": ["新关键词"]
  }
}
```

规则：

- `keywords` 必须是非空字符串数组。
- 规范化后不能和已有关键词重复。

## 9. update_memory_remove_keywords

用途：删除关键词。

```json
{
  "tool": "update_memory_remove_keywords",
  "params": {
    "memory_uuid": "xxx",
    "expected_revision": 2,
    "expected_keywords_hash": "0x...",
    "keywords": ["旧关键词"]
  }
}
```

规则：

- `keywords` 必须是非空字符串数组。
- 要删除的关键词必须存在。
- 最终 `keywords` 必须非空。
- 不提供整组替换工具；需要改名时先删除旧关键词，再增加新关键词。

## 10. 统一后端事务

所有更新工具内部都进入同一个流程：

```text
1. 开启事务
2. 锁定 memory_units
3. 读取当前完整 state
4. 校验 expected_revision
5. 校验 expected_*_hash
6. 分类 pending / active / existing change
7. 应用工具动作，生成 next_state
8. 校验 next_state
9. 写回 memory_units / memory_keywords
10. 回读 after_state
11. 写入或覆盖 memory_changes
12. active 记忆标记 graph dirty
13. 提交事务
```

如果最终 state 没有变化，返回 `NO_CHANGE`，不写 `memory_changes`。

## 11. change 规则

pending create：

```text
保持 memory_changes.action = create
只覆盖 after_state
不标记 graph dirty
```

active 且没有 open change：

```text
保存 before_state
写入 memory_changes.action = update
标记 graph dirty
approve 后刷新 embedding 并覆盖旧 embedding
```

active 且已有 update / restore：

```text
保留已有 before_state
只覆盖 after_state
标记 graph dirty
approve 后刷新 embedding 并覆盖旧 embedding
```

已有 delete：

```text
拒绝更新
```

## 12. 成功响应

```json
{
  "state": "success",
  "tool": "update_memory_append",
  "data": {
    "memory_uuid": "xxx",
    "action": "update",
    "result": "pending_review",
    "updated_fields": ["content"]
  },
  "error": null,
  "profile": "riko"
}
```

`action` 返回当前 `memory_changes.action`，所以 pending create 场景可能返回 `create`。

## 13. Agent 规则

```text
不能直接用 search/list 返回的 uuid 更新
不能按搜索排序自动选择第一条
不能通过 title 直接执行更新
必须先确认目标 memory_uuid
必须带 expected_revision
必须带 expected_*_hash
expected_revision 和 expected_*_hash 必须来自同一次 read_memory_hash
需要改摘要时使用 update_memory_replace
```

## 14. 非目标

- 批量更新
- 修改 relation
- 修改 usage
- 直接刷新 embedding
- 直接 approve / reject
- 按字符下标插入
- 模糊替换
