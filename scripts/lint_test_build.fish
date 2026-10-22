#!/bin/fish
set -x PROFILE release
cargo +nightly fmt --all -- --check --profile=$PROFILE
cargo +nightly clippy --profile=$PROFILE --all -- -D clippy::all -D warnings --allow clippy::needless_return
cargo nextest run --cargo-profile=$PROFILE --workspace
cargo +nightly build --profile=$PROFILE --workspace
