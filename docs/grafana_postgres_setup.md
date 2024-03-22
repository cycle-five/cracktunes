Grafana Postgres Monitoring
===========================

```bash
psql postgresql://postgres:mysecretpassword@localhost:5432/postgres
```

```SQL
CREATE USER grafana WITH PASSWORD 'asdfqwer' CREATEDB;

GRANT SELECT ON ALL TABLES IN SCHEMA public TO grafana;
```