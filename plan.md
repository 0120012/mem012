# mem012 服务端性能优化计划

## 目标

先针对 `GET /api/memories` 消除请求热路径中的重复配置读取和数据库连接池创建，在不改变 API 契约的前提下，提高吞吐量并降低延迟与连接开销。

本阶段只优化服务端资源复用，不同时引入分页、缓存或数据库结构调整，确保收益可以单独测量和验证。

## 当前问题

一次 `GET /api/memories` 请求当前大致会执行：

1. session 校验重新加载 `config.toml`。
2. project 校验再次加载配置。
3. memories 查询前第三次加载配置。
4. 创建 `max_connections = 1` 的新 `PgPool`。
5. 建立新的 PostgreSQL 连接。
6. 请求结束后销毁连接池。
7. 查询全部 active memories，并返回完整正文和关键词。

相关位置：

- `src/api/auth.rs:340`：认证过程重新加载配置。
- `src/api/utils.rs:43`：project 校验重新加载配置。
- `src/api/memories.rs:153`：查询前重新加载配置。
- `src/config.rs:235`：同步读取并解析配置。
- `src/psql/memories.rs:66`：每次请求创建连接池。
- `src/psql/memories.rs:11`：列表查询聚合完整 JSON。

## 目标架构

```text
server 启动
  ├─ 读取一次 Config
  ├─ 为每个 profile 创建惰性共享 PgPool
  └─ ServerState
       ├─ config
       ├─ api_token
       └─ profile -> PgPool

/api/memories
  ├─ 从 ServerState 校验 session
  ├─ 从 ServerState 校验 project
  ├─ 获取共享 PgPool
  └─ 执行 SQL
```

## 实施顺序

每一步作为独立、可验证的最小切片实施；完成并验证当前切片后再进入下一步。

1. 建立 `/api/memories` 真实基线。
   - 固定 profile、数据量、请求参数和压测并发。
   - 记录 RPS、错误率、p50、p95、p99 和 PostgreSQL 连接数。
2. 新增只读 `ServerState`。
   - server 启动时读取一次配置。
   - 保存配置和 API token，不改变现有接口行为。
3. 创建惰性共享的 profile 连接池。
   - 首次访问 profile 时创建连接池。
   - 后续请求复用同一 profile 的连接池。
4. 使用 `ServerState` 完成认证与 project 校验。
   - 移除 `/api/memories` 热路径中的重复 `load_config`。
   - 保持原有错误状态码和错误内容。
5. 让 `/api/memories` 查询使用 `&PgPool`。
   - 查询函数接收共享连接池。
   - 不再在单次请求中创建和销毁连接池。
6. 使用与基线完全相同的参数复测。
7. 仅在收益明确且行为稳定后，将共享状态与连接池逐步推广到其他 API。
8. 分页和摘要列表作为第二阶段单独设计，不与连接池优化混合实施。

## 连接池初始参数

```text
min_connections = 0
max_connections = 4
acquire_timeout = 3 秒
idle_timeout = 5 分钟
```

这些参数作为 1C1G 和 2C4G 部署的保守起点。最终值应依据实际数据库连接上限、profile 数量和复测结果调整，不能仅根据 CPU 核数推断。

## 工具并发基线

前端 API 压测不能代表工具并发能力。工具通过独立的 `mem012` CLI 进程执行，每次调用都会读取配置、创建当前 profile 的连接池并连接 PostgreSQL，因此需要单独进行多进程压测。

按以下路径分别测试，禁止混合统计：

1. 只读工具：`read_memory`，用于测量进程启动、配置读取和数据库读取开销。
2. 搜索工具：`search_memory`，分别测试字面命中路径与触发 embedding/rerank 的外部 provider 路径。
3. 写入工具：使用隔离测试数据执行 `create_memory` 或更新工具，测量事务、行锁和搜索索引刷新能力。

每组使用相同 profile 和固定数据集，依次测试并发 `1、5、10、20、50`；记录成功率、吞吐量、p50、p95、p99、进程 CPU/内存峰值和 PostgreSQL 活跃连接数。写入测试必须使用可清理的专用 profile，不能操作生产记忆。

工具基线需要回答：

- 单次 CLI 启动和数据库建连占总耗时的比例。
- 并发工具进程是否导致 PostgreSQL 连接数线性增长或触及连接上限。
- 只读、事务写入和外部 provider 三类路径各自的稳定并发上限。
- 失败来自本机资源、PostgreSQL、锁竞争，还是 embedding/rerank provider 限流。

## 验收标准

- `/api/memories` 热路径不再调用 `load_config`。
- 不再为每个请求创建 `PgPool`。
- 同一 profile 的请求复用同一个连接池。
- 原有错误语义、HTTP 状态码和 JSON 响应结构保持一致。
- PostgreSQL 连接数不随请求总数线性增加。
- 优化前后使用相同 profile、数据量、请求参数和压测环境。
- 压测错误率低于 `1%`。
- p95 和 p99 延迟不劣化。
- RPS 有明确、可重复的提升。

## 风险与约束

- 每个 profile 使用独立连接池会累加数据库连接数，因此池上限必须保守，并结合可用 profile 数量评估总连接数。
- 配置缓存后，新建或修改 profile 需要重启 server 才能生效；这与当前 README 描述的运行方式一致。
- 分页会改变 API 契约，必须后置并独立设计兼容策略。
- 健康检查接口不经过 memories 数据库查询，不能用于证明数据库路径的实际容量。
- 完整 memories 正文的响应体积仍可能成为瓶颈；只有完成连接池优化并取得基线数据后，才评估摘要列表或分页。

## 本阶段明确不做

- 不修改前端。
- 不改变 API 响应结构。
- 不增加分页。
- 不修改数据库表或索引。
- 不缓存 memories 响应。
- 不调整 Tokio 线程数。
- 不一次性重构全部 API。
