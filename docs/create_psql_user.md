# 创建 PostgreSQL 业务账号

## 1. 目标

按 `docs/PSQL_Account.md` 的边界创建六组账号，并让每个账号只能访问自己的数据库。

| 账号 | 数据库 |
| --- | --- |
| `riko` | `mem_riko` |
| `nous` | `mem_nous` |
| `claw` | `mem_claw` |
| `doge` | `mem_doge` |
| `share` | `mem_share` |
| `codex` | `mem_codex` |

业务账号密码从环境变量传入，避免把真实密码写进 Git 历史：

```bash
export MEM012_PSQL_USER_PASSWORD='你的业务账号密码'
```

每个数据库都启用扩展：`vector`、`pg_trgm`、`age`。

## 2. 前置条件

确认 PostgreSQL 容器正在运行：

```bash
docker ps --format '{{.Names}}' | grep '^mem012-postgres$'
```

以下命令默认使用管理账号 `uutest`，管理库为 `postgres`。

## 3. 创建或更新账号

```bash
docker exec -i mem012-postgres psql \
  -v ON_ERROR_STOP=1 \
  -v user_password="$MEM012_PSQL_USER_PASSWORD" \
  -U uutest \
  -d postgres <<'SQL'
SELECT format(
  'CREATE ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION',
  role_name
)
FROM (VALUES
  ('riko'),
  ('nous'),
  ('claw'),
  ('doge'),
  ('share'),
  ('codex')
) AS wanted(role_name)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = wanted.role_name)\gexec

ALTER ROLE riko WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD :'user_password';
ALTER ROLE nous WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD :'user_password';
ALTER ROLE claw WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD :'user_password';
ALTER ROLE doge WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD :'user_password';
ALTER ROLE "share" WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD :'user_password';
ALTER ROLE codex WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD :'user_password';
SQL
```

## 4. 创建缺失数据库

```bash
docker exec -i mem012-postgres psql -v ON_ERROR_STOP=1 -U uutest -d postgres <<'SQL'
SELECT format('CREATE DATABASE %I OWNER uutest', db_name)
FROM (VALUES
  ('mem_riko'),
  ('mem_nous'),
  ('mem_claw'),
  ('mem_doge'),
  ('mem_share'),
  ('mem_codex')
) AS wanted(db_name)
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = wanted.db_name)\gexec

ALTER DATABASE mem_riko OWNER TO uutest;
ALTER DATABASE mem_nous OWNER TO uutest;
ALTER DATABASE mem_claw OWNER TO uutest;
ALTER DATABASE mem_doge OWNER TO uutest;
ALTER DATABASE mem_share OWNER TO uutest;
ALTER DATABASE mem_codex OWNER TO uutest;
SQL
```

## 5. 收紧数据库连接权限

```bash
docker exec -i mem012-postgres psql -v ON_ERROR_STOP=1 -U uutest -d postgres <<'SQL'
SELECT format('REVOKE ALL PRIVILEGES ON DATABASE %I FROM PUBLIC', datname)
FROM pg_database
WHERE datallowconn AND NOT datistemplate\gexec

SELECT format('REVOKE ALL PRIVILEGES ON DATABASE %I FROM riko, nous, claw, doge, "share", codex', datname)
FROM pg_database
WHERE datallowconn AND NOT datistemplate\gexec

GRANT CONNECT ON DATABASE mem_riko TO riko;
GRANT CONNECT ON DATABASE mem_nous TO nous;
GRANT CONNECT ON DATABASE mem_claw TO claw;
GRANT CONNECT ON DATABASE mem_doge TO doge;
GRANT CONNECT ON DATABASE mem_share TO "share";
GRANT CONNECT ON DATABASE mem_codex TO codex;
SQL
```

## 6. 配置私有库权限

```bash
docker exec -i mem012-postgres psql -v ON_ERROR_STOP=1 -U uutest -d postgres <<'SQL'
\connect mem_riko
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex;
GRANT USAGE, CREATE ON SCHEMA public TO riko;
GRANT USAGE ON SCHEMA ag_catalog TO riko;
GRANT USAGE ON TYPE ag_catalog.agtype TO riko;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO riko;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO riko;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO riko;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO riko;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO riko;

\connect mem_nous
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex;
GRANT USAGE, CREATE ON SCHEMA public TO nous;
GRANT USAGE ON SCHEMA ag_catalog TO nous;
GRANT USAGE ON TYPE ag_catalog.agtype TO nous;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO nous;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO nous;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO nous;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO nous;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO nous;

\connect mem_claw
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex;
GRANT USAGE, CREATE ON SCHEMA public TO claw;
GRANT USAGE ON SCHEMA ag_catalog TO claw;
GRANT USAGE ON TYPE ag_catalog.agtype TO claw;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO claw;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO claw;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO claw;

\connect mem_doge
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex;
GRANT USAGE, CREATE ON SCHEMA public TO doge;
GRANT USAGE ON SCHEMA ag_catalog TO doge;
GRANT USAGE ON TYPE ag_catalog.agtype TO doge;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO doge;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO doge;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO doge;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO doge;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO doge;

\connect mem_codex
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex;
GRANT USAGE, CREATE ON SCHEMA public TO codex;
GRANT USAGE ON SCHEMA ag_catalog TO codex;
GRANT USAGE ON TYPE ag_catalog.agtype TO codex;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO codex;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO codex;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO codex;
SQL
```

## 7. 配置 `mem_share` 权限

```bash
docker exec -i mem012-postgres psql -v ON_ERROR_STOP=1 -U uutest -d mem_share <<'SQL'
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;

ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex;

GRANT USAGE, CREATE ON SCHEMA public TO "share";
GRANT USAGE ON SCHEMA ag_catalog TO "share";
GRANT USAGE ON TYPE ag_catalog.agtype TO "share";
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO "share";
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO "share";
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO "share";
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO "share";
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO "share";
SQL
```

## 8. 验证角色权限

```bash
docker exec mem012-postgres psql -U uutest -d postgres -Atc \
  "select rolname, rolsuper, rolcreatedb, rolcreaterole, rolreplication from pg_roles where rolname in ('riko','nous','claw','doge','share','codex') order by rolname;"

docker exec mem012-postgres psql -U uutest -d postgres -Atc \
  "select member::regrole || '->' || roleid::regrole from pg_auth_members where member in ('riko'::regrole,'nous'::regrole,'claw'::regrole,'doge'::regrole,'share'::regrole,'codex'::regrole);"
```

第二条命令应无输出。

## 9. 验证允许连接

```bash
docker exec -e PGPASSWORD="$MEM012_PSQL_USER_PASSWORD" mem012-postgres psql -h 127.0.0.1 -U riko -d mem_riko -Atc "select current_user, current_database();"
docker exec -e PGPASSWORD="$MEM012_PSQL_USER_PASSWORD" mem012-postgres psql -h 127.0.0.1 -U nous -d mem_nous -Atc "select current_user, current_database();"
docker exec -e PGPASSWORD="$MEM012_PSQL_USER_PASSWORD" mem012-postgres psql -h 127.0.0.1 -U claw -d mem_claw -Atc "select current_user, current_database();"
docker exec -e PGPASSWORD="$MEM012_PSQL_USER_PASSWORD" mem012-postgres psql -h 127.0.0.1 -U doge -d mem_doge -Atc "select current_user, current_database();"
docker exec -e PGPASSWORD="$MEM012_PSQL_USER_PASSWORD" mem012-postgres psql -h 127.0.0.1 -U share -d mem_share -Atc "select current_user, current_database();"
docker exec -e PGPASSWORD="$MEM012_PSQL_USER_PASSWORD" mem012-postgres psql -h 127.0.0.1 -U codex -d mem_codex -Atc "select current_user, current_database();"
```

## 10. 验证禁止跨库连接

```bash
docker exec -e PGPASSWORD="$MEM012_PSQL_USER_PASSWORD" mem012-postgres sh -lc '
for spec in \
  riko:mem_nous riko:mem_claw riko:mem_doge riko:mem_share riko:mem_codex riko:postgres \
  nous:mem_riko nous:mem_claw nous:mem_doge nous:mem_share nous:mem_codex nous:postgres \
  claw:mem_riko claw:mem_nous claw:mem_doge claw:mem_share claw:mem_codex claw:postgres \
  doge:mem_riko doge:mem_nous doge:mem_claw doge:mem_share doge:mem_codex doge:postgres \
  share:mem_riko share:mem_nous share:mem_claw share:mem_doge share:mem_codex share:postgres \
  codex:mem_riko codex:mem_nous codex:mem_claw codex:mem_doge codex:mem_share codex:postgres
do
  user=${spec%%:*}
  db=${spec#*:}
  if psql -h 127.0.0.1 -U "$user" -d "$db" -Atc "select 1" >/dev/null 2>&1; then
    echo "$spec:ALLOWED"
  else
    echo "$spec:DENIED"
  fi
done'
```

所有输出都应为 `DENIED`。

## 11. 验证扩展

```bash
docker exec mem012-postgres sh -lc '
for db in mem_riko mem_nous mem_claw mem_doge mem_share mem_codex; do
  psql -U uutest -d "$db" -Atc "select current_database() || chr(58) || string_agg(extname, chr(44) order by extname) from pg_extension where extname in (chr(97)||chr(103)||chr(101), chr(112)||chr(103)||chr(95)||chr(116)||chr(114)||chr(103)||chr(109), chr(118)||chr(101)||chr(99)||chr(116)||chr(111)||chr(114));"
done'
```

每个库都应输出：`age,pg_trgm,vector`。

## 12. 验证 DDL/DML 边界

```bash
docker exec -e PGPASSWORD="$MEM012_PSQL_USER_PASSWORD" mem012-postgres sh -lc '
set -eu
for spec in riko:mem_riko nous:mem_nous claw:mem_claw doge:mem_doge share:mem_share codex:mem_codex; do
  user=${spec%%:*}
  db=${spec#*:}
  table=__private_probe_$user
  psql -h 127.0.0.1 -U "$user" -d "$db" -v ON_ERROR_STOP=1 -Atc "drop table if exists public.$table; create table public.$table(id int); insert into public.$table values (1); select current_user || chr(58) || current_database() || chr(58) || count(*) from public.$table group by current_user, current_database(); drop table public.$table;"
done
'
```
