---
name: mem012-memory-skill
description: Use when creating, backing up, importing, searching, deleting, reading hashes, or updating mem012 memories through the CLI. This skill gives exact mem012 commands and JSON request shapes for backup_memory, import_memory, create_memory, search_memory, delete_memory, read_memory_hash, and update_memory_* tools.
---

# Mem012 CLI SKILL

## 执行门禁
- 首次使用某个 profile 或升级后迁移 schema，先执行 `mem012 --profile {profile} dbsetup`。
- `mem012 init` 只读取初始化记忆，直接读取输出内容；`mem012 --args` 工具调用必须按 JSON 判断结果。
- 工具调用成功条件：命令退出码为 0，且 JSON 中 `state == "success"`、`error == null`。否则一律视为失败。
- 任一步失败，立即停止并报告用户；禁止继续执行后续写操作，禁止伪造成功结果。
- 写操作必须单步执行；一次命令只能包含一个 `create_memory`、`import_memory`、`delete_memory` 或 `update_memory_*` 工具。
- `delete_memory` 前：`read_memory` 确认目标，`read_memory_hash` 取 `revision`。
- `update_memory_*` 前：`read_memory_hash` 取同一次 `revision + hash`。
- `search_memory` 只返回候选；不能直接把第一条当最终目标，必须再用 `read_memory` 核对。
- `create_memory` 成功以 `data.memory_uuid` 为准，`data.result == "pending"` 表示等待后续处理。
- `import_memory` 成功以 `data.memory_uuids` 和 `data.count` 为准，`data.result == "imported"` 表示已经写入 active memory。
- `update_memory_*` 成功通常返回 `data.result == "pending_review"`，不表示已经确认通过。
- `delete_memory` 成功返回 `data.result == "trashed"`。
- revision/hash 失效时停止，重新 `read_memory_hash`。

## dbsetup -- 数据库 schema 初始化

```bash
mem012 --profile {profile} dbsetup
```

## init -- 初始化 -- 找回自己

```bash
mem012 --profile {profile} init
```

## auth -- 授权 init 写入

仅当用户明确提供`auth_token` 后执行。`--auth` 必须同时带 `--profile`；成功后会写入本机短期授权文件 `~/.auth/auth_file.mem`。

```bash
mem012 --profile {profile} --auth {auth_token}
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
mem012 --profile {profile} --args '{"tool":"search_memory","params":{"query":"关键词","limit":10}}'
```

### 高级搜索

必须同时传 `terms` 和 `filters`，不传 `query`。高级搜索先做 `terms` + `filters` 字面筛选，再用 `terms.include` 作为语义输入执行 embedding fallback 或 rerank。

```bash
mem012 --profile {profile} --args '{"tool":"search_memory","params":{"limit":10,"terms":{"include":["word1","word2"],"exclude":["word3"]},"filters":["summary","keywords","content","recall_when"]}}'
```

- 可用 `filters` 只有 `title`、`summary`、`keywords`、`content`、`recall_when`。
- `terms.include`、`terms.exclude` 必须同时传数组，且至少一个数组非空。`include` 表示至少命中一个，`exclude` 表示全部不能命中。

## delete_memory -- 删除记忆

先 `read_memory` 确认目标，再 `read_memory_hash` 取 `revision`。

```bash
mem012 --profile {profile} --args '{"tool":"delete_memory","params":{"memory_uuid":"{memory_uuid}","expected_revision":{revision}}}'
```

## read_memory_hash

返回 `data.revision` 和 `data.hash.*`。

```bash
mem012 --profile {profile} --args '{"tool":"read_memory_hash","params":{"memory_uuid":"{memory_uuid}"}}'
```

## update_memory -- 更新记忆

### 更新 `content`。

整段替换用 `update_memory_replace`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_replace","params":{"memory_uuid":"{memory_uuid}","expected_revision":{revision},"expected_content_hash":"{content_hash}","new_content":"新的完整正文"}}'
```

替换唯一片段或模拟中间插入用 `update_memory_patch_content`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_patch_content","params":{"memory_uuid":"{memory_uuid}","expected_revision":{revision},"expected_content_hash":"{content_hash}","match_content":"旧片段","replace_content":"新片段"}}'
```

末尾追加用 `update_memory_append`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_append","params":{"memory_uuid":"{memory_uuid}","expected_revision":{revision},"expected_content_hash":"{content_hash}","append_content":"追加正文"}}'
```

### 更新 `keywords`。

增加关键词用 `update_memory_add_keywords`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_add_keywords","params":{"memory_uuid":"{memory_uuid}","expected_revision":{revision},"expected_keywords_hash":"{keywords_hash}","keywords":["新关键词"]}}'
```

删除关键词用 `update_memory_remove_keywords`：

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_remove_keywords","params":{"memory_uuid":"{memory_uuid}","expected_revision":{revision},"expected_keywords_hash":"{keywords_hash}","keywords":["旧关键词"]}}'
```

### 更新 `recall_when`。

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_append","params":{"memory_uuid":"{memory_uuid}","expected_revision":{revision},"expected_recall_when_hash":"{recall_when_hash}","append_recall_when":"追加召回条件"}}'
```

### 更新 `summary`。

只能用 `update_memory_replace` 整段替换；不能追加、插入或局部 patch。

```bash
mem012 --profile {profile} --args '{"tool":"update_memory_replace","params":{"memory_uuid":"{memory_uuid}","expected_revision":{revision},"expected_summary_hash":"{summary_hash}","new_summary":"新的摘要"}}'
```

## backup_memory -- 备份记忆

仅当用户明确要求备份时使用。

只使用 `--args` 工具入口，并且必须传 `params.output_path`。`output_path` 可以是文件路径；如果是已存在目录，会写入该目录下的 `backup.json`。

```bash
mem012 --profile {profile} --args '{"tool":"backup_memory","params":{"output_path":"backup.json"}}'
```

## import_memory -- 导入记忆

仅当用户明确要求导入时使用。

只使用 `--args` 工具入口，并且必须传 `params.input_path`。支持导入 `backup_memory` 生成的完整备份 JSON；每条 memory 必须是 `active`。本工具会写入 memory 主体和 keywords，并刷新搜索索引；当前会读取但不恢复 relations，成功响应中 `data.relations_imported` 为 `0`。

```bash
mem012 --profile {profile} --args '{"tool":"import_memory","params":{"input_path":"backup.json"}}'
```
