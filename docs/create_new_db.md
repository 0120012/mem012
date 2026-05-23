# 创建新数据库

用 `uutest` 管理账号创建新 profile 数据库。

## 1. 设置变量

```bash
export MEM012_NEW_USER='doge'
export MEM012_NEW_DB='mem_doge'
export MEM012_NEW_PASSWORD='你的密码'
```

## 2. 创建用户

```bash
docker exec -it mem012-postgres psql -U uutest -d TESTD \
  -c "CREATE USER ${MEM012_NEW_USER} WITH PASSWORD '${MEM012_NEW_PASSWORD}';"
```

## 3. 创建数据库

```bash
docker exec -it mem012-postgres psql -U uutest -d TESTD \
  -c "CREATE DATABASE ${MEM012_NEW_DB} OWNER ${MEM012_NEW_USER};"
```

## 4. 启用扩展

```bash
docker exec -it mem012-postgres psql -U uutest -d "$MEM012_NEW_DB" \
  -c "CREATE EXTENSION IF NOT EXISTS vector;"

docker exec -it mem012-postgres psql -U uutest -d "$MEM012_NEW_DB" \
  -c "CREATE EXTENSION IF NOT EXISTS pg_trgm;"

docker exec -it mem012-postgres psql -U uutest -d "$MEM012_NEW_DB" \
  -c "CREATE EXTENSION IF NOT EXISTS age;"
```

## 5. 验证扩展

```bash
docker exec -it mem012-postgres psql -U uutest -d "$MEM012_NEW_DB" \
  -c "select name, installed_version from pg_available_extensions where name in ('vector', 'pg_trgm', 'age');"
```

`installed_version` 不为空表示当前数据库已启用该扩展。
