# Frontend Support Notes

> 这份文档只讲一件事：**前端到底需要后端提供什么 API 支持。**

## 1. 先把角色说清楚

前端不是工具调用端。

前端的角色是：

- 给人类查看记忆库内容
- 给人类浏览树结构
- 给人类审核变更
- 给人类维护 orphan / deprecated 数据

所以前端**不应该**：

- 直接调用 CLI 工具
- 直接构造 `tool=create_memory` 这类 payload
- 直接参与 CLI 工具分发

一句话：

- **CLI / tool** 面向程序内部或 agent
- **frontend** 面向人类查看和管理记忆库

## 2. 前端现在实际依赖什么

前端入口在：

- [frontend/src/lib/api.js](/Users/vw/git/vcspace/012MEM/frontend/src/lib/api.js:1)

它固定走：

```js
baseURL: '/api'
```

并自动附加：

- `Authorization`
- `X-DB-Profile`

当前前端页面实际依赖的是这几组 HTTP 路由：

### 2.1 Health

- `GET /api/health`
- `GET /api/health/profiles`

### 2.2 Memory

- `GET /api/memory/domains`
- `GET /api/memory/node`
- `PUT /api/memory/node`
- `GET /api/memory/glossary`
- `POST /api/memory/glossary`
- `DELETE /api/memory/glossary`

### 2.3 Review

- `GET /api/review/groups`
- `GET /api/review/groups/{node_uuid}/diff`
- `POST /api/review/groups/{node_uuid}/rollback`
- `DELETE /api/review/groups/{node_uuid}`
- `DELETE /api/review`

### 2.4 Cleanup

- `GET /api/cleanup/orphans`
- `GET /api/cleanup/orphans/{memory_id}`
- `DELETE /api/cleanup/orphans/{memory_id}`

## 3. 这意味着什么

这意味着：

- 前端已经有代码
- 前端需要的不是“工具接口”
- 前端需要的是 **`backend/api` 这层 HTTP 支持**

所以如果后面做单 bin、`37777` 端口、统一服务，
前端仍然应该只看到：

```text
/api/health
/api/health/profiles
/api/memory/*
/api/review/*
/api/cleanup/*
```

而不是：

```text
/api/cli/v9/exec
```

`/api/cli/v9/*` 可以存在，但那不是前端主合同。

## 4. 前端支持层应该怎么理解

`backend/api` 是前端支持层。

它的职责是：

- 把前端需要的资源型接口暴露出来
- 返回前端页面当前能消费的结构
- 在后端内部决定是否调用共享逻辑

它**不是**：

- 直接把 CLI 工具协议原样暴露给前端
- 强迫前端去理解 tool / args / dispatcher

所以未来即使后端内部变成：

```text
frontend -> backend/api -> shared core -> storage
```

前端也不应该知道 shared core 是不是 CLI 风格。

## 5. 如果现在只做占位 API

如果现在只是为了先把服务起起来、让前端访问不炸，
那占位重点也应该是：

- 占位 **前端当前用到的这些 `/api/...` 路由**
- 不是先占位工具执行接口

## 6. 占位返回最小要求

占位时不能只想“返回个 ok”，还要考虑前端当前页面会不会因为字段缺失直接报错。

所以占位要分两类：

### 6.1 可以只返回 `ok` 的接口

这类通常是动作型接口，前端只关心成功失败：

- `POST /api/review/groups/{node_uuid}/rollback`
- `DELETE /api/review/groups/{node_uuid}`
- `DELETE /api/review`
- `DELETE /api/memory/glossary`
- `POST /api/memory/glossary`
- `DELETE /api/cleanup/orphans/{memory_id}`

这类最小可以先返回：

```json
{"ok": true}
```

### 6.2 不能只返回 `ok` 的接口

这类页面会直接读取结构字段，不能只给一个 `ok`：

- `GET /api/health/profiles`
- `GET /api/memory/domains`
- `GET /api/memory/node`
- `GET /api/review/groups`
- `GET /api/review/groups/{node_uuid}/diff`
- `GET /api/cleanup/orphans`
- `GET /api/cleanup/orphans/{memory_id}`

这些接口即使先占位，也要返回页面不会崩的最小结构。

## 7. 推荐的最小占位结构

### 7.1 `GET /api/health`

```json
{
  "status": "ok",
  "database": "connected"
}
```

### 7.2 `GET /api/health/profiles`

```json
{
  "profiles": [],
  "default_profile": "DEFAULT"
}
```

### 7.3 `GET /api/memory/domains`

```json
[]
```

### 7.4 `GET /api/memory/node`

```json
{
  "node": {
    "path": "",
    "domain": "core",
    "uri": "core://",
    "name": "root",
    "content": "",
    "priority": 0,
    "disclosure": null,
    "created_at": null,
    "is_virtual": true,
    "aliases": [],
    "node_uuid": null,
    "glossary_keywords": [],
    "glossary_matches": []
  },
  "children": [],
  "breadcrumbs": [
    {"path": "", "label": "root"}
  ]
}
```

### 7.5 `GET /api/review/groups`

```json
[]
```

### 7.6 `GET /api/review/groups/{node_uuid}/diff`

```json
{
  "before_content": "",
  "current_content": "",
  "before_meta": {},
  "current_meta": {},
  "path_changes": [],
  "glossary_changes": [],
  "action": "modified"
}
```

### 7.7 `GET /api/cleanup/orphans`

```json
[]
```

## 8. 当前阶段最正确的顺序

如果现在目标只是“程序先起来，前端先能访问”，顺序应该是：

1. 保证单一服务启动后监听 `37777`
2. 先把 `backend/api` 这层路由挂起来
3. 给前端当前实际使用的 `/api/...` 路由补最小占位返回
4. 确保前端页面不会因为字段缺失直接炸
5. 之后再把内部逻辑逐步换成共享核心

## 9. 当前不该做的事

现在不该优先做：

- 让前端理解 CLI 工具名
- 让前端改成 tool executor
- 让前端先接 CLI exec 类接口
- 为了 CLI 工具协议先改前端页面结构

因为这会把“人类界面”和“工具协议”混在一起。

## 10. 一句话结论

前端不是工具调用层。  
前端要的是 **`backend/api` 提供的人类管理界面 API**。  
所以现在该做的是：**先占位前端实际会访问的 `/api/...` 路由**，不是先让前端接工具执行接口。
