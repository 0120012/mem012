# search_memory 聚合搜索接口契约

冻结日期：2026-11-10。本文定义下一版目标接口；当前代码尚未切换到该契约。旧 P0 评测结果只作历史基线，不代表新接口已经验收。

## 请求

唯一入口为 `search_memory`，请求外壳为 `{"tool":"search_memory","params":{...}}`。

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

- `query` 必填，必须是 trim 后非空字符串；原自然语言同时供向量查询和 rerank 使用，不解析为布尔表达式。
- `terms` 必填，只接受 `include` 和 `exclude`。`include` 必须是至少含一个非空字符串的数组；`exclude` 可省略，默认 `[]`。关键词只 trim 首尾空白，保留顺序、大小写、重复项及词内空格。
- `filters` 可省略或为空数组，默认覆盖 `title`、`summary`、`keywords`、`content`、`recall_when` 五个字段；只接受这些字段名。
- `limit` 可省略或为 null，使用后端默认值；否则必须为正整数，实际返回数不超过请求值和后端上限。
- 未知参数、非法类型、空白 query/关键词及非法字段须在检索或模型请求前拒绝。

## 召回与重排

- 关键词路按 `terms` 独立召回；每个 include 都须命中，每个 exclude 都不得命中。
- 向量路按 `query` 独立召回，不要求向量候选包含任一 include。两路候选按 `memory_uuid` 去重，再一起交给同一轮 rerank。
- profile、工作态和 `trashed` 边界对两路一致。rerank 成功时按 rerank 顺序返回，去重后应用 `limit`。
- 合并去重后不足两条候选时跳过 rerank，直接按已确认的召回顺序返回；零候选返回空数组。
- rerank 失败时仍返回合并后的候选，回退顺序为关键词第 1 条、向量第 1 条、关键词第 2 条、向量第 2 条，重复 UUID 跳过，一路耗尽后取另一路，最后应用 `limit`。不比较两路原始分数；未成功 rerank 的结果不带伪装成 rerank 分数的字段。

## 响应

响应保留 `state`、`tool`、`data.results`、`data.count`、`error` 和 `profile`。`data.count` 等于 `data.results.length`；响应不包含 `data.strategy`、`embedding_fallback` 或 `rerank`。

正常完成或正常空结果：

```json
{
  "state": "success",
  "tool": "search_memory",
  "data": { "results": [], "count": 0 },
  "error": null,
  "profile": "{profile}"
}
```

rerank 失败但仍有候选时，保持 `state="success"`，`error` 为字符串 `"rerank_failed"`，`data.results` 按回退顺序返回且不超过 `limit`。

关键词路或向量路单独失败时，另一条成功的路继续完成搜索和 rerank；保持 `state="success"`，`error` 为 `"keyword_recall_failed"` 或 `"vector_recall_failed"`。若同一请求出现多个故障，`error` 为错误码数组，顺序固定为关键词路、向量路、rerank。两路召回都失败时 `state="error"`，错误码数组列出两路故障。

## 验收口径

- `embedding_search_samples.json` 的 `request_contract_cases` 与 `response_contract_cases` 是本契约的确定性接口样本；受控路由结果和 rerank 状态是测试夹具，不是请求参数。
- 每个成功或部分成功样本检查 `state`、`error`、结果顺序、UUID 去重、`count == results.length`、`count <= limit`、rerank 调用次数及 `strategy` 缺失。
- 请求样本覆盖默认值、query 原样传递、必填字段和非法输入；响应样本覆盖两路合并、向量独有候选、rerank 成功与失败、一路失败后的部分结果、多个故障、零候选和单候选。非法请求须在数据库和模型调用前拒绝。
- 既有 28 个检索用例、18 个非法请求及历史 27/28 结果仍保留作旧版资料；在重新标注并冻结前，不将它们称作新接口的 holdout 验收或质量证明。
