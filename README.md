# MEM012
mem012 是一个面向 AI Agent 的 CLI 记忆系统，提供持久化记忆与 RAG 检索能力，并支持通过 Web 端管理记忆。

## 0. Agent安装

给其他 Agent 接入 mem012 时，需要把 `mem012` 安装到系统 `PATH`，并让 Agent 读取本仓库的 `SKILL.md` 作为工具调用说明。新 profile 先由 `mem012 --create_profile <profile>` 创建；日常初始化读取执行 `mem012 --profile <profile> init`；创建、搜索、读取、更新、删除记忆都通过 `mem012 --profile <profile> --args '<json_object>'` 调用。

## 1. PostgreSQL

### 1.1 构建 PostgreSQL 镜像

```bash
docker build -t mem012-postgres:pg18 -f docker/postgres/Dockerfile docker/postgres
```

### 1.2 持久化启动

```bash
export MEM012_POSTGRES_USER='mem012_admin'
export MEM012_POSTGRES_PASSWORD='your_admin_password'

docker run -d \
  --name mem012-postgres \
  --restart unless-stopped \
  --network 1panel-network \
  -p 5632:5432 \
  -e POSTGRES_USER="$MEM012_POSTGRES_USER" \
  -e POSTGRES_PASSWORD="$MEM012_POSTGRES_PASSWORD" \
  -v mem012_pg18_data:/var/lib/postgresql \
  mem012-postgres:pg18
```

镜像只提供 PostgreSQL 与 `vector / pg_trgm / age` 扩展能力；管理员账号由 `docker run` 的 `POSTGRES_USER / POSTGRES_PASSWORD` 决定，不会自动创建 profile。

## 2. 安装到系统

```bash
sh install.sh
```

## 3. 创建 profile

首次使用某个 profile 时，显式运行：

```bash
export MEM012_ADMIN_DATABASE_URL="postgresql://${MEM012_POSTGRES_USER}:${MEM012_POSTGRES_PASSWORD}@127.0.0.1:5632/postgres"
mem012 --create_profile <profile>
```

`MEM012_ADMIN_DATABASE_URL` 必须指向 PostgreSQL 管理员账号：该账号需要能创建 role、创建 database、在新库启用 `vector / pg_trgm / age` 扩展，并向新 profile 授权 schema/table/sequence/default privileges。使用上方 Docker `POSTGRES_USER` 创建的账号通常满足该前提；外部 PostgreSQL 不能只使用普通业务账号。

`--create_profile` 会创建 role/database、启用扩展、初始化 mem012 表结构，并把 profile 连接串追加到 `config.toml`。日常 `mem012 --profile <profile> init` 只读取 `category=init` 的初始化记忆。配置中的 `reset_db=true` 只会在 `create_memory` 写入前重置当前 profile 的记忆表并重新建表，只能用于本地调试。

`share` 是普通 profile 名，需要共享库时手动执行 `mem012 --create_profile share` 创建，之后用 `mem012 --profile share ...` 显式访问；普通 profile 启动时不会自动创建或连接 `mem_share`。

## 4. Init 授权

1. 启动服务并登录 Web，打开 `/auth`，点击获取按钮拿到 5 分钟有效的 `auth_token`。
2. 在同一用户环境执行 `mem012 --profile <profile> --auth <auth_token>`，写入 `~/.auth/auth_file.mem`。
3. 执行一次 `category=init` 的 `create_memory` 会消费并删除该 auth file；重复写入需要重新授权。

## 5. 验证

```bash
docker exec -e PGPASSWORD="$MEM012_POSTGRES_PASSWORD" mem012-postgres \
  psql -U "$MEM012_POSTGRES_USER" -d "mem_<profile>" \
  -c "select name, installed_version from pg_available_extensions where name in ('vector', 'pg_trgm', 'age');"
```

`installed_version` 不为空，才表示当前数据库已启用该扩展。
