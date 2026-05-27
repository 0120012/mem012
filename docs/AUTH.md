# AUTH

## 目标

保护 `category = init` 的写入权限，同时避免把长期密钥写入 config、skill、环境变量或命令历史。

`init` 内容用于 Agent 初始化、工具引导和 skill 引导。Agent 可以读取 `init`，但写入 `init` 必须由用户临时授权。

## 角色

- 前端 auth 页面：展示短期 `auth_token`，每 180s 自动刷新；后端验证或消费授权后也必须立即刷新。
- 用户：从前端页面复制 `auth_token`，手动授权本机 CLI。
- CLI：执行 `mem012 --auth <auth_token>`，换取本机临时授权文件。
- 后端 API：验证前端 `auth_token`，签发 300s 的 Ed25519 一次性 grant，并在 grant 消费后立即废弃 grant。
- auth file：本机短期授权凭据，路径固定为 `~/.auth/auth_file.mem`。

## 算法

- `auth_token`：256-bit CSPRNG 随机值，Base64URL no padding 编码，只用于换取 grant。
- `grant_id` 和 `nonce`：256-bit CSPRNG 随机值，Base64URL no padding 编码。
- grant：Ed25519 签名票据，签名覆盖 `payload` 的稳定 JSON 字节。
- 服务端状态：保存 `grant_id` 的有效期、scope 和 consumed 状态，用于一次性消费和撤销。

## 授权流程

1. 前端 auth 页面生成并展示 `auth_token`，有效期 180s。
2. 用户执行：

```bash
mem012 --auth <auth_token>
```

3. CLI 调用后端 API 验证 `auth_token`。
4. 验证成功后，后端立即返回一个短期 Ed25519 grant，grant 有效期 300s。
5. CLI 创建 `~/.auth/auth_file.mem`，写入 grant 信息。
6. Agent 使用普通 create 命令写入 `init`，命令不再传 `--admin_auth`：

```bash
mem012 --profile riko --args '{"tool":"create_memory","params":{"category":"init","title":"标题","content":"正文","keywords":["init"]}}'
```

7. `create_memory` 发现 `category = init` 后，读取 `~/.auth/auth_file.mem`，并调用后端 API 验证 grant。
8. grant 验证通过后才允许写入。
9. 无论 grant 验证成功、失败、过期或 API 拒绝，后端都立即废弃 grant，并刷新前端 `auth_token`。
10. CLI 收到验证结果后立即删除 `~/.auth/auth_file.mem`；后续数据库写入失败也不保留 auth file。

## auth file

路径固定：

```text
~/.auth/auth_file.mem
```

内容是完整 Ed25519 grant 票据 JSON，不保存前端原始 `auth_token`：

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

约束：

- `~/.auth` 目录权限应为 `0700`。
- `auth_file.mem` 文件权限应为 `0600`。
- 文件内容不能为空。
- `payload.scope` 必须是 `init:create`。
- `payload.exp` 必须是签发后 300s。
- 文件存在不代表授权有效，必须通过后端 API 验证 grant。
- grant 只能使用一次，验证成功、验证失败、过期或 API 拒绝后都必须废弃。
- 不把前端页面的原始 `auth_token` 长期写入 auth file；`auth_token` 只用于换取 grant。

## create_memory 规则

`create_memory` 写入 `category = init` 时按顺序检查：

1. `init` 必须存在于 `[categories].index_list`。
2. `~/.auth/auth_file.mem` 必须存在且非空。
3. auth file 内 Ed25519 grant 必须通过后端 API 验签、查状态并消费。
4. 后端完成 grant 验证后，无论通过或失败，都立即废弃 grant，并刷新前端 `auth_token`。
5. CLI 无论验证通过或失败，都立即删除 auth file。
6. 只有 grant 验证通过后，才执行正常写入流程。

如果缺少 auth file，返回错误：

```text
写入 category=init 需要 auth file: ~/.auth/auth_file.mem；请向用户申请授权后重试
```

## 非目标

- 不在 skill 文档里写 `mem012 --auth`。
- 不允许 Agent 自己生成或刷新授权。
- 不使用长期 `server.api_token` 作为 CLI 写入 `init` 的授权凭据。
- 不通过 `--admin_auth` 在每次 `create_memory` 命令中传 token。
