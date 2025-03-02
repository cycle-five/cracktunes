#!/bin/fish
set -x PROFILE release-with-debug
cargo +nightly fmt --all -- --check --profile=$PROFILE
cargo +nightly clippy --profile=$PROFILE --all -- -D clippy::all -D warnings --allow clippy::needless_return --allow clippy::cast_sign_loss
cargo +nightly test --profile=$PROFILE
cargo +nightly tarpaulin --profile=$PROFILE --verbose --workspace --timeout 120 --out xml
cargo +nightly build --profile=$PROFILE --workspace
