# MEM012
mem012 是一个所有Agent的CLI 记忆系统，提供持久化记忆与 RAG 检索能力，并支持通过 Web 端管理记忆。

<img src="frontend/public/mem012-architecture.png" alt="MEM012 架构图" width="600">

## 让每个 Agent 都拥有长期记忆

- **全 Agent 兼容**：支持上述 Agent 及其他能调用 CLI / HTTP/JSON 工具的运行环境，无需专用 SDK。
- **持久化记忆 + RAG 检索**：记忆存储在 PostgreSQL，支持关键词、向量与重排检索。
- **独立记忆**：不同 Agent 可以拥有独立记忆空间，避免上下文互相污染。
- **可视化记忆审查**：Agent 负责调用，Web 界面负责审查：可集中查看记忆与变更记录，并进行审核、批准、拒绝、恢复和维护。

## 1. 配置config.toml：

```bash
cp config.example.toml config.toml
```

## 2. PostgreSQL

```bash
docker build -t mem012-postgres:pg18 -f docker/postgres/Dockerfile docker/postgres
```

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

## 3. 编译并安装

统一通过顶层 `install.sh` 安装。前端默认发布到 `/opt/1panel/www/sites/mem012/index`；在使用 systemd 的 Linux 上，安装后端时会自动安装并启动 `mem012.service`。

```bash
# 同时安装前端和后端
sh install.sh

# 只安装前端到默认目录
sh install.sh --frontend

# 只安装前端到指定目录（必须使用绝对路径）
sh install.sh --frontend /opt/1panel/www/sites/custom-site

# 只安装后端
sh install.sh --backend
```

## 4. 创建 profile

每个agent可以独享一个profile，实现记忆隔离。

例如创建一个profile 给codex使用，同时会创建mem_codex库

```bash
export MEM012_ADMIN_DATABASE_URL="postgresql://${MEM012_ADMIN_POSTGRES_USER}:${MEM012_POSTGRES_PASSWORD}@127.0.0.1:5632/postgres"
mem012 --create_profile codex
```

创建 profile 会更新 `config.toml`。重启服务以加载新配置，并确认服务仍在运行：

```bash
sudo systemctl restart mem012.service
sudo systemctl is-active --quiet mem012.service
```

## 5. 设置初始化记忆 (可选)

1. 执行 `sudo systemctl is-active --quiet mem012.service` 确认服务正在运行，再打开 `http://127.0.0.1:37777/auth` 获取 5 分钟有效的 `auth_token`。
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

## 8. 记忆导出/导入

记忆导出:

```shell
mem012 --profile maccodex --args '{"tool":"backup_memory","params":{"output_path":"/绝对路径/backup.json"}}'
```

记忆导入：
```shell
mem012 --profile 目标profile --args '{"tool":"import_memory","params":{"input_path":"/绝对路径/backup.json"}}'
```
