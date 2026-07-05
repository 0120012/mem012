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



---


1. 入口先加载配置
   src/main.rs:18 进入 main()，先调用 config::load_config("config.toml")。
   load_config 会优先使用 MEM012_CONFIG，否则读默认 config.toml，然后解析成 Config。

2. 解析 CLI 参数
   src/main.rs:35 调用 parse::parse_cli_args()。
   src/parse.rs:28 遇到 --create_profile <name> 时，会校验 profile 名必须是 [a-z][a-z0-9_]*，并拒绝 postgres/template0/template1。

3. create_profile 是独占入口
   src/parse.rs:50 明确拒绝它和 --profile、顶层命令、--args、--auth 混用。
   所以 --create_profile 不会进入普通 profile database 连接和工具分发路径。

4. main 分发 create_profile
   src/main.rs:43 如果 cli_args.create_profile 存在，直接调用：
   tools::dispatch_create_profile_command(&config, create_profile).await?
   然后 return Ok(())。

5. tools 层只是转发
   src/tools/mod.rs:34 调用 create_profile::run(config, profile).await。

6. create_profile 先做本地和远端预检
   src/tools/create_profile.rs:1 进入 run()。顺序是：
   先检查 profile 是否已存在于当前配置 [database]。
   读取 MEM012_ADMIN_DATABASE_URL，并用 admin URL 建一个 max_connections(1) 的 pool。
   调用 ensure_profile_admin_resources_absent 查询远端 role 和 database 是否已存在。
   远端资源不存在后，才生成随机 profile 密码、派生连接串、计算配置路径，并生成更新后的配置文本；此时仍不落盘。

7. 配置文本追加逻辑
   src/config.rs:297 append_database_profile_text 会解析 TOML，找到 database 表。
   当前实现支持 [database] 和 database = { ... } 两种形态。
   如果 profile 已存在则报错，否则插入 profile = "<profile_database_url>"，返回新文本。

8. 连接 admin database 并创建数据库资源
   src/tools/create_profile.rs:29 调用 src/psql/profile.rs:369 apply_profile_admin_setup_sql。

9. admin setup 的 SQL 顺序
   src/psql/profile.rs:378 顺序是：
   CREATE ROLE "<profile>" LOGIN PASSWORD '<password>'
   CREATE DATABASE "mem_<profile>" OWNER "<profile>"
   REVOKE CONNECT ON DATABASE "mem_<profile>" FROM PUBLIC
   GRANT CONNECT ON DATABASE "mem_<profile>" TO "<profile>"
   注意：这里只收紧新建数据库，不再动其他数据库的 PUBLIC CONNECT。

10. 初始化新 profile database
    src/tools/create_profile.rs:29 admin setup 成功后进入 finish_result。
    它分别连接：
    admin 身份连接 mem_<profile>，用于扩展、AGE、权限 DDL。
    profile 身份连接 mem_<profile>，用于创建 mem012 主表，验证运行期权限。

11. database 内部初始化
    src/psql/profile.rs:413 initialize_profile_database_schema 先调用 admin 连接执行 profile database setup：
    创建 vector/pg_trgm/age 扩展。
    LOAD 'age'，设置 AGE search_path。
    创建 memory_graph。
    授权 public、ag_catalog、memory_graph 相关 schema/table/sequence/default privileges。
    然后调用 init_profile_memory_tables，用 profile 连接创建/迁移 memory_units、embedding、keywords、search index、relations、changes、graph meta 等 mem012 表。

12. 最后才写配置文件
    src/tools/create_profile.rs:45 schema 初始化成功后，才调用 write_config_text_atomic。
    src/config.rs:318 它写同目录临时文件，复制原配置文件权限，再 rename 覆盖，避免半写入和权限放宽。

13. 失败补偿边界
    admin setup 内部如果在创建 role/database 后失败，会调用 cleanup_profile_admin_resources。
    schema 初始化或配置写入失败时，src/tools/create_profile.rs:49 也会调用同一个 cleanup：先断开 mem_<profile> 连接，再 DROP DATABASE IF EXISTS "mem_<profile>"，最后 DROP ROLE
    IF EXISTS "<profile>"。

14. 成功输出
    全部成功后，src/tools/create_profile.rs:63 输出 JSON：state: success、tool: create_profile、profile、database: mem_<profile>、config_path。
