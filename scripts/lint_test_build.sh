#!/bin/sh
export PROFILE=debug
cargo +nightly fmt --all -- --check --profile=$PROFILE
cargo +nightly clippy --profile=$PROFILE --workspace -- -D clippy::all -D warnings --allow clippy::needless_return
cargo nextest run --cargo-profile=$PROFILE --workspace
cargo test --doc --workspace
exit 0
cargo +nightly build --profile=$PROFILE --workspace

