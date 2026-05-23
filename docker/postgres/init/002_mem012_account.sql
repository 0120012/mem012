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
  FOREACH role_name IN ARRAY ARRAY['riko', 'herm', 'doge', 'share', 'hakimi', 'claw'] LOOP
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

SELECT format('REVOKE ALL PRIVILEGES ON DATABASE %I FROM riko, herm, doge, "share", hakimi, claw', datname)
FROM pg_database
WHERE datallowconn AND NOT datistemplate\gexec

SELECT format('GRANT CONNECT ON DATABASE %I TO %I', db_name, role_name)
FROM (VALUES
  ('mem_riko', 'riko'),
  ('mem_herm', 'herm'),
  ('mem_doge', 'doge'),
  ('mem_share', 'share'),
  ('mem_hakimi', 'hakimi'),
  ('mem_claw', 'claw')
) AS wanted(db_name, role_name)\gexec

\connect mem_riko
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, herm, doge, "share", hakimi, claw;
GRANT USAGE, CREATE ON SCHEMA public TO riko;
GRANT USAGE ON SCHEMA ag_catalog TO riko;
GRANT USAGE ON TYPE ag_catalog.agtype TO riko;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO riko;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO riko;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO riko;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO riko;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO riko;

\connect mem_herm
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, herm, doge, "share", hakimi, claw;
GRANT USAGE, CREATE ON SCHEMA public TO herm;
GRANT USAGE ON SCHEMA ag_catalog TO herm;
GRANT USAGE ON TYPE ag_catalog.agtype TO herm;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO herm;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO herm;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO herm;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO herm;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO herm;

\connect mem_doge
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, herm, doge, "share", hakimi, claw;
GRANT USAGE, CREATE ON SCHEMA public TO doge;
GRANT USAGE ON SCHEMA ag_catalog TO doge;
GRANT USAGE ON TYPE ag_catalog.agtype TO doge;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO doge;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO doge;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO doge;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO doge;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO doge;

\connect mem_share
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, herm, doge, "share", hakimi, claw;
GRANT USAGE, CREATE ON SCHEMA public TO "share";
GRANT USAGE ON SCHEMA ag_catalog TO "share";
GRANT USAGE ON TYPE ag_catalog.agtype TO "share";
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO "share";
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO "share";
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO "share";
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO "share";
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO "share";

\connect mem_hakimi
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, herm, doge, "share", hakimi, claw;
GRANT USAGE, CREATE ON SCHEMA public TO hakimi;
GRANT USAGE ON SCHEMA ag_catalog TO hakimi;
GRANT USAGE ON TYPE ag_catalog.agtype TO hakimi;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO hakimi;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO hakimi;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO hakimi;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO hakimi;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO hakimi;

\connect mem_claw
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS age;
ALTER SCHEMA public OWNER TO uutest;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
REVOKE ALL ON SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL ON SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL ON SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public FROM riko, herm, doge, "share", hakimi, claw;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM PUBLIC;
REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA ag_catalog FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON TABLES FROM riko, herm, doge, "share", hakimi, claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public REVOKE ALL ON SEQUENCES FROM riko, herm, doge, "share", hakimi, claw;
GRANT USAGE, CREATE ON SCHEMA public TO claw;
GRANT USAGE ON SCHEMA ag_catalog TO claw;
GRANT USAGE ON TYPE ag_catalog.agtype TO claw;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA ag_catalog TO claw;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO claw;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO claw;
ALTER DEFAULT PRIVILEGES FOR ROLE uutest IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO claw;
