# P0 交付与评测记录

日期：2026-11-02。P0 已完成契约、样本、限制核对、参考基线和验收口径；P1–P5 尚未实施。P0 的完成表示已经能判断后续改造的对错，不表示旧流程已达到新流程质量要求。

## 交付清单

| P0 项目 | 证据与完成范围 |
| --- | --- |
| 参数、include_text、字段与状态契约 | [冻结契约](EMBEDDING_P0_CONTRACT.md)：有效/非法输入、query 拒绝、exclude-only 不调用模型、整条记忆 AND/exclude、active/pending 可见与 profile 隔离 |
| 标注样本 | [合成样本](embedding_search_samples.json)：24 个 profile+UUID 身份、28 个检索用例、18 个非法输入、4 个分块边界场景、9 组/21 个受控流水线条件；原文依据与硬匹配集合已核对 |
| 降级规则 | 冻结契约的故障表，以及样本中的 pipeline_cases；明确故障不等于正常空结果、失败时不能追加未评分的向量候选、准确名称保留边界 |
| 模型与服务限制 | [固定版本 tokenizer 与真实 API 记录](embedding_p0_model_limits.json)，以及用户提供的 8K、1024 维、RPM/TPM 信息；区别声明来源与实测结果 |
| 旧流程参考基线 | [逐用例结果与中间证据](embedding_p0_baseline.json)：Python 执行当前原始 SQL、调用真实模型，未修改 Rust，也未执行完整 Rust CLI |
| 质量线与统计口径 | 冻结契约：固定合成集全通过；严格过滤违规、重复、泄漏、超 limit 为 0；按 tune/holdout/contract 分组记录相关性、空结果、耗时与模型调用量 |

调参/留出/契约样本分别为 11/14/3；tune 与 holdout 的正例记忆身份不重叠。未按基线结果修改标签、旧版 0.1 阈值或样本划分。holdout 的旧版结果仅作基线，不用于选择新版阈值。

## 参考基线结果

下表为最终保存的一次运行；精确时间、分数、源码/样本/契约 hash、Git commit 与 dirty 状态见 JSON。

| 范围 | 通过用例 | Precision | Recall |
| --- | --- | --- | --- |
| tune | 10/11 | 90.91% | 100% |
| holdout | 14/14 | 100% | 100% |
| contract | 3/3 | 100% | 100% |
| 总体 | 27/28 | 95.83% | 100% |

- 预期空结果 7 条，正确空结果 6 条；严格过滤违规和重复均为 0。
- `low-relevance-001`：硬条件命中一条记忆，但标注认为不相关。旧流程只有一条候选时跳过 rerank，最终多返回一条；该用例未通过。
- `state-boundary-001`：字面 SQL 正确召回 active 和 pending，重排只收到标题与摘要，未收到实际命中的正文。早期运行中 pending 得分为 0.0981820524，被旧阈值 0.1 过滤，整体为 26/28；最终运行得分为 0.1000941172，该用例通过，整体为 27/28。相同输入的服务分数存在波动，不能把这次跨过阈值当作修复或稳定收益；应继续按计划补齐命中证据并在 tune 集标定阈值。
- 最终基线调用 embedding 5 次、rerank 2 次，全部 HTTP 200；embedding 的 usage 合计 37 input tokens，rerank 的 meta.tokens 合计 89 input tokens。rerank 的 usage 字段为空，但 meta 中提供了计数，不能误记为 0。
- Python 参考流程 p50 1020.2 ms、p95 4531.4 ms、max 4660.4 ms；包含评测保护设置的模型请求起始间隔，且为小样本单次网络测量，不能当作 Rust 程序或生产容量指标。

## Tokenizer、窗口与配额

使用 BAAI 的官方 ModelScope 仓库固定提交；下载文件与官方元数据的 SHA-256 一致，仅下载 tokenizer/配置，没有下载模型权重。Hugging Face 官方仓库下载在本机持续超时，因此未将其未下载文件冒充实际核验来源。

| 模型 | 固定 tokenizer 提交 | 本地配置 | 特殊 token 开销 |
| --- | --- | --- | --- |
| BAAI/bge-m3 | e44369c5623cc146f016da906583db4ee0e3488d | model_max_length 8192 | 单文本 2，成对 4 |
| BAAI/bge-reranker-v2-m3 | e099d4b9cdbd291b1569d416f19aaa6523570bc3 | model_max_length 8192 | 单文本 2，成对 4 |

- embedding：本地计数 6 与 8192 tokens 的请求均 HTTP 200，服务 usage 分别为 6/8192，返回 1024 维；8193 tokens 返回 HTTP 400。
- rerank：本地 query/document 联合计数 17 与 8192 tokens 均 HTTP 200，服务 meta.tokens 分别为 17/8192；8193 tokens 仍 HTTP 200，但服务计数为 8281。已确认超长时计费/处理行为不同，未证实其内部是截断、切块还是其他处理，不能依赖它保证完整证据。
- 后续 embedding 的完整输入（含标题/上下文及特殊 token）按其 tokenizer 控制在 8192 内；rerank 的 query 与证据联合按对应 tokenizer 控制在 8192 内。不为 include_text 新增长度参数或专门超限规则；完整证据放不下时采用既定降级。
- 两个 tokenizer 不能因模型名字相近就混用：同一代码示例在 embedding/rerank 中分别为 14/13 tokens。最长样本正文为 105344 字符、64897 tokens，必须继续分块，字符数不能代替 token 数。
- 用户提供最高 RPM 2000、TPM 500000。本轮保守视为共用上限；服务响应没有限流头，未分别独立验证两模型的账户配额，也没有通过压测撞限流。评测任务额外限制最多 64 次请求、起始间隔至少 1 秒，实际调用量远低于上述上限。
- 分块大小、重叠、候选数和相关性阈值仍由 P1–P4 在 tune 集选择；8K 是预算上界，不是建议把每个块填到 8K。

## 隔离方法与复现

Python 从当前 search_memory.rs 提取两段原始 SQL，只做 PostgreSQL `$n` 到 psycopg 占位符转换。每个合成 profile 使用独立连接，装载连接私有 TEMP 表，确认表名解析到 pg_temp 后设置默认事务只读。连接关闭即清除临时表，没有启动服务、写入真实记忆、迁移或回填。

本次所有有效高级请求都受整条记忆的严格 AND/exclude 约束；字面硬命中为零时，相同边界下向量路也不能补入记忆，因此空 TEMP 向量表不改变这组请求的结果集合。仍实际执行了查询 embedding 和向量 SQL；该方法不评价 ANN 索引质量、自然语言 query 或将来的块向量召回率。

```bash
python3 -m pip install --user -r scripts/p0_requirements.txt
python3 -B scripts/embedding_p0.py
python3 -B scripts/embedding_p0.py --live
python3 -B scripts/probe_embedding_limits.py --live
cargo test
```

`--live` 使用 MEM012_CONFIG（未设置则为仓库 config.toml）。基线默认借用 codex 的数据库连接权限，只建立临时表；可用 `--connection-profile` 选择现有连接。报告均覆盖写入；需要保留多轮结果时，用 `--report` 指定不同路径。

本轮运行环境：Python 3.13、requests 2.34.2、psycopg 3.3.6、tokenizers 0.23.3。模型报告保存各下载文件 hash；基线报告保存实际源码/样本/契约 hash 和 Git commit。外部服务可能更新或产生分数波动，复现命令不保证逐 bit 相同。

## 验证与剩余边界

- 样本加载、全部硬条件标签/原文依据、profile 身份、状态、调参/留出边界、18 个非法请求的目标契约检查通过。
- 另核对 4 个分块边界场景的原文断言、10 个补充非法输入，以及判分器对重复、越界、错误 profile、源身份错配和 provider 失败的拒绝；模型失败不能被计为正确空结果。
- 保存结果按当前判分器回放一致，报告中的源码 hash 与当前文件一致；脚本语法和 diff 空白检查通过，报告中没有模型凭据。
- 仓库已有 `cargo test`：121 passed，0 failed。`git diff --exit-code -- src Cargo.toml Cargo.lock` 通过，Rust 源码及依赖无差异。
- Python 参考流程未经过完整 Rust CLI/dispatch/provider，不复刻其 f32 转换、重试、连接池和运行时开销。非法请求的 Python oracle 不冒充 Rust 入口的全量集成测试；公开 query 拒绝同时经现有路由源码核对。
- 故障/证据/分块场景在 P0 完成预期定义与材料自检；真正的新流程降级、分块输出、原子替换和并发版本保护仍是 P1–P3 的行为验收。P0 不勾选这些实现项，也不宣称检索质量已提升。
