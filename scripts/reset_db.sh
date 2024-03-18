#!/bin/sh
set -ex

if [ "${1}" != "replit" ]; then
	export DATABASE_URL=postgresql://postgres:asdf@localhost:5433/postgres
	# export DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5433/postgres
fi
# cargo clean
sqlx database drop
sqlx database create
sqlx migrate run --source migrations/

if [ "${1}" = "replit" ]; then
	cargo sqlx prepare --merged
else
	cargo sqlx prepare --workspace
fi
