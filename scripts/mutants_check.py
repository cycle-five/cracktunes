#!/usr/bin/env python3
"""Core of the cargo-mutants gate: which misses are new, and how to report them.

The gate runs cargo-mutants on the code a PR touched and fails only on NEW
misses (spec section 4, "New or old"). A miss is old if the base commit has
the same mutant missed too, so a PR is not blamed for what it did not break.

🔑 Misses are compared by key, WITHOUT line and column. An unrelated edit above
a mutant shifts its line number, and a line-keyed comparison would call a known
miss new. The key is the file plus the description, so `file.rs:48:5: replace x
with y` becomes `file.rs: replace x with y`.

🪤 The same description can occur several times in one file (three
`replace && with ||` in one function), so copies are counted: a base with one
copy and a head with two is one new miss.

Pure functions only; the git and cargo-mutants pipeline lives around them.
Stdlib only.
"""
import collections
import re

LINE_RE = re.compile(r"^(.+?):\d+:\d+: (.+)$")
# The characters regex::escape escapes in Rust.
RUST_SPECIAL = set("\\.+*?()|[]{}^$#&-~")


def key(name: str) -> str:
    m = LINE_RE.match(name.rstrip("\n"))
    if not m:
        raise ValueError(f"not a cargo-mutants line: {name!r}")
    return f"{m.group(1)}: {m.group(2)}"


def split_new_old(head_missed: list[str], base_missed: list[str]) -> tuple[list[str], list[str]]:
    """Return (new, old) head names, in head order; each base copy excuses one head miss."""
    remaining = collections.Counter(key(n) for n in base_missed)
    new, old = [], []
    for name in head_missed:
        k = key(name)
        if remaining[k] > 0:
            remaining[k] -= 1
            old.append(name)
        else:
            new.append(name)
    return new, old


def rust_escape(text: str) -> str:
    return "".join("\\" + c if c in RUST_SPECIAL else c for c in text)


def name_regex(name: str) -> str:
    """A regex for `--exclude-re`/`-F` that matches the mutant at any line and column."""
    m = LINE_RE.match(name)
    if not m:
        raise ValueError(f"not a cargo-mutants line: {name!r}")
    return "^" + rust_escape(m.group(1)) + ":[0-9]+:[0-9]+: " + rust_escape(m.group(2)) + "$"


def read_names(path: str) -> list[str]:
    """Non-blank lines of a cargo-mutants list file. A missing file raises: a run that wrote no list is broken, not empty."""
    with open(path) as f:
        names = [line.rstrip("\n") for line in f if line.strip()]
    for n in names:
        key(n)
    return names


def summary(counts: dict[str, int], new: list[str], old: list[str], timeouts: list[str]) -> str:
    lines = [
        f"cargo-mutants: {counts['caught']} caught, {counts['missed']} missed, "
        f"{counts['unviable']} unviable, {counts['timeout']} timed out"
    ]
    sections = (
        ("NEW misses (catch each with a test, or waive it with a reason):", new),
        ("Old misses in the touched code (missed on the base commit too; not blocking):", old),
        ("Timed out (counted as caught):", timeouts),
    )
    for title, names in sections:
        if names:
            lines.append(title)
            lines.extend(f"  {n}" for n in names)
    if new:
        lines.append(f"FAIL: {len(new)} new miss" + ("" if len(new) == 1 else "es"))
    else:
        lines.append("PASS: no new misses")
    return "\n".join(lines)
