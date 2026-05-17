# V2 前端工程计划

## Scope

- 本文件只负责前端工程、页面、组件和视觉设计。
- 后端接口设计与 Rust 实现见 `BACKEND_PLAN.md`。

## Required Stack

```text
Vite
React
shadcn/ui
```

其余依赖暂不固定。

## Directory

前端工程目录固定为：

```text
frontend/
```

不要使用 `fortend/`。

第一版目录边界：

```text
mem012/
  src/          # Rust 后端
  frontend/    # Vite + React + shadcn/ui 前端
```

Node 依赖放在 `frontend/package.json`，不要放到 Rust 根目录。

## Visual Constraints

```text
黑白灰配色
不使用品牌色
不使用渐变
不使用装饰插画
```

## 响应式 / 移动端

- 页面必须适配手机端，目标设备：iPhone 14 + iOS 18 Safari。
- 使用 Tailwind 响应式断点（sm/md/lg），以移动端优先设计布局。
- 触控目标不小于 44x44px；间距与字号在移动端保持不变或适当缩小，避免溢出。

## API Dependency

前端依赖后端提供：

```text
POST /auth/verify
GET  /auth/session
GET  /projects
GET  /memories
GET  /changes
GET  /changes/{memory_uuid}
POST /changes/{memory_uuid}/approve
POST /changes/{memory_uuid}/reject
```

当前 project 通过请求头传递：

```text
X-Mem-Project: riko
```

## Frontend Checklist

- [x] 确定前端工程目录位置：`frontend/`。
- [x] 创建 Vite + React + shadcn/ui 工程。
- [x] 设计前端路由。
- [x] 设计前端页面结构。
- [x] 设计前端组件结构。
- [x] 设计黑白灰视觉规范。
- [x] 对接后端 auth/project/memory/change API。
- [ ] 后端接口完成后，联调 `POST /auth/verify`。
- [ ] 后端接口完成后，联调 `GET /auth/session`。
- [ ] 后端接口完成后，联调 `GET /projects`。
- [ ] 后端接口完成后，联调 `GET /memories`。
- [ ] 后端接口完成后，联调 `GET /changes`。
- [ ] 后端接口完成后，联调 `GET /changes/{memory_uuid}`。
- [ ] 后端接口完成后，联调 approve / reject 操作。
- [ ] 补齐 API loading / empty / error 状态展示。
- [ ] 用真实后端数据检查移动端可用性。

## Later

- [ ] before_state / after_state diff。
- [ ] memory detail 页面。
- [ ] 分页、搜索、category/status 筛选。
