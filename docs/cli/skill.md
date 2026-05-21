---
name: mem012-cli
description: Use when creating, deleting, reading hashes, or updating mem012 memories through the CLI. This skill gives exact mem012 --profile commands and JSON request shapes for create_memory, delete_memory, read_memory_hash, and update_memory_* tools.
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
