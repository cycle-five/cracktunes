#!/bin/fish
set -x PROFILE release
cargo +nightly fmt --all -- --check --profile=$PROFILE
cargo clippy --profile=$PROFILE --all --all-targets -- -D warnings
cargo nextest run --cargo-profile=$PROFILE --workspace
cargo test --doc --workspace
cargo +nightly build --profile=$PROFILE --workspace
