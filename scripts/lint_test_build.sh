#!/bin/sh
export PROFILE=debug
cargo +nightly fmt --all -- --check --profile=$PROFILE
cargo +nightly clippy --profile=$PROFILE --workspace -- -D clippy::all -D warnings --allow clippy::needless_return
cargo +nightly test --profile=$PROFILE --workspace
exit 0
cargo +nightly tarpaulin --profile=$PROFILE --verbose --workspace --timeout 120 --out xml
cargo +nightly build --profile=$PROFILE --workspace

