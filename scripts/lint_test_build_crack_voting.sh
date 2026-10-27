#!/bin/sh
cargo clippy -p crack-voting --release --all-targets -- -D warnings
cargo test -p crack-voting --release
cargo build -p crack-voting --release