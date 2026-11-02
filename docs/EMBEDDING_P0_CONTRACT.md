# P0 搜索契约与验收口径

冻结日期：2026-11-02。适用于本计划的新流程；旧流程不满足的行为记录为基线差异，不在 P0 修正生产代码。

## 参数与 include_text

- 唯一入口为 `search_memory`，外壳为 `{"tool":"search_memory","params":{...}}`。
- params 只接受 `terms`、`filters`、`limit`；任何出现的 `query`（包括 null）、未知参数均拒绝。
- `terms` 与 `filters` 必须同时提供且非 null；terms 只接受必填的 `include`、`exclude` 两个字符串数组。
- include/exclude 可各自为空，但不能同时为空；元素去除首尾空白后不能为空。类型错误、未知 terms 字段、空白元素均在检索和模型请求之前拒绝。
- include/exclude 仅 trim，保留顺序、大小写、重复项及词内部空格；include_text 为 trim 后的 include 按一个空格连接。不增加独立检索描述参数，不为 include_text 另设长度上限。
- `filters` 为字符串数组，白名单为 `title`、`summary`、`keywords`、`content`、`recall_when`。空数组表示全部五个字段；重复字段不改变匹配范围。字段名区分大小写。
- `limit` 省略或 null 时沿用后端默认值；其他值必须为 1 至 2147483647 的 JSON 整数，布尔值不是整数。实际返回上限为请求值和后端上限的较小者；当前源码后端上限为 8。

```json
{"terms":{"include":[" 网页 ","Markdown"],"exclude":["付费"]},"filters":["title","content"],"limit":5}
```

该请求的 include_text 为 `网页 Markdown`，不是对外新增的参数。

## 硬条件、字段与状态

- 仅搜索当前 profile 的当前工作态；`active`、`pending` 可见，`trashed` 不可见。状态列不是可搜索文本，不开放 status 请求参数。
- 选定字段按既有投影方式以空格拼接；keywords 先排序、以空格拼接。对该文本与关键词采用大小写不敏感的字面子串匹配，不执行正则、分词扩展、词干化或 AND 放宽。
- 每个 include 都必须出现，每个 exclude 都不能出现；约束作用于整条记忆的选定字段，允许各 include 分散在不同字段、章节和块中。
- 字面与向量两路遵守相同硬条件；filters 外的文字不参与 include/exclude 判断。相同正文的不同 UUID 不能合并，跨 profile 的相同 UUID 是不同身份。
- include 非空时才调用 embedding/rerank。include 为空、exclude 非空时仅做纯过滤，不调用任何语义模型；仍受状态、profile、limit 约束。
- 外部继续返回记忆 ID、标题、摘要等既有字段，按需 read_memory 回读全文；不要求公开接口新增证据片段。

## 故障和相关性

| 条件 | 固定预期 |
| --- | --- |
| 参数非法 | invalid_request；数据库检索、embedding、rerank 均不执行 |
| 正常无硬命中或重排全部低相关 | success，空结果；不能伪装成 provider 故障 |
| 向量路失败，字面路可用 | degraded；只对已有字面候选继续执行完整相关性判断 |
| 字面路失败，向量路可用 | degraded；向量候选须满足全部硬条件且通过完整重排后才能返回 |
| 两路均失败 | provider/检索故障；不作为正常空结果统计 |
| rerank 失败、空响应、部分响应、重复/越界索引、非有限分数 | degraded；仅保留字面路已召回且满足全部硬条件的候选，不追加未完整评分的向量候选 |
| 证据无法完整保留各 include 依据 | 与 rerank 不可用同样降级，不提交丢失依据的文档 |
| 准确名称且 rerank 不可用 | 已由字面路召回、满足硬条件的名称候选保留；不因名称匹配绕过正常成功的相关性判断 |

- 只有一条候选也进行相关性判断。去重后每条记忆对应一个 rerank document/分数；先相关性过滤和排序，最后应用 limit。
- 证据保留真实字段名、章节路径和自然原文；合并重叠范围，先缩短外围上下文，再按上述规则处理仍放不下的完整依据。
- 具体响应字段/错误码随 P2/P3 接入现有工具确定；本文件的 success/degraded/invalid_request 是必须可观测和区分的行为类别，不是假称已上线的新响应格式。

## 样本与统计

- 使用 `embedding_search_samples.json`：24 个 profile+UUID 身份、28 个检索用例、18 个非法请求、4 个分块边界场景及受控故障/证据场景。
- tune 11 条用于选参数，holdout 14 条在参数固定后验收，contract 3 条不参与阈值调参。两组正例记忆身份不得重用；所有标签均为合成标注，不能代表真实用户语料质量。
- `expected_memory_uuids` 表示最终相关结果集合；`expected_hard_match_memory_uuids`（未给出时等于前者）表示硬条件允许集合。两者分别用于相关性与越界判断，不能混用。
- 结果按集合比较；有 expected_order 时再比较顺序。UUID 重复、超 limit、profile/状态/字段/terms 越界均独立计错。标有 expected_readback_summary 的用例同时核对对应身份的摘要。
- 单例召回率 = TP / 预期正例数；预期为空时单列空结果准确率。总体 precision = TP/(TP+FP)，recall = TP/(TP+FN)；分母为零记 null，不能把故障算作正确空结果。
- 质量按 tune/holdout/contract 分别报告，不用总体均值掩盖某组失败。记录每个用例的原始 UUID、TP/FP/FN、硬条件违规、重复、错误、耗时及调用量。

## P4 固定通过标准

此处先于旧流程实测冻结；不按基线结果降低标准。

- 28 个小规模合成检索用例全部满足预期结果集合（100%）；所有标注顺序及回读摘要检查通过。
- 严格过滤违规、跨 profile 泄漏、重复结果、超 limit 均为 0；18 个非法请求全部拒绝且不检索、不调用模型。
- exclude-only 的模型调用数为 0；单候选、失败降级、部分/畸形响应与证据预算场景全部符合固定预期。
- 分块正文范围完整覆盖、续块上下文计入实际 tokenizer 预算、尾部及围栏锚点保留；版本并发与原子替换另由 P1 的隔离行为测试证明，不能由本次样本校验冒充。
- 留出样本只在参数冻结后用于最终验收；P0 记录旧版的留出表现不用于选择新阈值。更改标签/语料/参数后必须记录新版本，不得把同一次结果说成独立留出验证。
- 上述标准仅是固定回归集的准入门槛；真实资料质量需另行评测。

## 耗时、调用量与预算

- Python 参考基线用 monotonic clock 测量从一次搜索开始至返回，包含 SQL 与模型网络，排除建临时表及 tokenizer 下载。分别报告 p50/p95/max；不将此数值视为完整 Rust CLI 耗时或生产容量。
- Python 请求封装逐次记录模型类型、HTTP 状态、耗时、输入/文档数、服务返回 usage 及限流头。usage 缺失记 unknown，不估作 0；不输出密钥或认证头。
- 本轮任务保护上限：最多 64 次模型请求，单请求连接/读取超时 10/45 秒，串行且起始间隔至少 1 秒。它是评测任务预算，不是账户真实配额；遇到 429 停止新增上游请求并保留失败记录。
- 用户提供的模型信息：embedding 为 1024 维，两个模型均标 8K，bge-m3 明确为 8192 tokens；另提供最高 RPM 2000、TPM 500000。报告注明来源，不假称通过响应头测得；本轮保守共用该调用上限，不通过压测撞限流猜测配额。
- tokenizer 按官方仓库固定 revision 下载，仅下载 tokenizer/配置，不下载模型权重；记录 SHA-256、库版本、特殊 token 开销及中英/代码计数。服务 token usage 与本地 tokenizer 不一致时不得当作已证明兼容。

## 隔离与复现

- 不新增或修改 Rust 代码。Python 从当前 `search_memory.rs` 提取原始两段 SQL，保留参数绑定、字面零命中才向量保底、候选多于一条才重排及旧版 0.1 过滤规则；模型调用使用 requests。
- 每个合成 profile 使用独立连接；建连先建立并填充 TEMP 搜索投影表与 TEMP 向量表，确认名称解析到 pg_temp，再进入只读事务默认模式；连接关闭后临时表消失。
- 不启动 mem012 server，不调用 approve、create_profile、import_memory、迁移器或回填器；临时表随连接关闭消失。
- 高级搜索字面硬条件为零时，旧向量路使用相同硬条件仍为零。本次向量表为空不会改变这些有效高级请求的结果集合；仍实际执行查询 embedding 和空表 SQL。此基线不评价自然语言 query、ANN 索引质量或向量召回率。
- 该方法是执行原始 SQL 的 Python 参考基线，未经过完整 Rust CLI/dispatch/provider；不覆盖其解析器、连接池、f32 转换、重试与运行时开销。参数拒绝由冻结契约自检及现有 Rust 单元测试分别验证，不把 Python oracle 结果冒充 Rust 行为。
- 逐用例输出和源码/样本/契约 hash、Git commit/dirty 状态、配置中的非敏感模型参数共同保存，避免将本地结果当作远端部署证据。
