#!/bin/bash
cargo clean
sqlx database drop --database-url sqlite://./data/crackedmusic.db
sqlx database create --database-url sqlite://./data/crackedmusic.db
sqlx migrate run --source migrations/  --database-url sqlite://./data/crackedmusic.db
cargo sqlx prepare --workspace  --database-url sqlite://./data/crackedmusic.db
