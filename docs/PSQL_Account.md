# pro2：一账号一库执行清单（精简）

## 0. 目标与边界

- [ ] 一账号一库, riko->mem_riko
- [ ] 共享数据库 mem_share
- [ ] 仅可访问本库 + `mem_share`，其他数据库无 `CONNECT`
- [ ] 运行态不做自动初始化（`init_db()` 为 no-op）

## 1. 账号与数据库映射
- [ ] 仅授予本库/共享库 DDL：`CREATE`
- [ ] 仅授予本库/共享库 DML（`SELECT/INSERT/UPDATE/DELETE`）

## 2. 权限基线（每库）

- [ ] 仅授予本库/共享库：`CONNECT` + schema `USAGE`
- [ ] 仅授予本库/共享库 DML：`SELECT/INSERT/UPDATE/DELETE`
- [ ] 仅授予本库/共享库目标 schema `CREATE`
- [ ] `ALTER/DROP` 仅限对象所有者
- [ ] 角色默认：`NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION`
- [ ] 审计 `pg_auth_members`，确认无高权限继承链

## 最终目标

对应帐号只能操作本库与明确授权的 `mem_share`，无法 `连接/更改/删除` 其他任何数据库
