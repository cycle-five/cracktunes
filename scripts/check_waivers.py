#!/usr/bin/env python3
"""Check that every cargo-mutants waiver carries a written reason.

    scripts/check_waivers.py [--root DIR]

A mutant no test catches blocks a change unless it is waived, and a waiver must
say why, so a reviewer can disagree. Rules (each message names its rule):

1. rust-bare-form: `#[mutants::skip]` (or `#![mutants::skip]`) makes `mutants`
   a release dependency. Write `#[cfg_attr(test, mutants::skip)]`.
2. rust-reason: any other non-comment line that mentions `mutants::skip` is a
   waiver (`#[cfg_attr(test, …)]`, an inner `#![cfg_attr(…)]`, another cfg
   predicate such as `debug_assertions`, an attribute among others on one line
   or wrapped over several), and needs, on the line directly above, a `//`
   comment with text. `///` and `//!` are docs, not reasons.
3. config-reason: every element of a key in .cargo/mutants.toml that narrows
   what is mutated (`exclude_globs`, `exclude_re`, `examine_globs`,
   `examine_re`, `skip_calls`; quoted keys too) sits on its own line, directly
   below a `#` comment with text. A one-element array on its key's line takes
   the comment above the key.
4. config-count: the line scanner's element count per key must equal what
   tomllib parses; anything the scanner can't see is reported, never passed.

Checks the files `git ls-files '*.rs'` lists under --root, plus
.cargo/mutants.toml (a missing file has no waivers).

Exit codes: 0 pass, 1 a waiver lacks a reason, 2 anything else (bad arguments,
unreadable TOML, git failure). A broken input must never look like a pass.

Stdlib only.
"""
import argparse
import os
import re
import subprocess
import sys
import tomllib

SKIP = re.compile(r"mutants\s*::\s*skip")
BARE = re.compile(r"#\s*!?\s*\[\s*mutants\s*::\s*skip")
CONFIG = os.path.join(".cargo", "mutants.toml")
# The keys that narrow what cargo-mutants mutates: each element is a waiver.
NARROWING = ("exclude_globs", "exclude_re", "examine_globs", "examine_re", "skip_calls")
KEY = re.compile(r"""\s*(?:"(\w+)"|'(\w+)'|(\w+))\s*=\s*\[(.*)$""")


def has_reason(line):
    """A `//` comment (not `///` or `//!`) with text after the slashes."""
    s = line.strip()
    return (s.startswith("//") and not s.startswith(("///", "//!"))
            and bool(s[2:].strip()))


def is_comment(stripped):
    """A line comment, or a line that opens or continues a block comment."""
    return stripped.startswith(("//", "/*", "*"))


def check_rust(path, text):
    """Return (waivers, problems) for one Rust file; problems are (line, msg)."""
    lines = text.splitlines()
    waivers, problems = 0, []
    for i, raw in enumerate(lines):
        s = raw.strip()
        if is_comment(s) or not SKIP.search(s):
            continue  # a commented-out waiver is no waiver
        waivers += 1
        if BARE.search(s):
            problems.append((i + 1, "rust-bare-form: write "
                             "`#[cfg_attr(test, mutants::skip)]`, the bare form "
                             "makes `mutants` a release dependency"))
        elif i == 0 or not has_reason(lines[i - 1]):
            problems.append((i + 1, "rust-reason: a mutants::skip waiver needs "
                             "a `//` comment with the reason on the line "
                             "directly above (not `///` or `//!`)"))
    return waivers, problems


def scan_strings(text):
    """Count the TOML strings in one line of an array, up to a `#` comment or
    the closing `]`. Return (count, closed)."""
    i, count = 0, 0
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while i < len(text) and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
            count, i = count + 1, i + 1
        elif c == "'":
            end = text.find("'", i + 1)
            count, i = count + 1, (len(text) if end < 0 else end + 1)
        elif c == "#":
            break
        elif c == "]":
            return count, True
        else:
            i += 1
    return count, False


def config_reason(line):
    """A `#` comment with text after the hashes."""
    s = line.strip()
    return s.startswith("#") and bool(s.strip("# \t"))


def check_config(text):
    """Return (waivers, problems) for mutants.toml. Raises tomllib.TOMLDecodeError."""
    data = tomllib.loads(text)
    lines = text.splitlines()
    waivers, problems = 0, []
    scanned, key_lines = {}, {}
    i = 0
    while i < len(lines):
        m = KEY.match(lines[i])
        key = m and (m.group(1) or m.group(2) or m.group(3))
        if key not in NARROWING:
            i += 1
            continue
        key_lines.setdefault(key, i + 1)
        count, at, rest = 0, i, m.group(4)
        while True:
            n, closed = scan_strings(rest)
            if n:
                count += n
                what = f"`{key}` on one line" if at == i else f"an element of `{key}`"
                if at == 0 or not config_reason(lines[at - 1]):
                    problems.append((at + 1, f"config-reason: {what} needs a `#` comment "
                                     "with the reason on the line directly above"))
                if n > 1:
                    problems.append((at + 1, f"config-reason: {n} elements of `{key}` on "
                                     "one line; give each its own line and its own reason"))
            if closed or at + 1 >= len(lines):
                break
            at += 1
            rest = lines[at]
            if rest.strip().startswith("#"):
                rest = ""  # a comment line inside the array
        scanned[key] = scanned.get(key, 0) + count
        waivers += count
        i = at + 1
    for key in NARROWING:
        if key not in data:
            continue
        want = len(data[key]) if isinstance(data[key], list) else 1
        if key not in scanned:
            problems.append((1, f"config-count: `{key}` is set, but the line scanner did "
                             f"not find it; write it as `{key} = [` at the start of a line"))
        elif scanned[key] != want:
            problems.append((key_lines[key], f"config-count: `{key}` has {want} element(s), "
                             f"but the line scanner counted {scanned[key]}; write one "
                             "string per line"))
    return waivers, problems


def main(argv=None):
    try:
        ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
        ap.add_argument("--root", default=os.path.dirname(
            os.path.dirname(os.path.abspath(__file__))))
        args = ap.parse_args(argv)
    except SystemExit as e:
        return 2 if e.code else 0
    try:
        ls = subprocess.run(["git", "-C", args.root, "ls-files", "-z", "*.rs"],
                            capture_output=True, check=True)
        files = [f for f in ls.stdout.decode().split("\0") if f]
        total, report = 0, []
        for rel in files:
            with open(os.path.join(args.root, rel), encoding="utf-8",
                      errors="replace") as f:
                n, probs = check_rust(rel, f.read())
            total += n
            report += [(rel, ln, msg) for ln, msg in probs]
        cfg = os.path.join(args.root, CONFIG)
        if os.path.exists(cfg):
            with open(cfg, encoding="utf-8") as f:
                n, probs = check_config(f.read())
            total += n
            report += [(CONFIG, ln, msg) for ln, msg in probs]
    except Exception as e:  # noqa: BLE001 - any breakage is exit 2, never a pass
        print(f"check_waivers: error: {e}", file=sys.stderr)
        return 2
    for path, ln, msg in report:
        print(f"{path}:{ln}: {msg}")
    print(f"{total} waiver(s) checked, {len(report)} problem(s)")
    return 1 if report else 0


if __name__ == "__main__":
    sys.exit(main())
