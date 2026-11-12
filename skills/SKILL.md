---
name: mem012-memory-skill
description: Use when creating, backing up, importing, searching, deleting, reading hashes, or updating mem012 memories through the CLI. This skill gives exact mem012 commands and JSON request shapes for backup_memory, import_memory, create_memory, search_memory, delete_memory, read_memory_hash, and update_memory_* tools.
---

# Mem012 CLI SKILL

## 执行门禁
- 新 profile 必须先由 `mem012 --create_profile {profile}` 创建；日常 `init` 和 `--args` 不执行 DDL。
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

## init -- 初始化 -- 找回自己

```bash
mem012 --profile {profile} init
```

## create_profile -- 创建新 profile

不主动执行，只有用户主动要求才可调用create_profile

```bash
MEM012_ADMIN_DATABASE_URL="postgresql://{admin_user}:{admin_password}@{host}:{port}/postgres" mem012 --create_profile {profile}
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

只使用统一聚合搜索；必须同时提供自然语言 `query` 和少量有把握的 `terms.include`。该工具只返回候选，不读取、更新、删除或审核记忆；需要确认候选完整内容时，先用 `read_memory`。

### 聚合搜索

请求只接受 `query`、`terms`、`filters`、`limit`。`query` trim 后必须非空，用于向量召回和统一 rerank，不解析其中的 AND/OR。`terms.include` 必须是至少一个非空字符串的数组；`terms.exclude` 可省略，默认空数组。关键词条件只约束关键词召回路，向量候选不要求包含 include。

```bash
mem012 --profile {profile} --args '{"tool":"search_memory","params":{"query":"想查找的内容或方法","limit":5,"terms":{"include":["确定的关键词"],"exclude":[]},"filters":["summary","keywords","content","recall_when"]}}'
```

- 可用 `filters` 只有 `title`、`summary`、`keywords`、`content`、`recall_when`。
- `filters` 可省略或为空数组，默认五个字段；不能为 null。`limit` 可省略或为 null，否则必须是正整数，超过后端上限按上限返回。
- 关键词路以不区分大小写的子串匹配要求全部 include 命中，exclude 任一命中即排除；关键词可以分散在选定字段中。
- 不提供 `any` 或 `exact` 参数；未知字段、非法类型和空白关键词在检索及模型调用前拒绝。
- 两路独立并行召回后按 `memory_uuid` 去重，再统一进入 rerank；只有合并后至少两条候选时才调用 rerank，零条或单条直接返回。
- rerank 使用选定字段的完整原文；长文按冻结 tokenizer 切成 query/document pair 不超过 `8192` token 的重叠块，每批最多 `40` 块、累计 pair token 不超过 `32768`。每条记忆取最高块分数，再过滤、排序并应用最终 `limit`；块数不占用结果名额。
- 任一 rerank 批次请求失败、评分缺失/重复/越界/非有限，或 query 耗尽预算时，整次 rerank 回退，不用截断证据或部分评分冒充成功。
- 搜索响应不含 `data.strategy`、`embedding_fallback` 或 `rerank`。`state=success,error=null` 为正常结果；`error` 非 null 表示单路召回失败或 rerank 回退，错误码按关键词、向量、rerank 顺序排列。两路召回都失败时返回 `state=error`。
- rerank 回退仍返回两路合并候选，按关键词第 1 条、向量第 1 条交替取值，跳过重复 UUID，最后应用 `limit`；回退结果不代表已确认相关性，不能直接执行后续写操作。

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
