# mem012

## 构建 PostgreSQL 镜像

```bash
docker build -t mem012-postgres:pg18 -f docker/postgres/Dockerfile docker/postgres
```

## 持久化启动

```bash
export MEM012_UUTEST_PASSWORD='你的密码'

docker run -d \
  --name mem012-postgres \
  -p 5632:5432 \
  -e POSTGRES_PASSWORD="$MEM012_UUTEST_PASSWORD" \
  -v mem012_pg18_data:/var/lib/postgresql \
  mem012-postgres:pg18
```

首次初始化会自动创建 `TESTD / mem_riko / mem_herm / mem_doge / mem_claw / mem_hakimi / mem_share`，并启用 `vector / pg_trgm / age`。

## 验证：

```bash
docker exec mem012-postgres psql -U uutest -d TESTD -c "select name, installed_version from pg_available_extensions where name in ('vector', 'pg_trgm', 'age');"
```

`installed_version` 不为空，才表示当前数据库已启用该扩展。
