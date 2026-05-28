# AUTH

## 目标

保护 `category = init` 的写入权限，同时避免把长期密钥写入 config、skill、环境变量或命令历史。

`init` 内容用于 Agent 初始化、工具引导和 skill 引导。Agent 可以读取 `init`，但写入 `init` 必须由用户临时授权。

## 角色

- `/auth` 页面：只在登录 session 存在时可访问；通过 Turnstile 后展示短期 `auth_token`。
- 用户：从 `/auth` 页面复制 `auth_token`，手动授权本机 CLI。
- CLI：执行 `mem012 --auth <auth_token>`，换取本机临时授权文件。
- 后端 API：验证 Turnstile、签发 `auth_token`、签发 300s Ed25519 单活一次性 grant，并消费 grant。
- auth file：本机短期授权凭据，路径固定为 `~/.auth/auth_file.mem`。

## `/auth` 页面

页面初始只显示 Cloudflare Turnstile，不展示 token 区域。

Turnstile 使用官方 explicit rendering：

```html
<script src="https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit" defer></script>
```

前端使用公开 site key：

```text
0x4AAAAAADXWPWveDjEIZ8XK
```

Turnstile 成功后，前端拿到 challenge token，并调用：

```http
POST /api/auth/refresh
```

只有后端返回 `auth_token` 后，页面才展示 token、倒计时和复制按钮。`auth_token` 有效期为 180s。

如果用户已经获取过 `auth_token` 但没有复制，随后刷新页面并再次完成 Turnstile，第二次 refresh 会签发新的 `auth_token`。旧 `auth_token` 和旧 grant 会立即失效，即使它们还没过期。页面刷新本身不生成 token；只有 Turnstile 通过后的 refresh 成功才会轮换授权。

页面每 3-5s 轮询：

```http
GET /api/auth/status
```

轮询只用于确认当前 token 是否仍有效。轮询不能生成新 token，也不能返回 token 明文。token 过期、刷新失败、被 CLI 换 grant、grant 被 consume 或后端状态失效时，页面必须立即隐藏旧 token，并要求用户重新通过 Turnstile。

## Turnstile 后端验证

后端按 Cloudflare Siteverify 验证前端提交的 Turnstile token：

```http
POST https://challenges.cloudflare.com/turnstile/v0/siteverify
```

请求字段：

- `secret`：Turnstile secret key，只能放在本地 `config.toml`
- `response`：前端 Turnstile challenge token。
- `remoteip`：可选。

响应必须检查 `success`。失败时读取 `error-codes` 并拒绝签发 `auth_token`。

仓库只允许提交占位配置和公开 site key，不提交真实 secret key。

## API 合同

`POST /api/auth/refresh`

- 需要登录 session。
- 请求：`{ "turnstile_token": "..." }`
- 行为：后端 Siteverify 通过后生成 256-bit `auth_token`，并废弃旧 token 和所有未消费 grant。
- 响应：`{ "auth_token": "...", "expires_at": 1760000180 }`
- TTL：180s。

`GET /api/auth/status`

- 需要登录 session。
- 行为：只返回当前 token 状态，不生成新 token。
- 响应：`{ "valid": true, "expires_at": 1760000180 }` 或 `{ "valid": false, "expires_at": null }`
- 禁止返回 token 明文。

`POST /api/auth/grant`

- CLI 调用，不需要浏览器 session。
- 请求：`{ "auth_token": "..." }`
- 行为：验证 `auth_token` 成功后立即废弃该 token，废弃旧 grant，并返回新的 300s Ed25519 grant。
- 失败：不签发 grant。

`POST /api/auth/grant/consume`

- CLI 在 `create_memory category=init` 时调用。
- 请求：auth file 中的完整 grant JSON。
- 行为：验签、检查 `scope`、检查过期时间、检查服务端 grant 状态，并一次性消费。
- 请求无法解析或未通过当前服务签名校验时不改变服务端授权状态；已通过验签但过期、状态不匹配、重复消费或消费成功时，废弃该 grant，并让前端 token 状态失效。

## grant 格式

`auth_token`、`grant_id`、`nonce` 都是 256-bit CSPRNG 随机值，使用 Base64URL no padding 编码。

grant 是 Ed25519 签名票据，签名覆盖 `payload` 的稳定 JSON 字节：

```json
{
  "version": 1,
  "payload": {
    "grant_id": "base64url-256bit-random",
    "scope": "init:create",
    "iat": 1760000000,
    "exp": 1760000300,
    "nonce": "base64url-256bit-random"
  },
  "signature": "base64url-ed25519-signature"
}
```

服务端内存状态保存当前有效 `grant_id` 的有效期、scope 和 consumed 状态。v1 中 grant 是单活授权：新 `auth_token` 生成或新 grant 签发时，旧的未消费 grant 必须立即失效。Ed25519 keypair 在服务进程启动时生成，服务重启后旧 grant 全部失效。

## auth file

路径固定：

```text
~/.auth/auth_file.mem
```

内容是完整 Ed25519 grant JSON，不保存前端原始 `auth_token`。

约束：

- `~/.auth` 目录权限应为 `0700`。
- `auth_file.mem` 文件权限应为 `0600`。
- 文件内容不能为空。
- `payload.scope` 必须是 `init:create`。
- `payload.exp` 必须是签发后 300s。
- 文件存在不代表授权有效，必须通过后端 API 验证 grant。
- grant 只能使用一次；已通过当前服务签名校验后的成功、过期、状态拒绝、刷新 token 或签发新 grant 后都必须废弃。

## 授权流程

1. 用户登录后访问 `/auth`。
2. 页面显示 Turnstile，不展示 token。
3. Turnstile 成功后，页面调用 `POST /api/auth/refresh`。
4. 后端 Siteverify 通过后返回 180s `auth_token`。
5. 用户执行：

```bash
mem012 --auth <auth_token>
```

6. CLI 调用 `POST /api/auth/grant`。
7. 后端验证 `auth_token`，成功后废弃该 token 和旧 grant，并返回新的 300s Ed25519 grant。
8. CLI 创建 `~/.auth/auth_file.mem`，写入 grant JSON。
9. Agent 使用普通 create 命令写入 `init`：

```bash
mem012 --profile riko --args '{"tool":"create_memory","params":{"category":"init","title":"标题","content":"正文","keywords":["init"]}}'
```

所有新建记忆都应显式带 `keywords`。`category = init` 时，CLI 内部仍必须确保 keywords 包含 `init`；用户已传 `init` 时不重复追加。

10. `create_memory` 发现 `category = init` 后，读取 `~/.auth/auth_file.mem`，并调用 `POST /api/auth/grant/consume`。
11. grant 验证通过后才允许写入。
12. CLI 收到 consume 结果后必须先删除 `~/.auth/auth_file.mem`，再继续数据库写入或返回错误；后续数据库写入失败也不保留 auth file。

## create_memory 规则

`create_memory` 写入 `category = init` 时按顺序检查：

1. `init` 必须存在于 `[categories].index_list`。
2. `~/.auth/auth_file.mem` 必须存在且非空。
3. 创建 init 记忆时，程序内部必须自动确保 keywords 包含 `init`；用户已传 `init` 时不重复追加，不需要向用户额外提示。
4. auth file 内 grant 必须通过后端 API 验签、查状态并消费。
5. 后端确认请求携带当前服务签发的 grant 后，无论消费通过、过期或状态拒绝，都立即废弃 grant，并让前端 token 状态失效；无法解析或未验签的请求不影响现有授权状态。
6. CLI 无论验证通过或失败，都立即删除 auth file。
7. 只有 grant 验证通过后，才执行正常写入流程。

如果缺少 auth file，返回错误：

```text
写入 category=init 需要 auth file: ~/.auth/auth_file.mem；请向用户申请授权后重试
```

## 非目标

- 不在 skill 文档里写 `mem012 --auth`。
- 不允许 Agent 自己生成或刷新授权。
- 不使用长期 `server.api_token` 作为 CLI 写入 `init` 的授权凭据。
- 旧 `--admin_auth` 方案不再使用。

## 参考

- Cloudflare Turnstile client-side rendering: <https://developers.cloudflare.com/turnstile/get-started/client-side-rendering/>
- Cloudflare Turnstile server-side validation: <https://developers.cloudflare.com/turnstile/get-started/server-side-validation/>
