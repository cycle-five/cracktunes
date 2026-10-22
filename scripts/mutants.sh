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
python3 scripts/sweep_target.py --scratch-root "$scratch" >&2

export TMPDIR="$scratch"
export CARGO_TARGET_DIR=target
export NEXTEST_PROFILE=mutants
# Tells the database test runner (crack-core/src/test_db.rs) to drop a failing
# test's database. Mutant runs fail tests on purpose, and each scratch copy has
# its own database names, so kept databases would pile up.
export CRACK_MUTANTS=1

exec cargo mutants "$@"
