# Server 数据库连接池复用主线计划

## 摘要

让常驻 Server 按启动配置为每个有效 profile 创建一个 `PgPool`，通过 Axum 共享状态复用于 HTTP 路由和后台 worker。每池最多 5 个连接；数据库配置修改后重启 Server 生效。CLI 保持现有连接生命周期，HTTP 路径与正常请求的响应契约不变。

计划基线：`ff062e7`。实现中已移除变更详情查询的临时局部缓存，并转为 Server 共享状态管理连接池。

## 实施清单

- [x] [接线] 构建按 profile 索引的 Server 状态并注入 Axum 路由；变更详情、列表、计数及审批/拒绝操作改用共享池，移除详情接口局部缓存。首个接线切片允许改动少量相关函数。
- [x] [迁移] 变更审查的列表、详情、审批和拒绝操作，以及回收站列表、详情、删除和恢复，均改为借用共享池。
- [x] [迁移] 记忆列表、新建、编辑和类别关键词查询均改为借用对应 profile 的共享池。
- [x] [迁移] 审批触发的 embedding 生成/写回与周期性补齐扫描/写回均借用对应 profile 的共享池；补齐 worker 与 Axum 共用 Server 状态。
- [x] [迁移] 图状态、重建、邻居、总览与关系 API 均通过 Server 共享状态取得 profile 池，移除每请求建池路径。
- [x] [迁移] 回收站清理 worker 接收并复用 Server 状态中的 profile 池；单个 profile 故障时继续处理其他 profile。
- [x] [验收] 单元测试验证同一 profile 复用同一池、不同 profile 隔离、数据库 URL 变化不复用旧池、无效 URL 跳过且不阻断其他 profile；只读真实数据库测试确认连续详情查询沿用同一 backend connection 且池大小不超过 5。`cargo test` 125 通过/0 失败/2 忽略，`cargo check` 与 `git diff --check` 通过；耗时仅作观测。
- [ ] [发布] 部署后重启 Server，检查健康接口、项目列表及详情请求。无数据库迁移；无效 URL 的 profile 不进入可用项目列表并记录 profile 名称，不记录连接串。

## 接口与失败处理

- Server 状态持有 `profile → PgPool` 映射；有效 URL 使用 `connect_lazy`，不因数据库暂时不可达而阻止 Server 启动。
- URL 格式无效时跳过对应 profile，其余 profile 和非数据库路由继续服务；项目列表仅公开成功构建连接池的 profile；如果没有可用池，Server 仍可启动并提供非数据库功能。
- 项目授权和选择以启动时可用的 profile 池为准；`database` 配置变更需重启 Server 生效。其他配置加载行为不在本计划中调整。
