# MEM012
mem012 是一个面向 AI Agent 的 CLI 记忆系统，提供持久化记忆与 RAG 检索能力，并支持通过 Web 端管理记忆。

## 1. PostgreSQL

### 1.1 构建 PostgreSQL 镜像

```bash
docker build -t mem012-postgres:pg18 -f docker/postgres/Dockerfile docker/postgres
```

### 1.2 持久化启动

```bash
export MEM012_ADMIN_POSTGRES_USER='mem012_admin'
export MEM012_POSTGRES_PASSWORD='your_admin_password'

docker run -d \
  --name mem012-postgres \
  --restart unless-stopped \
  --network 1panel-network \
  -p 5632:5432 \
  -e POSTGRES_USER="$MEM012_ADMIN_POSTGRES_USER" \
  -e POSTGRES_PASSWORD="$MEM012_POSTGRES_PASSWORD" \
  -v mem012_pg18_data:/var/lib/postgresql \
  mem012-postgres:pg18
```

镜像只提供 PostgreSQL 与 `vector / pg_trgm / age` 扩展能力；管理员账号由 `docker run` 的 `POSTGRES_USER / POSTGRES_PASSWORD` 决定，不会自动创建 profile。

## 2. 编译并安装

```bash
sh install.sh
```

## 3. 创建 profile

每个agent可以独享一个profile，实现记忆隔离。首次创建某个 profile 时，显式运行：

```bash
export MEM012_PROFILE='codex'
export MEM012_ADMIN_DATABASE_URL="postgresql://${MEM012_ADMIN_POSTGRES_USER}:${MEM012_POSTGRES_PASSWORD}@127.0.0.1:5632/postgres"
mem012 --create_profile "$MEM012_PROFILE"
```

## 4. 验证

验证第 3 步创建的 profile database。

```bash
docker exec -e PGPASSWORD="$MEM012_POSTGRES_PASSWORD" mem012-postgres \
  psql -U "$MEM012_ADMIN_POSTGRES_USER" -d "mem_${MEM012_PROFILE}" \
  -c "select name, installed_version from pg_available_extensions where name in ('vector', 'pg_trgm', 'age');"
```

`installed_version` 不为空，才表示当前数据库已启用该扩展。

## 5. 设置初始化记忆

1. 启动 `mem012 server`，打开 `http://127.0.0.1:37777/auth` 获取 5 分钟有效的 `auth_token`。
2. 同一用户环境执行 `mem012 --profile <profile> --auth <auth_token>`，生成临时授权文件 `~/.auth/auth_file.mem`。
3. 通过 `create_memory` 创建类别位`init` 的记忆，会在初始化后读取。

## 6. SOUL.md

下面的这段话加入全局引导文件。

```text
## INIT
初始化触发条件：仅限首次对话，或上下文压缩后的首次对话。其余情况切勿重复执行。
我的profile: codex.
mem012 是我的记忆系统。启动后，我必须先执行 shell 命令 `mem012 --profile codex init`，完整读取返回内容，完成初始化后再继续处理用户请求。
```

## 7. SKILL && mem012_prompt

[SKILL.md](SKILL.md)

[mem012_prompt.md](mem012_prompt.md)
