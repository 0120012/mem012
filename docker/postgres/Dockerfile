# Why：把 PostgreSQL 扩展固定在项目镜像里，避免运行容器缺 pgvector / AGE 时临时手工修。
ARG PG_MAJOR=18
ARG PGVECTOR_REF=v0.8.2
ARG AGE_REF=PG18/v1.7.0-rc0

FROM postgres:${PG_MAJOR}-bookworm AS builder

ARG PG_MAJOR
ARG PGVECTOR_REF
ARG AGE_REF

RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        bison \
        build-essential \
        ca-certificates \
        flex \
        git \
        postgresql-server-dev-${PG_MAJOR}; \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build

RUN set -eux; \
    git clone --depth 1 --branch "${PGVECTOR_REF}" https://github.com/pgvector/pgvector.git; \
    cd pgvector; \
    make; \
    make install

RUN set -eux; \
    git clone --depth 1 --branch "${AGE_REF}" https://github.com/apache/age.git; \
    cd age; \
    make PG_CONFIG="/usr/lib/postgresql/${PG_MAJOR}/bin/pg_config"; \
    make PG_CONFIG="/usr/lib/postgresql/${PG_MAJOR}/bin/pg_config" install

FROM postgres:${PG_MAJOR}-bookworm

ARG PG_MAJOR

COPY --from=builder /usr/lib/postgresql/${PG_MAJOR}/lib/vector.so /usr/lib/postgresql/${PG_MAJOR}/lib/
COPY --from=builder /usr/share/postgresql/${PG_MAJOR}/extension/vector* /usr/share/postgresql/${PG_MAJOR}/extension/
COPY --from=builder /usr/lib/postgresql/${PG_MAJOR}/lib/age.so /usr/lib/postgresql/${PG_MAJOR}/lib/
COPY --from=builder /usr/share/postgresql/${PG_MAJOR}/extension/age* /usr/share/postgresql/${PG_MAJOR}/extension/
RUN echo "shared_preload_libraries = 'age'" >> /usr/share/postgresql/${PG_MAJOR}/postgresql.conf.sample
