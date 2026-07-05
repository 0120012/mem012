# 创建 PostgreSQL profile 账号

手工创建 PostgreSQL 业务账号的流程已经退场。

使用：

```bash
MEM012_ADMIN_DATABASE_URL="postgresql://{admin_user}:{admin_password}@{host}:{port}/postgres" mem012 --create_profile {profile}
```

`--create_profile` 会创建 profile role、`mem_<profile>` database、扩展和权限，并初始化 mem012 schema。
