---
name: mem012-memory-skill
description: Use when creating, searching, deleting, reading hashes, authorizing init writes, or updating mem012 memories through the CLI. This skill gives exact mem012 commands and JSON request shapes for create_memory, search_memory, delete_memory, read_memory_hash, and update_memory_* tools.
---

# Mem012 CLI

## init -- 初始化 -- 找回自己

```bash
mem012 --profile {profile} init
```

## read_memory -- 读取记忆

仅用 `memory_uuid` 读取一条记忆的完整当前工作态。

```bash
mem012 --profile {profile} --args '{"tool":"read_memory","params":{"memory_uuid":"{memory_uuid}"}}'
```

## create_memory - 创建记忆

- 准备必填字段 `category`、`title`、`content`、`keywords`。可选字段是 `summary`、`recall_when`。
- keywords为数组可以填写多个，但是简洁为主。

```bash
mem012 --profile {profile} --args '{"tool":"create_memory","params":{"category":"core","title":"标题","content":"正文","summary":"摘要","keywords":["关键词"]}}'
```

## search_memory -- 搜索记忆

### 基础搜索

只传 `query` 和可选 `limit`。

```bash
mem012 --profile {profile} --args '{"tool":"search_memory","params":{"query":"关键词","limit": n}}'
```

### 高级搜索

必须同时传 `terms` 和 `filters`，不传 `query`。高级搜索先做 `terms` + `filters` 字面筛选，再用 `terms.include` 作为语义输入执行 embedding fallback 或 rerank。

```bash
mem012 --profile {profile} --args '{"tool":"search_memory","params":{"limit":n,"terms":{"include":["word1","word2"],"exclude":["word3"]},"filters":["summary","keywords","content","recall_when"]}}'
```

- 可用 `filters` 只有 `title`、`summary`、`keywords`、`content`、`recall_when`。
- `terms.include`、`terms.exclude` 必须同时传数组，且至少一个数组非空。`include` 表示至少命中一个，`exclude` 表示全部不能命中。

## delete_memory -- 删除记忆

首先read_memory 记忆，确定是正确的uuid，然后执行删除。

```bash
mem012 --profile {profile} --args '{"tool":"delete_memory","params":{"memory_uuid":"{memory_uuid}"}}'
```

## read_memory_hash

更新记忆之前先调用 `read_memory_hash`，拿到对应字段 hash。

```bash
mem012 --profile {profile} --args '{"tool":"read_memory_hash","params":{"memory_uuid":"{memory_uuid}"}}'
```

## update_memory -- 更新记忆

### 更新 `content`。

整段替换用 `update_memory_replace`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_replace","params":{"memory_uuid":"{memory_uuid}","expected_content_hash":"{content_hash}","new_content":"新的完整正文"}}'
```

替换唯一片段或模拟中间插入用 `update_memory_patch_content`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_patch_content","params":{"memory_uuid":"{memory_uuid}","expected_content_hash":"{content_hash}","match_content":"旧片段","replace_content":"新片段"}}'
```

末尾追加用 `update_memory_append`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_append","params":{"memory_uuid":"{memory_uuid}","expected_content_hash":"{content_hash}","append_content":"追加正文"}}'
```

### 更新 `keywords`。

增加关键词用 `update_memory_add_keywords`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_add_keywords","params":{"memory_uuid":"{memory_uuid}","expected_keywords_hash":"{keywords_hash}","keywords":["新关键词"]}}'
```

删除关键词用 `update_memory_remove_keywords`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_remove_keywords","params":{"memory_uuid":"{memory_uuid}","expected_keywords_hash":"{keywords_hash}","keywords":["旧关键词"]}}'
```

### 更新 `recall_when`。

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_append","params":{"memory_uuid":"{memory_uuid}","expected_recall_when_hash":"{recall_when_hash}","append_recall_when":"追加召回条件"}}'
```

### 更新 `summary`。

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_replace","params":{"memory_uuid":"{memory_uuid}","expected_summary_hash":"{summary_hash}","new_summary":"新的摘要"}}'
```
