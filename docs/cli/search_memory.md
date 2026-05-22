# Search Memory

## 1. 目标

`search_memory` 用于先搜索候选记忆，再把明确的 `memory_uuid` 交给 `read_memory`、`read_memory_hash`、`update_memory_*` 或 `delete_memory`。

核心原则：

```text
search_memory = 找候选 memory_uuid
read_memory = 读取完整工作态
read_memory_hash = 更新前读取字段 hash
update_memory_* = 依赖 memory_uuid 和 expected_*_hash 写入
```

`search_memory` 不写入 `memory_units`、`memory_keywords`、`memory_changes`，不刷新 embedding，也不标记 graph dirty。

## 2. 搜索边界

第一版搜索只使用当前 PostgreSQL 数据：

```text
memory_units.title_norm
memory_units.summary
memory_units.content
memory_units.recall_when
memory_units.category
memory_units.status
memory_keywords.keyword_norm
memory_embeddings.embedding
```

默认用途是帮助 Agent 找到可读、可改、可删的候选记忆：

```text
默认状态范围 = pending + active
永远排除 = trashed
```

如果搜索用于正式召回，可以在高级搜索里只请求 `active`。

Agent 搜索支持字段：

```text
title
summary
keywords
content
recall_when
category
status
```

`title`、`summary`、`keywords`、`content`、`recall_when`、`category` 是精准召回主路径；`status` 用于过滤候选范围。embedding 只做保底召回：当字面字段命中不足时补候选，不作为直接更新、删除或自动选择记忆的依据。

rerank 模型是可选重排阶段，由 `[rerank].enabled` 控制。关闭时只按基础召回分排序；开启时使用 `[rerank].rerank_api`、`[rerank].rerank_model` 对候选集做重排，但不扩大召回范围，不替代用户确认。

`[rerank].rerank_api` 和 `embeddings_api` 使用同一条规则：值为 `local` 表示本地模型入口；否则必须填写 URL。

rerank 模型候选：

```text
Qwen/Qwen3-Reranker-4B      = 默认建议，输入倍率 5x，补全倍率 1x
Qwen/Qwen3-Reranker-8B      = 质量优先备选，输入倍率 10x，补全倍率 1x
BAAI/bge-reranker-v2-m3    = 开源权重备选，输入倍率 5x，补全倍率 1x，窗口 512
```

## 3. 基础搜索

基础搜索只需要传 `query`：

```json
{
  "tool": "search_memory",
  "params": {
    "query": "尼采 深渊"
  }
}
```

规则：

- `query` 必填且不能为空。
- `limit` 可选；不传时使用 `[search].default_limit`。
- `limit` 必须在 `1` 到 `20` 之间。
- 默认搜索 `pending + active`，排除 `trashed`。
- 默认搜索 `title`、`summary`、`keywords`、`content`、`recall_when`、`category`，并按 `statuses` 过滤 `status`。
- embedding 只在精准字段命中不足时保底补候选。
- 如果 `[rerank].enabled = true`，返回前对候选结果执行重排。

## 4. 高级搜索

高级搜索用于明确控制状态范围、搜索字段和是否返回片段：

```json
{
  "tool": "search_memory",
  "params": {
    "query": "尼采 深渊",
    "mode": "advanced",
    "limit": 8,
    "statuses": ["active", "pending"],
    "fields": ["title", "summary", "keywords", "content", "recall_when", "category", "status"],
    "require_all_terms": false,
    "include_snippets": true
  }
}
```

可用参数：

```text
mode = basic / advanced，默认 basic
statuses = ["active"] 或 ["active", "pending"]，默认 ["active", "pending"]
fields = title / summary / keywords / content / recall_when / category / status 的非空子集
require_all_terms = query 分词是否必须全部命中，默认 false
include_snippets = 是否返回命中文本片段，默认 false
limit = 返回条数，不传时使用 [search].default_limit
```

禁止请求 `trashed`。如果要看已删除记忆，应走专门的审计或恢复工具，不能通过搜索绕过删除边界。

字段说明：

```text
title       = memory_units.title_norm
summary     = memory_units.summary
keywords    = memory_keywords.keyword_norm
content     = memory_units.content
recall_when = memory_units.recall_when
category    = memory_units.category
status      = memory_units.status，只做状态过滤
embedding   = 保底召回，不在 fields 里显式请求
rerank      = 可选重排，不在 fields 里显式请求
rerank.rerank_api = local 或 rerank API URL
rerank.rerank_model = 配置项，只在 rerank.enabled = true 时使用
rerank.rerank_key = rerank API key；rerank_api = local 时可为空
```

## 5. 成功响应

```json
{
  "state": "success",
  "tool": "search_memory",
  "data": {
    "query": "尼采 深渊",
    "mode": "basic",
    "limit": 8,
    "statuses": ["active", "pending"],
    "result_count": 1,
    "results": [
      {
        "memory_uuid": "{memory_uuid}",
        "title_norm": "尼采深渊凝视 approve flow test 20260521_01",
        "status": "active",
        "summary": "用于测试 approve 流程的哲学版本记忆，主题是尼采与深渊凝视。",
        "matched_fields": ["title", "summary", "keywords"],
        "fallback": false,
        "score": 87.5
      }
    ]
  },
  "error": null,
  "profile": "{profile}"
}
```

如果 `include_snippets = true`，单条结果可以增加：

```json
{
  "snippets": {
    "summary": "主题是尼采与深渊凝视",
    "content": "当讨论尼采、深渊凝视或存在主义时召回"
  }
}
```

规则：

- `results` 没有命中时返回空数组。
- `score` 只用于候选排序，不是用户确认。
- `matched_fields` 只说明命中来源，不能替代 `read_memory`。
- `fallback = true` 表示该候选来自 embedding 保底，必须回读确认。
- rerank 开启后只影响候选顺序，不改变候选可操作边界。
- `summary` 可以是 `null`。

## 6. 成功验证

执行后检查：

```text
1. state = success
2. tool = search_memory
3. data.query 等于请求 query 去除首尾空白后的值
4. data.results 是数组
5. 每条结果都有 memory_uuid、title_norm、status、matched_fields、fallback、score
6. 每条结果的 status 只能是 active 或 pending
7. 结果中不能出现 trashed memory
```

如果后续要更新记忆，不能直接使用搜索排序第一条。必须让用户确认目标，然后调用 `read_memory_hash` 获取字段 hash。

## 7. Agent 规则

```text
不能按搜索排序自动选择第一条
不能通过 title 直接执行 read/update/delete
必须展示候选 memory_uuid、title_norm、status
embedding 保底结果必须回读确认
用户确认后才能把 memory_uuid 交给 read/update/delete
更新前必须再调用 read_memory_hash
```

推荐流程：

```text
1. 调用 search_memory 找候选
2. 展示候选 memory_uuid、title_norm、status
3. 用户确认具体 memory_uuid
4. 读取内容时调用 read_memory
5. 更新内容时调用 read_memory_hash
6. 携带 expected_*_hash 调用 update_memory_*
```

## 8. 失败场景

```text
query 为空              = 拒绝
mode 非 basic/advanced  = 拒绝
statuses 包含 trashed   = 拒绝
statuses 为空           = 拒绝
fields 为空             = 拒绝
fields 包含未知字段     = 拒绝
limit 小于 1 或大于 20  = 拒绝
请求包含未知字段        = 拒绝
embedding 被显式请求     = 拒绝
```

## 9. 非目标

- 直接更新记忆
- 直接删除记忆
- approve / reject
- 搜索 `trashed` 记忆
- 跨 profile 搜索
- 跨 share 库合并搜索
- embedding 精准搜索
- 关系图扩展搜索
- 自动选择最相关结果
