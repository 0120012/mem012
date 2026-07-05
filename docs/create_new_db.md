# 创建新 profile

新 profile 不再通过手写 PostgreSQL DDL 创建。

使用管理员连接串执行：

```bash
MEM012_ADMIN_DATABASE_URL="postgresql://{admin_user}:{admin_password}@{host}:{port}/postgres" mem012 --create_profile {profile}
```

该命令会一次性完成 role、database、扩展、权限、mem012 表结构初始化，并把 profile 连接串追加到 `config.toml`。
