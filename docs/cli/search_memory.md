# Search Memory

## 1. 工具定位

`search_memory` 用于从记忆库中搜索候选记忆，返回可确认的 `memory_uuid`。

它只负责找候选，不负责读取、更新、删除或审核：

```text
search_memory    = 找候选 memory_uuid
read_memory      = 读取目标记忆完整内容
read_memory_hash = 更新前读取字段 hash
update_memory_*  = 带 expected_*_hash 写入变更
delete_memory    = 删除前仍依赖明确 memory_uuid
```

`search_memory` 不写入 `memory_units`、`memory_keywords`、`memory_changes`，不刷新 embedding，也不标记 graph dirty。

## 2. 高级筛选

基础搜索类似 GitHub 的默认搜索：只给 `query`，系统在全部可搜索内容里做模糊召回。

高级搜索类似 GitHub 左侧筛选面板：先用 `query` 找候选，再用 `filters` 收窄结果。

可用筛选：

```text
filters      = title / summary / keywords / content / recall_when 的数组
```

字段映射：

```text
title       = memory_units.title_norm
summary     = memory_units.summary
keywords    = memory_keywords.keyword_norm
content     = memory_units.content
recall_when = memory_units.recall_when
category    = memory_units.category
```

## 3. 状态边界

默认用途是帮助 Agent 找到可读、可改、可删的候选记忆：

```text
不开放状态搜索
不开放状态筛选
永远排除 trashed
```

结果可以返回 `status` 给 Agent 展示，但请求参数不能按状态过滤，也不能搜索状态文本。

## 4. 基础搜索

基础搜索是模糊搜索，只接受 `query` 和可选 `limit`：

```json
{
  "tool": "search_memory",
  "params": {
    "query": "尼采深渊凝视",
    "limit": 8
  }
}
```

规则：

- `query` 必填，去除首尾空白后不能为空。
- `limit` 可选；不传时使用 `[search].default_limit`。
- `limit` 必须大于 0。
- `limit` 超过 `[search].default_limit` 时，按 `[search].default_limit` 截断。
- 不传 `filters` 和 `terms` 时执行基础搜索。
- 结果永远排除 `trashed`。
- 字面搜索结果为 0 时，才允许 embedding 保底补候选。
- 如果 `[rerank].enabled = true`，返回前对候选集重排。

## 5. 高级搜索

高级搜索用于表达明确的字面约束和搜索范围。`query` 仍然必填，表示本次搜索的自然语言意图；`terms` 表示硬性关键词条件；`filters` 表示搜索字段范围。

高级搜索的 `query` 只能是自然语言文本，不能包含 `AND` 或 `OR`。关键词逻辑必须写入 `terms`。

```json
{
  "tool": "search_memory",
  "params": {
    "query": "微信读书导出笔记",
    "limit": 8,
    "terms": {
      "all": ["导出"],
      "none": ["失败", "报错"],
      "any": ["微信读书", "skill"]
    },
    "filters": ["title", "content"]
  }
}
```

参数：

```text
query             = 搜索文本，必填
terms             = 硬性关键词条件；高级搜索必填，可以为空对象
limit             = 返回条数；不传或超过 [search].default_limit 时使用 [search].default_limit
filters           = 搜索字段数组；高级搜索必填，可以为空数组
```

高级搜索触发规则：

```text
传 terms       = 必须同时传 filters
传 filters     = 必须同时传 terms
terms = {}     = 允许，表示没有硬性关键词条件
filters = []   = 允许，表示不限制搜索字段
```

可用 `filters`：

```text
title / summary / keywords / content / recall_when
```

可用 `terms`：

```text
all      = 必须包含 all
none     = 必须全部不包含
any      = 至少包含 any 之一
```

筛选语义：

```text
terms.all = ["导出"]
表示必须包含 all 中的全部关键词。

terms.none = ["失败", "报错"]
表示必须全部不包含 none 中的关键词。

terms.any = ["微信读书", "skill"]
表示至少包含 any 中的一个关键词。

filters = ["title", "content"]
表示 query 和 terms 只在 title 或 content 中匹配。

filters = []
表示不限制字段，query 和 terms 在 title、summary、keywords、content、recall_when 中匹配。

terms = {}
表示没有硬性关键词条件，只使用 query 和 filters 搜索。

terms.all、terms.none、terms.any 同时出现时使用 AND。
terms.any 数组内多个值使用 OR。
filters 数组内多个值使用 OR。
```

`terms` 不能替代 `query`。`query` 仍用于自然语言搜索、preview、embedding fallback 和 rerank。

`filters` 不能包含 `status`、`category`、`embedding` 或 `rerank`。CLI 搜索不开放状态和分类筛选，仍然固定排除 `trashed`。

## 6. Embedding 保底

embedding 只做保底召回。

使用时机：

```text
字面搜索结果为 0
```

配置入口：

```toml
[embeddings]
embeddings_api = "local"
embeddings_model = "BAAI/bge-m3"
embeddings_dimension = 1024
embeddings_key = ""
```

规则：

- `embeddings_api = "local"` 表示本地 embedding 入口。
- `embeddings_api` 不是 `local` 时必须填写 URL。
- `embeddings_dimension` 必须和 `memory_embeddings.embedding` 的 pgvector 维度一致。
- `embeddings_key` 在 `local` 模式下可为空。
- embedding 保底是否触发只看 `strategy.embedding_fallback`。
- embedding 命中不能返回 `trashed`。
- embedding 命中不能直接触发 read/update/delete。
- 字面搜索有结果时不混入 embedding 候选。

## 7. Rerank 重排

rerank 只对已经召回的候选集排序，不扩大召回范围。

配置入口：

```toml
[rerank]
enabled = false
rerank_api = "local"
rerank_model = "Qwen/Qwen3-Reranker-4B"
rerank_key = ""
```

规则：

- `enabled = false` 时只使用基础召回分排序。
- `rerank_api = "local"` 表示本地 rerank 入口。
- `rerank_api` 不是 `local` 时必须填写 URL。
- `rerank_key` 在 `local` 模式下可为空。

执行流程：

```text
1. search_memory 先按 query 搜索
2. 最多取 effective_limit 条；effective_limit = min(limit, [search].default_limit)
3. 如果 [rerank].enabled = true 且候选数 > 1
   就把这 effective_limit 条发给 rerank API 排序
4. 返回排序后的同一批候选
```

边界：

- rerank 不扩大召回。
- rerank 不额外拉更多候选。
- rerank 只排序最终要返回的 `effective_limit` 条。
- rerank 失败时返回原排序。
- rerank 只影响顺序，不改变候选可操作边界。

模型候选：

```text
Qwen/Qwen3-Reranker-4B      = 默认建议，输入倍率 5x，补全倍率 1x
Qwen/Qwen3-Reranker-8B      = 质量优先备选，输入倍率 10x，补全倍率 1x
BAAI/bge-reranker-v2-m3    = 开源权重备选，输入倍率 5x，补全倍率 1x，窗口 512
```

## 8. 实现方案

第一版实现采用 `memory_search_index` 搜索投影表、`pg_trgm` 字面召回、参数化硬过滤、embedding 保底、rerank 重排的顺序。

执行顺序：

```text
1. 从 memory_search_index 读取候选搜索文本。
2. 使用 pg_trgm 对 query 和 terms 做字面召回。
3. status 固定排除 trashed。
4. filters 控制 query 和 terms 参与匹配的字段范围。
5. terms.all / terms.none / terms.any 作为硬过滤。
6. 字面召回结果为 0 时，才允许 embedding fallback。
7. embedding fallback 仍必须遵守 status、filters、terms 硬过滤。
8. rerank 只重排最终候选，不扩大召回。
```

SQL 约束：

```text
用户输入的 query 和 terms 必须参数化绑定。
filters 只能从字段白名单映射到 memory_search_index 字段。
禁止把用户关键词直接拼进 SQL 字符串。
禁止让 embedding 绕过 terms.none、terms.all 或 trashed 边界。
```

索引方向：

```text
pg_trgm = memory_search_index.title_text / summary_text / keywords_text / content_text / recall_when_text / all_text
pgvector = memory_embeddings.embedding
```

当前不开放 `category` 作为 filter；如果后续要让 `category` 参与召回，只能作为搜索面，不作为硬过滤。

## 9. 成功响应

```json
{
  "state": "success",
  "tool": "search_memory",
  "data": {
    "strategy": {
      "embedding_fallback": false,
      "rerank": false
    },
    "results": [
      {
        "memory_uuid": "{memory_uuid}",
        "title_norm": "尼采深渊凝视 approve flow test 20260521_01",
        "status": "active",
        "summary": "用于测试 approve 流程的哲学版本记忆，主题是尼采与深渊凝视。",
        "preview": "……主题是尼采与深渊凝视……",
        "matched_fields": ["title", "summary", "keywords"]
      }
    ],
    "count": 1
  },
  "error": null,
  "profile": "{profile}"
}
```

响应规则：

- 没有命中时 `results` 返回空数组，`count = 0`。
- `strategy.embedding_fallback = true` 表示本次由 embedding 保底召回。
- `strategy.rerank = true` 表示本次结果已由 rerank 重排。
- `score` 只在 `strategy.rerank = true` 时返回，用于说明 rerank 排序分。
- `matched_fields` 只说明单条候选的命中来源，不能替代 `read_memory`。
- `preview` 是命中上下文短文本，最多 120 字，不能返回完整 `content`。
- `summary` 可以是 `null`。

## 10. Agent 使用规则

Agent 不能把搜索结果当作最终目标。

必须遵守：

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
3. 等用户确认具体 memory_uuid
4. 读取内容时调用 read_memory
5. 更新内容时调用 read_memory_hash
6. 携带 expected_*_hash 调用 update_memory_*
```

## 11. 成功验证

执行后检查：

```text
1. state = success
2. tool = search_memory
3. data.strategy 包含 embedding_fallback、rerank
4. data.count 等于 data.results 的数量
5. data.results 是数组
6. data.results 中的每条结果都有 memory_uuid、title_norm、status、matched_fields
7. 如果返回 preview，preview 不能超过 120 字
8. 结果中不能出现 trashed memory
```

## 12. 失败场景

```text
query 为空              = 拒绝
基础搜索 query 混用 AND 和 OR = 拒绝
高级搜索 query 包含 AND 或 OR = 拒绝
包含 mode              = 拒绝
包含 terms 但省略 filters = 拒绝
包含 filters 但省略 terms = 拒绝
filters 不是数组        = 拒绝
filters 包含未知值      = 拒绝
filters 包含 embedding  = 拒绝
filters 包含 rerank     = 拒绝
filters 包含 status     = 拒绝
filters 包含 category   = 拒绝
terms 不是对象          = 拒绝
terms.all 不是数组      = 拒绝
terms.none 不是数组     = 拒绝
terms.any 不是数组      = 拒绝
terms.all 数组为空      = 拒绝
terms.none 数组为空     = 拒绝
terms.any 数组为空      = 拒绝
terms 包含空关键词      = 拒绝
limit 小于 1           = 拒绝
limit 超过默认值        = 按 [search].default_limit 截断
请求包含未知字段        = 拒绝
```

## 13. 非目标

- 直接更新记忆
- 直接删除记忆
- approve / reject
- 搜索 `trashed` 记忆
- 跨 profile 搜索
- 跨 share 库合并搜索
- embedding 精准搜索
- rerank 扩大召回
- 关系图扩展搜索
- 自动选择最相关结果
