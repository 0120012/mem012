# Frontend Placeholder API Plan

## Context
- 本阶段只处理前端路由支持
- 本阶段不考虑 CLI 开发
- 本阶段不考虑 MCP 开发
- 单一 bin：`llm_memory`
- HTTP 启动后常驻监听 `37777`
- `.env` 中已有 `API_TOKEN`，本阶段保留 Bearer Token 鉴权
- 前端静态站由 1Panel 提供
- Rust 只负责前端需要的 HTTP API
- Rust Web 依赖已经存在：
  - `axum = { version = "0.8", features = ["json", "macros"] }`
  - `tokio = { version = "1", features = ["rt-multi-thread", "macros", "signal"] }`
  - `serde = { version = "1", features = ["derive"] }`
  - `serde_json = "1"`
- 前端主 API 命名采用：
  - `/api/health`
  - `/api/health/profiles`
  - `/api/memory/*`
  - `/api/review/*`
  - `/api/cleanup/*`
- 当前先做占位返回
- 认证流采用单独探针接口：`GET /api/auth/verify`
- 未认证时前端应跳转到 sign in page，并保留 return_to
- 占位返回的目标不是接数据库，而是先让前端页面能正常请求、正常打开、正常渲染基础结构
- Why：先把前端页面依赖的 API 形状固定，再进入真实数据接入，避免路由名和返回结构反复漂移

## Structure
```text
llm_memory  :37777
└─ /api
   ├─ /auth/verify
   ├─ /health
   ├─ /health/profiles
   ├─ /memory/*
   ├─ /review/*
   └─ /cleanup/*

frontend
└─ static site by 1Panel
   └─ 调用 /api/... 获取占位数据
```

## Boundary
- 前端不参与工具调用
- 本阶段不实现 CLI
- 本阶段不实现 MCP
- 本阶段不接数据库
- 本阶段不追求真实数据
- 本阶段保留 Token 鉴权，但不接第三方登录
- 本阶段只保证：路由存在、返回结构稳定、前端页面可进入
- Why：当前目标是先打通页面访问链路，不是提前做执行层和存储层

## Frontend Route Inventory
- `GET /api/auth/verify`
- `GET /api/health`
- `GET /api/health/profiles`
- `GET /api/memory/domains`
- `GET /api/memory/node`
- `PUT /api/memory/node`
- `GET /api/memory/glossary`
- `POST /api/memory/glossary`
- `DELETE /api/memory/glossary`
- `GET /api/review/groups`
- `GET /api/review/groups/{node_uuid}/diff`
- `POST /api/review/groups/{node_uuid}/rollback`
- `DELETE /api/review/groups/{node_uuid}`
- `DELETE /api/review`
- `GET /api/cleanup/orphans`
- `GET /api/cleanup/orphans/{memory_id}`
- `DELETE /api/cleanup/orphans/{memory_id}`
- Why：先把前端页面真实会打到的路由列全，后面每次只做一个最小接口

## Placeholder Response Design
- `GET /api/auth/verify`
  - 成功：`200 {"ok": true, "authenticated": true}`
  - 失败：`401 {"detail": "Unauthorized"}`
  - 用途：只做 Bearer Token 探针，不承担页面数据读取
  - Why：把“认证失败”和“普通数据接口失败”拆开，避免前端把 404/500 误判成需要重新登录
- `GET /api/health`
  - 成功：`200 {"status": "ok", "database": "unconfigured"}`
  - 用途：前端与运维探测服务是否可达
  - Why：占位阶段先明确服务在线，但不假装数据库已经接通
- `GET /api/health/profiles`
  - 成功：`200 {"profiles": [], "default_profile": null}`
  - 用途：前端读取可用 profile 列表与默认 profile
  - Why：前端初始化依赖固定字段名，先返回空集合与空默认值，避免占位阶段结构漂移
- `GET /api/memory/domains`
  - 成功：`200 []`
  - 用途：前端读取可用 domain 列表
  - Why：空数组是前端可直接消费的最小稳定结构，不会把占位阶段误判成接口异常
- `GET /api/memory/node`
  - 成功：`200 {"node": {...}, "children": [], "breadcrumbs": []}`
  - `node` 最小字段：
    - `path`
    - `domain`
    - `uri`
    - `name`
    - `content`
    - `priority`
    - `disclosure`
    - `created_at`
    - `is_virtual`
    - `aliases`
    - `node_uuid`
    - `glossary_keywords`
    - `glossary_matches`
  - 用途：前端详情页与侧栏共用的节点读取接口
  - Why：先把详情页依赖的固定骨架补齐，避免前端把空对象和缺字段状态当成异常
- `PUT /api/memory/node`
  - 成功：`200 {"ok": true}`
  - 用途：前端保存节点编辑结果
  - Why：当前前端保存后会主动重新读取节点，写接口先返回稳定成功标记即可
- `GET /api/memory/glossary`
  - 成功：`200 []`
  - 用途：前端读取 glossary 列表
  - Why：空数组是最小稳定结构，先保证前端能安全消费，再接真实关键词数据
- `POST /api/memory/glossary`
  - 成功：`200 {"ok": true}`
  - 用途：前端新增 glossary 关键词
  - Why：前端新增后会主动刷新节点数据，写接口先返回稳定成功标记即可
- `DELETE /api/memory/glossary`
  - 成功：`200 {"ok": true}`
  - 用途：前端删除 glossary 关键词
  - Why：前端删除后也会主动刷新节点数据，写接口先返回稳定成功标记即可
- `GET /api/review/groups`
  - 成功：`200 []`
  - 用途：前端读取待审核分组列表
  - Why：空数组是前端列表页可直接消费的最小稳定结构，不会把占位阶段误判成接口异常
- `GET /api/review/groups/{node_uuid}/diff`
  - 成功：`200 {"action":"modified","has_changes":false,...}`
  - 最小字段：
    - `action`
    - `has_changes`
    - `before_meta`
    - `current_meta`
    - `path_changes`
    - `active_paths`
    - `glossary_changes`
    - `before_content`
    - `current_content`
  - 用途：前端读取单个审核分组的 diff 详情
  - Why：先提供“无变更”骨架，保证审核详情页所有分支都能稳定渲染
- `POST /api/review/groups/{node_uuid}/rollback`
  - 成功：`200 {"success": true}`
  - 用途：前端回滚一个审核分组
  - Why：前端只需要明确成功标记，随后会重新拉取列表，不依赖更复杂的响应正文
- `DELETE /api/review/groups/{node_uuid}`
  - 成功：`200 {"ok": true}`
  - 用途：前端批准并移除一个审核分组
  - Why：前端批准后会重新拉取列表，写接口先返回稳定成功标记即可
- `DELETE /api/review`
  - 成功：`200 {"ok": true}`
  - 用途：前端清空全部审核分组
  - Why：前端清空后会直接重置本地状态，写接口先返回稳定成功标记即可
- `GET /api/cleanup/orphans`
  - 成功：`200 []`
  - 用途：前端读取待清理记忆列表
  - Why：空数组能让前端稳定进入“无 orphan 数据”分支，不会把占位阶段误判成接口异常
- `GET /api/cleanup/orphans/{memory_id}`
  - 成功：`200 {"content": "", "migration_target": null}`
  - 用途：前端读取单条待清理记忆的展开详情
  - Why：详情区至少依赖 `content`，最小骨架先保证展开交互可运行
- `DELETE /api/cleanup/orphans/{memory_id}`
  - 成功：`200 {"ok": true}`
  - 用途：前端删除单条待清理记忆
  - Why：前端删除后会直接刷新本地列表，写接口先返回稳定成功标记即可

## Checklist
- [x] 重写计划：本阶段冻结 CLI / MCP 范围，只保留前端占位 API
- [x] 固化前端资源型路由清单
- [x] 将 Bearer Token 鉴权与 `GET /api/auth/verify` 预留路由写入计划
- [x] 设计 `GET /api/auth/verify` 的最小占位返回
- [x] 在 Rust 中落 `GET /api/auth/verify` 占位路由壳
- [x] 设计 `GET /api/health` 的最小占位返回
- [x] 设计 `GET /api/health/profiles` 的最小占位返回
- [x] 设计 `GET /api/memory/domains` 的最小占位返回
- [x] 设计 `GET /api/memory/node` 的最小占位返回
- [x] 设计 `PUT /api/memory/node` 的最小占位返回
- [x] 设计 `GET /api/memory/glossary` 的最小占位返回
- [x] 设计 `POST /api/memory/glossary` 的最小占位返回
- [x] 设计 `DELETE /api/memory/glossary` 的最小占位返回
- [x] 设计 `GET /api/review/groups` 的最小占位返回
- [x] 设计 `GET /api/review/groups/{node_uuid}/diff` 的最小占位返回
- [x] 设计 `POST /api/review/groups/{node_uuid}/rollback` 的最小占位返回
- [x] 设计 `DELETE /api/review/groups/{node_uuid}` 的最小占位返回
- [x] 设计 `DELETE /api/review` 的最小占位返回
- [x] 设计 `GET /api/cleanup/orphans` 的最小占位返回
- [x] 设计 `GET /api/cleanup/orphans/{memory_id}` 的最小占位返回
- [x] 设计 `DELETE /api/cleanup/orphans/{memory_id}` 的最小占位返回
- [x] 在 Rust 中落第一批占位路由壳
- [x] 补最小 API 可达性测试
