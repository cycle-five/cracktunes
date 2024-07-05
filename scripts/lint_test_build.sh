#!/bin/sh
export PROFILE=release
cargo +nightly fmt --all -- --check --profile=$PROFILE
cargo +nightly clippy --profile=$PROFILE --workspace -- -D clippy::all -D warnings
cargo +nightly test --profile=$PROFILE --workspace
cargo +nightly tarpaulin --profile=$PROFILE --verbose --workspace --timeout 120 --out xml
cargo +nightly build --profile=$PROFILE --workspace

