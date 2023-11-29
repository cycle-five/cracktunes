#!/bin/bash
export DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5433/cracked_music
# cargo clean
sqlx database drop
sqlx database create
sqlx migrate run --source migrations/
cargo sqlx prepare --workspace
