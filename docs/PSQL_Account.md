# pro2：一账号一库执行清单（精简）

## 0. 目标与边界

- [ ] 一账号一库, riko->mem_riko, herm->mem_herm, doge->mem_doge, share->mem_share
- [ ] `mem_share` 是共享记忆库，但使用独立 `share` 账号访问
- [ ] 每个账号仅可访问本库，其他数据库无 `CONNECT`
- [ ] 运行态不做自动初始化（`init_db()` 为 no-op）

## 1. 账号与数据库映射
- [ ] 仅授予本库 DDL：`CREATE`
- [ ] 仅授予本库 DML（`SELECT/INSERT/UPDATE/DELETE`）

## 2. 权限基线（每库）

- [ ] 仅授予本库：`CONNECT` + schema `USAGE`
- [ ] 仅授予本库 DML：`SELECT/INSERT/UPDATE/DELETE`
- [ ] 仅授予本库目标 schema `CREATE`
- [ ] 每库必须启用 `age` extension
- [ ] 每库业务账号必须可使用 `ag_catalog`、执行 AGE 函数、使用 `agtype`
- [ ] 每库新连接必须能加载 AGE，否则 `rebuild_graph` 不可用
- [ ] `ALTER/DROP` 仅限对象所有者
- [ ] 角色默认：`NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION`
- [ ] 审计 `pg_auth_members`，确认无高权限继承链

## 最终目标

对应帐号只能操作自己的数据库，无法 `连接/更改/删除` 其他任何数据库；应用层需要访问共享记忆时，使用 `share` 账号连接 `mem_share`
