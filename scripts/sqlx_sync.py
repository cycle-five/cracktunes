#!/usr/bin/env python3
"""Sync .sqlx from an online build's SQLX_OFFLINE_DIR, in the repo's format.

`cargo sqlx prepare` from sqlx-cli 0.8 rewrites every file for this sqlx 0.9
crate; this regenerates through the macros themselves instead. With a
migrated database at DATABASE_URL:

    out=$(mktemp -d)
    SQLX_OFFLINE=false SQLX_OFFLINE_DIR="$out" \
        cargo build -p crack-core --all-targets --features db-tests
    python3 scripts/sqlx_sync.py "$out"

- Writes every generated query file that is new or differs (origin keys
  ignored when comparing), with
  sqlx 0.9's `origin` keys stripped (the repo's files never carry them).
- Removes .sqlx files whose query text no longer appears in any tracked .rs file.
"""
import glob, json, os, subprocess, sys

out = sys.argv[1]

def strip(d):
    d.pop("origin", None)
    for c in d.get("describe", {}).get("columns", []):
        c.pop("origin", None)
    return d

def dump(d):
    return json.dumps(d, indent=2, ensure_ascii=False) + "\n"

written = []
for f in sorted(glob.glob(os.path.join(out, "query-*.json"))):
    gen = strip(json.load(open(f)))
    target = os.path.join(".sqlx", os.path.basename(f))
    # Compare with origin stripped on both sides: a file that differs only in
    # carrying origin keys is left alone (one from v0.24.0 does).
    if not os.path.exists(target) or strip(json.load(open(target))) != gen:
        open(target, "w").write(dump(gen))
        written.append(target)
files = subprocess.check_output(["git", "ls-files", "*.rs"], text=True).split()
src = "".join(open(p, encoding="utf-8", errors="ignore").read() for p in files)
stale = [f for f in sorted(glob.glob(".sqlx/query-*.json")) if json.load(open(f))["query"] not in src]
for f in stale:
    os.remove(f)
print("written:", written)
print("removed stale:", stale)
