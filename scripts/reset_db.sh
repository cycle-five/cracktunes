#!/bin/bash
cargo clean
sqlx database drop
sqlx database create
sqlx migrate run --source migrations/
cargo sqlx prepare
