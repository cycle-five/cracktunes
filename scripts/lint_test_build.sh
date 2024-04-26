#!/bin/sh
cargo +nightly fmt --all -- --check
cargo +nightly clippy --all -- -D clippy::all -D warnings
RES1=$?
cargo tarpaulin --verbose --workspace --timeout 120 --out xml
RES2=$?

if [ ${RES1} = 0 ] && [ ${RES2} = 0 ]; then
	echo "Building..."
else
	echo "Something broke, still building..."
fi
cargo build --profile=release-with-debug
