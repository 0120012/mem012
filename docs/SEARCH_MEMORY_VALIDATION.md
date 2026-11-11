# search_memory 聚合搜索验收记录

记录日期：2026-11-11。当前状态：客户端长文分块修复、审查及本地验收完成；原失败查询已用本地 release 及真实模型复测通过，安装入口尚未部署本轮改动。下列原版验收结果作为历史保留，当前修复见末节。

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

## 2026-11-11：客户端长文分块修复

- 基线 `e76dd439ed949abdb073fc5abb7f4a86b204bf68`。已定位长文候选 `7993f641-21c8-4a91-9776-5edec13e62e6`，原 query/document pair 约 39,665 token；本地旧证据门禁直接回退。完整发送当前 SiliconFlow endpoint 后 HTTP 400 / `20015`，显式 `max_chunks_per_doc=16` 和 `overlap_tokens=80` 仍失败；短文对照 HTTP 200，因此不依赖服务端自动切块。
- 新实现使用冻结 BGE reranker tokenizer，按原文 UTF-8 字节边界切分，最多 80 token 重叠；保留完整字段，短文不改写。每块 pair 最多 8192 token，每批最多 40 块/32768 pair token。块评分先校验完整性，再按原记忆取最大值、稳定排序。任一批失败都保留既有显式回退，query 无可用文档预算时同样失败。
- `cargo test --quiet`：123 passed、1 ignored。新增测试覆盖 Unicode/组合字符、原文完整覆盖至末尾、边界证据、短文及空文档保持原样、query 耗尽预算。debug/release 构建通过，仅有既有 embedding dead_code 警告。
- 独立 PostgreSQL schema + 受控 HTTP 模型经过真实 CLI 的八项场景全部通过：长文末尾正例、长文负例、第二批 HTTP 失败、部分评分、重复索引、越界索引、缺少评分和非有限评分。长文均跨两个批次；逐次检查每块/批预算、记忆级最高分、limit=1 及故障时无伪评分。测试 fixture 只操作本次新建 schema。
- 原冻结真实模型样本首次复验在阈值 0.3 下 tune 3/3、holdout 3/3 通过；追加长文 tune 将阈值收紧至 0.4 后再次使用 release CLI 全部复验通过，最终耗时为 13.97/16.15/13.06 秒和 11.36/16.30/16.94 秒。每例一次 embedding、一次 rerank；六例 query embedding 共 102 prompt tokens，rerank usage unknown，另有七次语料 embedding。
- 本地当前 release 对正式 `codex` 资料只读复测原失败查询：原 40 条候选形成 44 个块，分两批 17/27 块；批 pair token 合计分别 32265/21888，单块最大 8192/8191。两次真实 rerank 均 HTTP 200，服务耗时 0.91/0.76 秒，整体 CLI 79.15 秒。一次 query embedding 18 prompt tokens；rerank usage unknown。最终 `error=null`、3 条记忆带重排分数，约 0.956/0.856/0.802；未写入正式记忆。
- 追加长文 tune：1500 条带编号的植物观察段落，正例仅在末尾追加网页离线导出 Markdown 的操作说明，负例不包含该说明。实际 rerank 正例约 0.970；负例约 0.311，原阈值 0.3 误保留，故本次仅依据追加 tune 将阈值收紧至 0.4。随后冻结参数复验既有样本，并另用天文背景中的队列恢复末尾证据作为长文 holdout，不使用 holdout 调参。
- 最终真实长文 tune 2/2 通过：末尾正例评分约 0.970、15.06 秒，无关长文被正常排除、17.29 秒。独立长文 holdout 2/2 通过：1500 条天文观察背景，末尾追加队列恢复说明时评分约 0.941、14.94 秒；无该说明时正常空结果、13.21 秒。每例两条记忆形成五个块，单块最多 8192 token，均一次 rerank 完成，usage unknown。为单独检验重排，长文用例的召回向量受控，查询 embedding 及 rerank 均使用真实模型；不将这些小规模合成样本等同于大规模真实资料质量验收。
- 最终 0.4 阈值下本地 release 再次对正式 `codex` 资料只读执行原查询：50.00 秒，`error=null`，三条评分约 0.956/0.855/0.802，stderr 为空。临时测试 schema 核实剩余为零，临时脚本/配置随后移除；工作区正式配置、数据库结构、正式记忆与已安装二进制均未修改。当前源码尚未提交或部署，整体查询延迟仍为数十秒，不能把本轮 token 故障修复等同于延迟优化。
