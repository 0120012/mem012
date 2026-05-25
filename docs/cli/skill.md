---
name: mem012-cli
description: Use when creating, searching, deleting, reading hashes, or updating mem012 memories through the CLI. This skill gives exact mem012 --profile commands and JSON request shapes for create_memory, search_memory, delete_memory, read_memory_hash, and update_memory_* tools.
---

# Mem012 CLI

## create_memory

1. 用 `create_memory` 创建一条新记忆。创建后返回 `memory_uuid`，结果为 `pending`，后续需要走批准或撤销流程。
2. 准备必填字段 `category`、`title`、`content`、`keywords`。可选字段是 `summary`、`recall_when`。
3. 用目标库名替换 `{profile}` 后执行：

```bash
mem012 --profile {profile} --args '{"tool":"create_memory","params":{"category":"core","title":"标题","content":"正文","summary":"摘要","keywords":["关键词"]}}'
```

4. 成功后记录返回的 `memory_uuid`。
5. 先确认返回结果里 `state` 是 `success`，并且 `data.memory_uuid` 存在。然后用返回的 `memory_uuid` 回读 hash：

```bash
mem012 --profile {profile} --args '{"tool":"read_memory_hash","params":{"memory_uuid":"{memory_uuid}"}}'
```

6. 如果回读返回 `state: success`，并且能看到同一个 `memory_uuid`、`title_norm` 和各字段 hash，说明创建已写入成功。

## search_memory

1. 基础搜索只传 `query` 和可选 `limit`。搜索会排除 `trashed`，返回 `memory_uuid`、`title_norm`、`status`、`summary`、`content_preview`、`matched_fields` 和 `strategy`。

```bash
mem012 --profile riko --args '{"tool":"search_memory","params":{"query":"李白","limit":8}}'
```

2. `query` 必填，去除首尾空白后不能为空。`limit` 必须大于 0；不传或超过配置默认值时，使用 `[search].default_limit`。
3. 高级搜索必须同时传 `terms` 和 `filters`。`terms = {}` 允许，表示没有硬性关键词条件；`filters = []` 允许，表示不限制字段。

```bash
mem012 --profile riko --args '{"tool":"search_memory","params":{"query":"饮酒 月亮","limit":8,"terms":{"any":["饮酒","月亮"],"none":["送别"]},"filters":["summary","keywords","content","recall_when"]}}'
```

4. 可用 `filters` 只有 `title`、`summary`、`keywords`、`content`、`recall_when`。不要传 `status`、`category`、`embedding` 或 `rerank`。
5. `terms.all` 表示必须全部命中，`terms.any` 表示至少命中一个，`terms.none` 表示必须不命中。`query` 不能写非法 `AND/OR` 逻辑，关键词逻辑放进 `terms`。
6. `strategy.embedding_fallback` 表示字面召回为空后启用了 embedding 保底；`strategy.rerank` 表示结果经过 rerank。`score` 只在 rerank 结果中返回。

## delete_memory

1. 确认要删除的 `memory_uuid`。
2. 用目标库名替换 `riko`。
3. 执行：

```bash
mem012 --profile riko --args '{"tool":"delete_memory","params":{"memory_uuid":"{memory_uuid}"}}'
```

4. 成功后继续使用同一个 `memory_uuid` 执行批准或撤销。

## update_memory

1. 更新记忆之前先调用 `read_memory_hash`，拿到对应字段 hash。

```bash
mem012 --profile riko --args '{"tool":"read_memory_hash","params":{"memory_uuid":"{memory_uuid}"}}'
```

2. 更新 `content`。

整段替换用 `update_memory_replace`：

```bash
mem012 --profile riko --args '{"tool":"update_memory_replace","params":{"memory_uuid":"{memory_uuid}","expected_content_hash":"{content_hash}","new_content":"新的完整正文"}}'
```

替换唯一片段或模拟中间插入用 `update_memory_patch_content`：

```bash
mem012 --profile riko --args '{"tool":"update_memory_patch_content","params":{"memory_uuid":"{memory_uuid}","expected_content_hash":"{content_hash}","match_content":"旧片段","replace_content":"新片段"}}'
```

末尾追加用 `update_memory_append`：

```bash
mem012 --profile riko --args '{"tool":"update_memory_append","params":{"memory_uuid":"{memory_uuid}","expected_content_hash":"{content_hash}","append_content":" 追加正文"}}'
```

3. 更新 `keywords`。

增加关键词用 `update_memory_add_keywords`：

```bash
mem012 --profile riko --args '{"tool":"update_memory_add_keywords","params":{"memory_uuid":"{memory_uuid}","expected_keywords_hash":"{keywords_hash}","keywords":["新关键词"]}}'
```

删除关键词用 `update_memory_remove_keywords`：

```bash
mem012 --profile riko --args '{"tool":"update_memory_remove_keywords","params":{"memory_uuid":"{memory_uuid}","expected_keywords_hash":"{keywords_hash}","keywords":["旧关键词"]}}'
```

4. 更新 `recall_when`。

```bash
mem012 --profile riko --args '{"tool":"update_memory_append","params":{"memory_uuid":"{memory_uuid}","expected_recall_when_hash":"{recall_when_hash}","append_recall_when":" 追加召回条件"}}'
```

5. 更新 `summary`。

```bash
mem012 --profile riko --args '{"tool":"update_memory_replace","params":{"memory_uuid":"{memory_uuid}","expected_summary_hash":"{summary_hash}","new_summary":"新的摘要"}}'
```
