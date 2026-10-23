#!/usr/bin/env bash
# A safe way to run cargo-mutants on this machine.
#
#   scripts/mutants.sh [cargo-mutants args...]
#
# 🔑 It never mutates the working tree in place: cargo-mutants copies the tree
# to a scratch directory and mutates the copy. `--in-place` is refused,
# because another session may be editing the same checkout.
# 🔑 The scratch copies live under $HOME/.cache/cargo-mutants, not /tmp, which
# is a small tmpfs in RAM. cargo-mutants puts them in $TMPDIR.
# 🪤 CARGO_TARGET_DIR is relative on purpose: each scratch copy then builds
# into its own `target/`, so parallel jobs never share (or rebuild over) one.
# The price is a cold build per job, and disk: scripts/sweep_target.py clears
# leftovers from killed runs first.
# 🔑 CI only: CRACK_MUTANTS_SHARED_TARGET=1 keeps a preset ABSOLUTE
# CARGO_TARGET_DIR instead. A CI shard runs one job at `-j 1`, so per-copy
# targets buy nothing there, while one shared directory lets rust-cache restore
# the dependencies' build and lets the base run reuse the head run's. Measured
# 2026-10-23: see mutants-diff.yml.
set -euo pipefail

for arg in "$@"; do
    if [[ "$arg" == "--in-place" ]]; then
        echo "mutants.sh: --in-place is refused: it would mutate this working tree." >&2
        exit 2
    fi
done

cd "$(dirname "${BASH_SOURCE[0]}")/.."

scratch="${CRACK_MUTANTS_SCRATCH:-$HOME/.cache/cargo-mutants}"
mkdir -p "$scratch"
# scripts/mutants_check.py sweeps once itself and sets CRACK_MUTANTS_NO_SWEEP
# for the several calls it makes; used directly, this sweeps every time.
if [[ -z "${CRACK_MUTANTS_NO_SWEEP:-}" ]]; then
    python3 scripts/sweep_target.py --scratch-root "$scratch" >&2
fi

export TMPDIR="$scratch"
if [[ "${CRACK_MUTANTS_SHARED_TARGET:-}" == 1 ]]; then
    if [[ "${CARGO_TARGET_DIR:-}" != /* ]]; then
        echo "mutants.sh: CRACK_MUTANTS_SHARED_TARGET=1 needs an absolute CARGO_TARGET_DIR," \
            "got '${CARGO_TARGET_DIR:-}'" >&2
        exit 2
    fi
else
    export CARGO_TARGET_DIR=target
fi
export NEXTEST_PROFILE=mutants
# sqlx's query macros check every query against a live database whenever
# DATABASE_URL is set (in the environment or a .env), and the result then
# depends on that database's schema. Offline, they use the committed `.sqlx/`
# metadata, as CI does.
export SQLX_OFFLINE=true
# Tells the database test runner (crack-core/src/test_db.rs) to drop a failing
# test's database. Mutant runs fail tests on purpose, and each scratch copy has
# its own database names, so kept databases would pile up.
export CRACK_MUTANTS=1

exec cargo mutants "$@"
