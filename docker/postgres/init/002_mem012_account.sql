-- What：创建六组业务账号和一个 admin 登录账号。
-- Why：profile 账号只负责各自数据库，admin 账号保留独立维护入口且不单独建库。

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'uutest') THEN
    EXECUTE format(
      'CREATE ROLE %I WITH LOGIN SUPERUSER PASSWORD %L',
      'uutest',
      'D9744sfg20AADA'
    );
  ELSE
    EXECUTE format(
      'ALTER ROLE %I WITH LOGIN SUPERUSER PASSWORD %L',
      'uutest',
      'D9744sfg20AADA'
    );
  END IF;
END $$;

DO $$
DECLARE
  role_name text;
BEGIN
  FOREACH role_name IN ARRAY ARRAY['riko', 'nous', 'claw', 'doge', 'share', 'codex', 'claude'] LOOP
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
      EXECUTE format(
        'CREATE ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %L',
        role_name,
        'zxz456123'
      );
    ELSE
      EXECUTE format(
        'ALTER ROLE %I WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %L',
        role_name,
        'zxz456123'
      );
    END IF;
  END LOOP;
END $$;

SELECT format('REVOKE ALL PRIVILEGES ON DATABASE %I FROM PUBLIC', datname)
FROM pg_database
WHERE datallowconn AND NOT datistemplate\gexec

SELECT format('REVOKE ALL PRIVILEGES ON DATABASE %I FROM riko, nous, claw, doge, "share", codex, claude', datname)
FROM pg_database
WHERE datallowconn AND NOT datistemplate\gexec

SELECT format('GRANT CONNECT ON DATABASE %I TO %I', db_name, role_name)
FROM (VALUES
  ('mem_riko', 'riko'),
  ('mem_nous', 'nous'),
  ('mem_claw', 'claw'),
  ('mem_doge', 'doge'),
  ('mem_share', 'share'),
  ('mem_codex', 'codex'),
  ('mem_claude', 'claude')
) AS wanted(db_name, role_name)\gexec

\connect mem_riko
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
SELECT ag_catalog.create_graph('memory_graph')
WHERE NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph');
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA memory_graph FROM PUBLIC;
REVOKE ALL ON SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex, claude;
GRANT USAGE, CREATE ON SCHEMA public TO riko;
GRANT USAGE ON SCHEMA ag_catalog TO riko;
GRANT USAGE ON TYPE ag_catalog.agtype TO riko;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO riko;
GRANT USAGE, CREATE ON SCHEMA memory_graph TO riko;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO riko;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO riko;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO riko;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO riko;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO riko;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO riko;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO riko;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO riko;

\connect mem_nous
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
SELECT ag_catalog.create_graph('memory_graph')
WHERE NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph');
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA memory_graph FROM PUBLIC;
REVOKE ALL ON SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex, claude;
GRANT USAGE, CREATE ON SCHEMA public TO nous;
GRANT USAGE ON SCHEMA ag_catalog TO nous;
GRANT USAGE ON TYPE ag_catalog.agtype TO nous;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO nous;
GRANT USAGE, CREATE ON SCHEMA memory_graph TO nous;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO nous;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO nous;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO nous;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO nous;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO nous;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO nous;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO nous;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO nous;

\connect mem_claw
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
SELECT ag_catalog.create_graph('memory_graph')
WHERE NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph');
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA memory_graph FROM PUBLIC;
REVOKE ALL ON SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex, claude;
GRANT USAGE, CREATE ON SCHEMA public TO claw;
GRANT USAGE ON SCHEMA ag_catalog TO claw;
GRANT USAGE ON TYPE ag_catalog.agtype TO claw;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO claw;
GRANT USAGE, CREATE ON SCHEMA memory_graph TO claw;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO claw;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO claw;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO claw;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO claw;

\connect mem_doge
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
SELECT ag_catalog.create_graph('memory_graph')
WHERE NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph');
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA memory_graph FROM PUBLIC;
REVOKE ALL ON SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex, claude;
GRANT USAGE, CREATE ON SCHEMA public TO doge;
GRANT USAGE ON SCHEMA ag_catalog TO doge;
GRANT USAGE ON TYPE ag_catalog.agtype TO doge;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO doge;
GRANT USAGE, CREATE ON SCHEMA memory_graph TO doge;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO doge;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO doge;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO doge;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO doge;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO doge;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO doge;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO doge;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO doge;

\connect mem_share
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
SELECT ag_catalog.create_graph('memory_graph')
WHERE NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph');
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA memory_graph FROM PUBLIC;
REVOKE ALL ON SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex, claude;
GRANT USAGE, CREATE ON SCHEMA public TO "share";
GRANT USAGE ON SCHEMA ag_catalog TO "share";
GRANT USAGE ON TYPE ag_catalog.agtype TO "share";
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO "share";
GRANT USAGE, CREATE ON SCHEMA memory_graph TO "share";
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO "share";
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO "share";
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO "share";
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO "share";
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO "share";
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO "share";
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO "share";
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO "share";

\connect mem_codex
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
SELECT ag_catalog.create_graph('memory_graph')
WHERE NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph');
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA memory_graph FROM PUBLIC;
REVOKE ALL ON SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex, claude;
GRANT USAGE, CREATE ON SCHEMA public TO codex;
GRANT USAGE ON SCHEMA ag_catalog TO codex;
GRANT USAGE ON TYPE ag_catalog.agtype TO codex;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO codex;
GRANT USAGE, CREATE ON SCHEMA memory_graph TO codex;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO codex;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO codex;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO codex;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO codex;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO codex;

\connect mem_claude
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
SELECT ag_catalog.create_graph('memory_graph')
WHERE NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'memory_graph');
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL ON SCHEMA memory_graph FROM PUBLIC;
REVOKE ALL ON SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA memory_graph FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, nous, claw, doge, "share", codex, claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, nous, claw, doge, "share", codex, claude;
GRANT USAGE, CREATE ON SCHEMA public TO claude;
GRANT USAGE ON SCHEMA ag_catalog TO claude;
GRANT USAGE ON TYPE ag_catalog.agtype TO claude;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO claude;
GRANT USAGE, CREATE ON SCHEMA memory_graph TO claude;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO claude;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA memory_graph TO claude;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO claude;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA memory_graph TO claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO claude;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA memory_graph GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO claude;
