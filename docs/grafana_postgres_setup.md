Grafana Postgres Monitoring
===========================

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

```SQL
CREATE USER grafana WITH PASSWORD 'asdfqwer' CREATEDB;

GRANT SELECT ON ALL TABLES IN SCHEMA public TO grafana;
```

```bash
docker run --name pdc-agent grafana/pdc-agent:latest -token <TOKEB>  -cluster prod-us-east-0 -gcloud-hosted-grafana-id 691720
```