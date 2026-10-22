#!/usr/bin/env python3
"""Compare cargo-mutants' missed list against a shrink-only baseline.

The baseline (.cargo/mutants-baseline.txt) records mutants we know the tests
miss. It may only shrink: a new miss fails `compare`, and `refresh` refuses to
add anything.

🔑 Entries are keyed WITHOUT line and column. An unrelated edit above a
mutant shifts its line number, and a line-keyed baseline would then report a
known miss as new and fail CI for nothing. The key is the file plus the
description, so `file.rs:48:5: replace x with y` becomes
`file.rs: replace x with y`.

🪤 The same description can occur several times in one file (three
`replace && with ||` in one function), so the comparison counts copies:
a baseline with one copy and a run with two is one new miss.

    mutants_baseline.py compare --baseline FILE --missed FILE
    mutants_baseline.py refresh --baseline FILE --missed FILE [--init]

Baseline format: one key per line, sorted; `#` lines and blanks are ignored.
Stdlib only.
"""
import argparse
import collections
import os
import re
import sys

LINE_RE = re.compile(r"^(.+?):\d+:\d+: (.+)$")
DEFAULT_HEADER = [
    "# Mutants the test suite is known to miss (shrink-only ratchet).",
    "# Keys have no line:col. Regenerate with: scripts/mutants_baseline.py refresh",
]


def key(line: str) -> str:
    m = LINE_RE.match(line.rstrip("\n"))
    if not m:
        raise ValueError(f"not a cargo-mutants line: {line!r}")
    return f"{m.group(1)}: {m.group(2)}"


def _content_lines(path):
    with open(path) as f:
        for raw in f:
            line = raw.rstrip("\n")
            if line.strip() and not line.startswith("#"):
                yield line


def load_baseline(path) -> collections.Counter:
    """Count a BASELINE file's lines, which are already keys (no key() applied)."""
    return collections.Counter(_content_lines(path))


def load_missed(path) -> collections.Counter:
    """Key and count a MISSED list, whose lines are full `file:L:C: text` (raises ValueError otherwise)."""
    return collections.Counter(key(line) for line in _content_lines(path))


def _load_baseline(path):
    return load_baseline(path) if os.path.exists(path) else collections.Counter()


def _header(path):
    if not os.path.exists(path):
        return list(DEFAULT_HEADER)
    header = []
    with open(path) as f:
        for raw in f:
            line = raw.rstrip("\n")
            if line.startswith("#") or not line.strip():
                header.append(line)
            else:
                break
    return header


def _write(path, header, counter):
    with open(path, "w") as f:
        for h in header:
            f.write(h + "\n")
        for k in sorted(counter.elements()):
            f.write(k + "\n")


def _new_misses(baseline, missed_path):
    """Return (new full lines, known count), consuming baseline copies."""
    remaining = collections.Counter(baseline)
    new, known = [], 0
    for line in _content_lines(missed_path):
        k = key(line)
        if remaining[k] > 0:
            remaining[k] -= 1
            known += 1
        else:
            new.append(line)
    return new, known


def cmd_compare(args):
    new, known = _new_misses(_load_baseline(args.baseline), args.missed)
    print(f"{len(new)} new missed mutant(s)")
    for line in new:
        print(f"  NEW {line}")
    print(f"{known} known missed mutant(s) (in the baseline)")
    return 1 if new else 0


def cmd_refresh(args):
    exists = os.path.exists(args.baseline)
    missed = load_missed(args.missed)
    if args.init:
        if exists:
            print(f"refusing --init: {args.baseline} already exists", file=sys.stderr)
            return 2
        _write(args.baseline, list(DEFAULT_HEADER), missed)
        print(f"wrote {sum(missed.values())} key(s) to {args.baseline}")
        return 0
    baseline = _load_baseline(args.baseline)
    grown = missed - baseline
    if grown:
        print(f"refusing to grow the baseline: {sum(grown.values())} new key(s)", file=sys.stderr)
        for k in sorted(grown.elements()):
            print(f"  NEW {k}", file=sys.stderr)
        return 1
    kept = baseline & missed
    removed = sum(baseline.values()) - sum(kept.values())
    _write(args.baseline, _header(args.baseline), kept)
    print(f"removed {removed} key(s); {sum(kept.values())} remain")
    return 0


COMPARE_HELP = "Exit 1 if the missed list has a mutant the baseline does not list."
# 🪤 refresh keeps only the baseline entries that are in --missed, so a partial
# list silently deletes every entry outside it.
REFRESH_HELP = (
    "Rewrite the baseline from a missed list, refusing to add entries. "
    "--missed must be the missed list of a COMPLETE full run (the mutants-missed "
    "artifact): a list from one file, one module, an --in-diff run or one shard "
    "drops every baseline entry outside it."
)


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    commands = (
        ("compare", cmd_compare, COMPARE_HELP),
        ("refresh", cmd_refresh, REFRESH_HELP),
    )
    for name, fn, about in commands:
        s = sub.add_parser(name, help=about, description=about)
        s.add_argument("--baseline", required=True)
        s.add_argument("--missed", required=True)
        if name == "refresh":
            s.add_argument("--init", action="store_true")
        s.set_defaults(fn=fn)
    args = p.parse_args(argv)
    # 🔑 Exit 1 means "new misses"; a broken input must not look like that.
    try:
        return args.fn(args)
    except ValueError as e:
        print(f"error: {args.missed}: {e}", file=sys.stderr)
        return 2
    except OSError as e:
        print(f"error: cannot read {e.filename or 'input'}: {e.strerror or e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
