---
name: mem012-cli
description: Use when creating or deleting mem012 memories through the CLI with create_memory or delete_memory. This skill gives exact mem012 --profile commands and JSON request shapes only for the current two supported tools.
---

# Mem012 CLI

## create_memory

1. 判断是否需要新增记忆。
2. 准备 `category`、`title`、`content`、`keywords`。
3. 可选准备 `summary`、`recall_when`。
4. 用目标库名替换 `riko`。
5. 执行：

```bash
mem012 --profile riko --args '{"tool":"create_memory","params":{"category":"core","title":"标题","content":"正文","summary":"摘要","keywords":["关键词"]}}'
```

6. 成功后记录返回的 `memory_uuid`。

## delete_memory

1. 确认要删除的 `memory_uuid`。
2. 用目标库名替换 `riko`。
3. 执行：

```bash
mem012 --profile riko --args '{"tool":"delete_memory","params":{"memory_uuid":"8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00"}}'
```

4. 成功后继续使用同一个 `memory_uuid` 执行批准或撤销。
