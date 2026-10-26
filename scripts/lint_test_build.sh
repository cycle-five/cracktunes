#!/bin/sh
export PROFILE=debug
cargo +nightly fmt --all -- --check --profile=$PROFILE
cargo clippy --profile=$PROFILE --all --all-targets -- -D warnings
cargo nextest run --cargo-profile=$PROFILE --workspace
cargo test --doc --workspace
exit 0
cargo +nightly build --profile=$PROFILE --workspace

