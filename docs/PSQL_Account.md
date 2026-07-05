# PostgreSQL profile 边界

当前主路径是 `mem012 --create_profile <profile>`。

约束：

- role 名为 `<profile>`。
- database 名为 `mem_<profile>`。
- profile 只能连接自己的 database。
- Docker 镜像只提供 PostgreSQL 与 `vector / pg_trgm / age` 扩展能力，不预置业务账号或 profile database。

手工账号清单已经退场；不要再维护固定 profile 列表。
