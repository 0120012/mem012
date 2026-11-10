# search_memory 聚合搜索验收记录

记录日期：2026-11-10。当前状态：代码审查及本地集中测试完成；生产新接口已启用，rerank 正常成功路径尚待运维侧复核。

## 版本与环境

- 开发起点 HEAD：`e915c35cf7b905164f0efa4285d811e07d0e6247`。
- 实测入口：本工作区 `target/release/mem_012 --profile ... --args ...`，经过真实 CLI 路由、解析和 PostgreSQL 查询。
- 确定性测试采用独立 schema 和本地受控 embedding/rerank HTTP 服务；不代表真实模型相关性质量。测试 schema 在各批结束后已清理。
- Python 临时夹具：`/tmp/mem012-search-eval/eval.py`；未新增持久化 Rust 测试。
- `cargo check --tests --quiet`、`cargo build --quiet` 已通过；存在既有 embedding dead_code 警告。
- `cargo build --release --quiet`、`cargo test --quiet` 通过：120 passed，1 ignored。仅有既有 embedding dead_code 警告。

## 已确认结果

最终使用 release CLI 通过 51 项确定性检查；统一审查专项另有 3 项通过，不重复计入此表：

| 检查组 | 数量 | 结果 |
| --- | ---: | --- |
| 冻结 request_contract_cases | 9 | 通过 |
| 冻结 response_contract_cases | 7 | 通过 |
| 畸形评分及全部低相关评分 | 8 | 通过 |
| 跨字段 AND、排除词及字段边界 | 5 | 通过 |
| 实际并行与 limit=1 不提前截断 | 1 | 通过 |
| 远端同义证据与预算不足回退 | 2 | 通过 |
| 状态、源 revision、多块及 profile 隔离 | 2 | 通过 |
| 参数类型、空值和未知字段边界 | 15 | 通过 |
| 关键词路故障和多故障组合 | 2 | 通过 |

评分检查覆盖部分、重复、越界、空、非有限、缺失评分及缺失 results；全部低相关评分正常返回空结果。并行检查在关键词表被锁住时观察到向量模型请求启动。隔离检查覆盖 pending 可见、trashed 和旧 revision 不可见、相同 UUID 跨 profile 及异域专属 UUID。

测试曾复现：仅按 query 字面词片裁剪长文，会遗漏远端同义证据。已改为保留全部独有原文，仅压缩连续相同段落并记录重复次数和原文位置；超预算则显式回退。远端同义证据及无法缩减的预算不足用例复测通过。

## 统一代码审查

- 审查了真实工具路由、输入校验、并行召回、include/exclude 硬条件、profile/status/revision 边界、按 UUID 合并、rerank 证据预算、评分映射与错误回退、最终 limit 和文档契约。
- 复现并修复三项问题：长文证据不能只按 query 词片裁剪；向量块不能在按 UUID 去重前固定截断；默认 rerank 模型须与已冻结 tokenizer 相符。release CLI 专项覆盖远端同义证据、850 个失效旧 revision 块、单个记忆 850 个有效块挤占另一个候选、以及省略 rerank 模型名，全部通过。
- 复测采用独立 PostgreSQL schema 和受控模型服务。测试后确认夹具 schema 清理；未修改正式记忆。

## 相关性验收与剩余事项

- 已另行冻结 `search_memory_quality_cases.json`：7 条合成记忆、3 个 tune 和 3 个 holdout 用例；旧版 cases 未用作新版质量证明。
- 真实 `BAAI/bge-m3`（1024 维）及 `BAAI/bge-reranker-v2-m3` 经本地 HTTP 转发服务和真实 CLI 执行。初始阈值 0.1 在网页保存用例返回两个无关尾部，分数约 0.175 和 0.107；正例分数约 0.926 和 0.810。仅依据 tune 调整阈值为 0.3，候选预算保留 40，向量距离阈值保持 0.55。
- 修复后重跑 tune 3/3、holdout 3/3，均通过，未使用 holdout 调参。每例调用一次查询 embedding 和一次 rerank，另有 7 次语料 embedding。模型服务未提供可验证的权重 revision，版本记录限于服务配置模型名。
- 最终 tune：网页保存 19.84 秒（0.926、0.810）、语法 42.96 秒（0.984）、无关查询 33.15 秒（空结果）。holdout：队列 14.79 秒（0.903）、书签 15.49 秒（0.948）、截图 14.93 秒（0.930）。六例查询 embedding 共 102 prompt tokens；rerank usage 均为 unknown。该结果仅证明本次小规模合成集，不替代真实资料质量评测；候选预算 40 尚无大规模召回收益证据。
- CLI 文档、配置示例和记忆技能已同步；技能结构检查通过。当前 `/usr/local/bin/mem012 2.0.0` 生产入口已接受新接口；首次宽字段只读冒烟返回两条候选但 `error="rerank_failed"`。窄字段冒烟未产生候选，因而没有触发 rerank。已部署进程没有提供错误阶段诊断，生产 rerank 正常成功路径尚待复核。此前记录的 `public.memory_embedding_config` 故障恢复情况仍未知。
