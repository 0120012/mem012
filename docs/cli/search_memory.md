# Search Memory

更新：2026-11-11。本文描述 `search_memory` 聚合接口；实际入口版本及验收范围见 [验收记录](../SEARCH_MEMORY_VALIDATION.md)。

## 用途

`search_memory` 并行从关键词和向量两路找候选，合并去重后统一 rerank，返回不超过 `limit` 条结果。它只返回候选，不读取、更新、删除或审核记忆；需要确认完整内容时调用 `read_memory`。

搜索始终限定当前 profile 和当前工作态，允许 `active`、`pending`，排除 `trashed`。

## 请求

```json
{
  "tool": "search_memory",
  "params": {
    "query": "把网页保存成 Markdown 的离线方法",
    "terms": { "include": ["Markdown"], "exclude": ["付费"] },
    "filters": ["content"],
    "limit": 5
  }
}
```

只接受 `query`、`terms`、`filters` 和 `limit`：

- `query` 必填，trim 后不能为空；保留自然语言，供向量召回和 rerank 使用。
- `terms.include` 必填且至少有一个非空字符串；`terms.exclude` 可省略，默认空数组。两者只约束关键词召回路，按不区分大小写的子串匹配；所有 include 都要命中，exclude 任一命中即排除该关键词候选。
- `filters` 可省略或传空数组，默认覆盖 `title`、`summary`、`keywords`、`content`、`recall_when`；也可传这些字段的子集。
- `limit` 可省略或为 null，使用后端默认值；否则必须为正整数，超过后端上限时按上限返回。
- 未知字段、非法类型、空白 query/关键词和未知 filter 均拒绝；参数错误发生在检索和模型请求前。

## 搜索约定

- 关键词路根据 `terms` 独立召回；向量路根据 `query` 独立召回，向量候选不必包含 `terms.include`。
- 两路候选按 `memory_uuid` 去重后统一送入 rerank。rerank 成功按 rerank 排序，之后应用 `limit`。
- 重排保留选定字段的完整原文；长文由客户端按实际 tokenizer 切成 query/document pair 不超过 8192 token 的重叠块，分批调用模型。每条记忆取最高块分数，再执行阈值过滤、排序和最终 limit；块数不占用最终结果名额。
- 每批最多 40 块、累计 pair 预算最多 32768 token。任一批请求失败或评分不完整时，整次 rerank 显式回退；query 自身耗尽预算也返回 `rerank_failed`，不提交残缺证据。
- 去重后少于两条候选时跳过 rerank；零条返回空结果，单条直接返回。
- rerank 失败时保留两路合并候选，按“关键词第 1 条、向量第 1 条、关键词第 2 条、向量第 2 条”的次序交替回退，跳过重复 UUID，最后应用 `limit`。回退结果不携带伪装成 rerank 分数的原始分数。

## 响应

正常结果和正常空结果都使用 `state="success"`、`error=null`。`data.count` 始终等于结果条数；响应不再包含 `data.strategy`、`embedding_fallback` 或 `rerank`。

rerank 失败时仍为 `state="success"`，并返回：

```json
{
  "state": "success",
  "tool": "search_memory",
  "data": { "results": [{ "memory_uuid": "{memory_uuid}" }], "count": 1 },
  "error": "rerank_failed",
  "profile": "{profile}"
}
```

若关键词路或向量路单独失败，另一条成功的路继续完成搜索，并返回该路召回的结果（可能为空）；用 `error="keyword_recall_failed"` 或 `error="vector_recall_failed"` 标出失败来源。一次出现多个故障时，`error` 是错误码数组，按关键词路、向量路、rerank 的顺序排列；两路召回都失败时 `state="error"`。只有一个故障时 `error` 为字符串，正常完成时为 null。

结果保留 `memory_uuid`、`title_norm`、`status`、`summary` 和 `matched_fields` 等现有字段；`content_preview` 仅在适用时出现且不超过 120 字。搜索结果只用于定位候选，不能按排序自动选择第一条执行后续读写操作。

## Agent 调用

```bash
mem012 --profile {profile} --args '{"tool":"search_memory","params":{"query":"离线保存网页的方法","terms":{"include":["网页"],"exclude":[]},"filters":["title","summary","content"],"limit":5}}'
```

- 检查 `state` 与 `error`：`state="success"` 且 error 非 null 表示部分召回或 rerank 回退，仍须检查 `data.results`；`state="error"` 表示搜索未能完成。
- 展示候选 `memory_uuid`、`title_norm`、`status`，取得确认后再调用 `read_memory`；更新前调用 `read_memory_hash`。
- 调用返回后核对 `data.results` 为数组、`data.count` 与数组长度一致，且结果数不超过请求 `limit`。
