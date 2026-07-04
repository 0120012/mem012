# MEM012
mem012 是一个面向 AI Agent 的 CLI 记忆系统，提供持久化记忆与 RAG 检索能力，并支持通过 Web 端管理记忆。

## 0. Agent安装

给其他 Agent 接入 mem012 时，需要把 `mem012` 安装到系统 `PATH`，并让 Agent 读取本仓库的 `SKILL.md` 作为工具调用说明。首次使用某个 profile 或升级后迁移 schema 时，先执行 `mem012 --profile <profile> dbsetup`；日常初始化读取执行 `mem012 --profile <profile> init`；创建、搜索、读取、更新、删除记忆都通过 `mem012 --profile <profile> --args '<json_object>'` 调用。

## 1. PostgreSQL

### 1.1 构建 PostgreSQL 镜像

```bash
docker build -t mem012-postgres:pg18 -f docker/postgres/Dockerfile docker/postgres
```

### 1.2 持久化启动

```bash
export MEM012_UUTEST_PASSWORD='your_password'

docker run -d \
  --name mem012-postgres \
  --restart unless-stopped \
  --network 1panel-network \
  -p 5632:5432 \
  -e POSTGRES_PASSWORD="$MEM012_UUTEST_PASSWORD" \
  -v mem012_pg18_data:/var/lib/postgresql \
  mem012-postgres:pg18
```

首次初始化会自动创建 `postgres` 管理库和 `mem_riko / mem_nous / mem_claw / mem_doge / mem_share / mem_codex`；profile 库会启用 `vector / pg_trgm / age`。

## 2. 安装到系统

```bash
sh install.sh
```

## 3. 数据库 schema 初始化

首次使用某个 profile，或升级后需要迁移数据库 schema 时，显式运行：

```bash
mem012 --profile <profile> dbsetup
```

`dbsetup` 负责执行 schema 初始化/迁移；日常 `mem012 --profile <profile> init` 只读取 `category=init` 的初始化记忆，普通 `--args` 工具调用也不会自动迁移数据库。配置中的 `reset_db=true` 只在 `dbsetup` 时生效。

## 4. Init 授权

1. 启动服务并登录 Web，打开 `/auth`，点击获取按钮拿到 5 分钟有效的 `auth_token`。
2. 在同一用户环境执行 `mem012 --auth <auth_token>`，写入 `~/.auth/auth_file.mem`。
3. 执行一次 `category=init` 的 `create_memory` 会消费并删除该 auth file；重复写入需要重新授权。

## 5. 验证

```bash
docker exec mem012-postgres psql -U uutest -d mem_riko -c "select name, installed_version from pg_available_extensions where name in ('vector', 'pg_trgm', 'age');"
```

`installed_version` 不为空，才表示当前数据库已启用该扩展。
