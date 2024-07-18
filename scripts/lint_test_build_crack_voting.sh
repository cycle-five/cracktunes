#!/bin/sh
cargo clippy -p crack-voting --release -- -D clippy::all -D warnings
cargo test -p crack-voting --release
cargo build -p crack-voting --release