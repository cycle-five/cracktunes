Grafana Postgres Monitoring
===========================

1) Allow access to which thing?

```bash
lothrop@kalevala:~$ sudo ufw allow 443
Rules updated
Rules updated (v6)
lothrop@kalevala:~$ sudo ufw allow 22
Rules updated
Rules updated (v6)
lothrop@kalevala:~$ sudo ufw allow 6001
Rules updated
Rules updated (v6)
lothrop@kalevala:~$ sudo ufw allow 8000
Rules updated
Rules updated (v6)
lothrop@kalevala:~$ sudo ufw allow 80
lothrop@kalevala:~$ psql postgresql://postgres:mysecretpassword@localhost:5432/postgres
```

2) Create a user for Grafana

```SQL
CREATE USER grafana WITH PASSWORD 'asdfqwer' CREATEDB;

GRANT SELECT ON ALL TABLES IN SCHEMA public TO grafana;
```

3) Run the Postgres Exporter

```bash
docker run --name pdc-agent grafana/pdc-agent:latest -token [TOKEN] -cluster prod-us-east-0 -gcloud-hosted-grafana-id 691720
```