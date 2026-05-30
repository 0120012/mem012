# mem012

PostgreSQL 后端的记忆系统。当前 Rust CLI 支持 create / search / read / update / delete；approve / reject 通过 HTTP API 完成。

CLI 文档见 [docs/TOOLS.md](docs/TOOLS.md) 和 [docs/cli/skill.md](docs/cli/skill.md)。

## 安装到系统 bin

```bash
sh install.sh
```

## Init 授权

1. 启动服务并登录 Web，打开 `/auth`，通过 Turnstile 获取 5 分钟有效的 `auth_token`。
2. 在同一用户环境执行 `mem012 --auth <auth_token>`，写入 `~/.auth/auth_file.mem`。
3. 执行一次 `category=init` 的 `create_memory` 会消费并删除该 auth file；重复写入需要重新授权。

## 构建 PostgreSQL 镜像

```bash
docker build -t mem012-postgres:pg18 -f docker/postgres/Dockerfile docker/postgres
```

## 持久化启动

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

## 验证：

```bash
docker exec mem012-postgres psql -U uutest -d mem_riko -c "select name, installed_version from pg_available_extensions where name in ('vector', 'pg_trgm', 'age');"
```

`installed_version` 不为空，才表示当前数据库已启用该扩展。
