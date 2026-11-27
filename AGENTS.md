# MEM012 Agent 指南

MEM012 为 Agent 提供跨会话的长期记忆。每个 Agent 可以使用独立的 profile 隔离记忆。

## 安装

1. 安装 Rust 和 Cargo，按部署说明准备 PostgreSQL。在仓库根目录首次配置时，用 `cp config.example.toml config.toml` 创建配置文件，再按部署环境填写配置。已有配置可直接编辑。[部署说明](README.md#1-configure-configtoml)
2. 在仓库根目录运行 `sh install.sh --backend` 安装 CLI 和后端。需要同时部署 Web 前端时运行 `sh install.sh`。Linux 使用 systemd 时，可用 `systemctl is-active mem012.service` 检查服务状态。
3. 首次创建 profile 前，在当前 shell 设置 `MEM012_ADMIN_DATABASE_URL`，再执行 `mem012 --create_profile codex`。`codex` 是示例名称，请换成该 Agent 使用的 profile。创建后按部署说明重启服务以加载配置。[Profile 配置](README.md#4-create-a-profile)

## 使用

- 运行 `mem012 --profile codex init` 查看该 profile 配置的初始化记忆。后续命令使用相同的 profile。
- 让 Agent 能访问并加载 `mem012-memory-skill`。其中定义了检索、回读、创建、更新和删除记忆的命令、前置条件与成功判定。[CLI Skill](skills/SKILL.md)
- 可以用以下命令试用检索。替换示例查询和关键词后运行，采用候选前按 skill 回读确认。

  ```bash
  mem012 --profile codex --args '{"tool":"search_memory","params":{"query":"用户对交付格式的偏好","terms":{"include":["交付格式"]},"limit":5}}'
  ```

## 建议添加 Memory 提示词

建议将提示词正文放入 Agent 的全局指令或目标项目的 `AGENTS.md`。它定义何时检索、什么值得保存以及如何汇报结果。[Memory 提示词](skills/mem012_prompt.md)

也可以把同一份提示词设为目标 profile 的 `init` 类别记忆，由 Agent 在新会话或上下文压缩后首次恢复时执行 `mem012 --profile codex init`，读完返回内容再处理任务。此方式需要在 Agent 的持久指令里保留初始化命令和触发条件。[初始化记忆设置](README.md#5-set-up-initial-memory-optional)
