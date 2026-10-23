#!/usr/bin/env python3
"""Check that every cargo-mutants waiver carries a written reason.

    scripts/check_waivers.py [--root DIR]

A mutant no test catches blocks a change unless it is waived, and a waiver must
say why, so a reviewer can disagree. Rules (each message names its rule):

1. rust-bare-form: `#[mutants::skip]` makes `mutants` a release dependency.
   Write `#[cfg_attr(test, mutants::skip)]`.
2. rust-reason: `#[cfg_attr(test, mutants::skip)]` needs, on the line directly
   above, a `//` comment with text. `///` and `//!` are docs, not reasons.
3. config-reason: every element of `exclude_globs` / `exclude_re` in
   .cargo/mutants.toml sits on its own line, directly below a `#` comment. A
   one-line array counts as one element, with the comment above its key.

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

BARE = re.compile(r"#\s*\[\s*mutants\s*::\s*skip")
CFG = re.compile(r"#\s*\[\s*cfg_attr\s*\(\s*test\s*,\s*mutants\s*::\s*skip")
CONFIG = os.path.join(".cargo", "mutants.toml")
KEY = re.compile(r"\s*(exclude_globs|exclude_re)\s*=\s*\[(.*)$")


def has_reason(line):
    """A `//` comment (not `///` or `//!`) with text after the slashes."""
    s = line.strip()
    return (s.startswith("//") and not s.startswith(("///", "//!"))
            and bool(s[2:].strip()))


def check_rust(path, text):
    """Return (waivers, problems) for one Rust file; problems are (line, msg)."""
    lines = text.splitlines()
    waivers, problems = 0, []
    for i, raw in enumerate(lines):
        s = raw.strip()
        if not s.startswith("#"):
            continue  # also skips comments: a commented-out waiver is no waiver
        if BARE.match(s):
            waivers += 1
            problems.append((i + 1, "rust-bare-form: write "
                             "`#[cfg_attr(test, mutants::skip)]`, the bare form "
                             "makes `mutants` a release dependency"))
        elif CFG.match(s):
            waivers += 1
            if i == 0 or not has_reason(lines[i - 1]):
                problems.append((i + 1, "rust-reason: a mutants::skip waiver needs "
                                 "a `//` comment with the reason on the line "
                                 "directly above (not `///` or `//!`)"))
    return waivers, problems


def check_config(text):
    """Return (waivers, problems) for mutants.toml. Raises tomllib.TOMLDecodeError."""
    tomllib.loads(text)
    lines = text.splitlines()
    waivers, problems = 0, []

    def commented(i):
        return i > 0 and lines[i - 1].strip().startswith("#")

    def need(i, what):
        if not commented(i):
            problems.append((i + 1, f"config-reason: {what} needs a `#` comment "
                             "with the reason on the line directly above"))

    i = 0
    while i < len(lines):
        m = KEY.match(lines[i])
        if not m:
            i += 1
            continue
        rest = m.group(2).split("#")[0].strip()
        if rest:  # elements on the key line: one-line (or opened-with-elements) array
            waivers += 1
            need(i, f"`{m.group(1)}` on one line")
            while "]" not in rest and i + 1 < len(lines):
                i += 1
                rest = lines[i].split("#")[0].strip()
        else:
            i += 1
            while i < len(lines) and not lines[i].strip().startswith("]"):
                s = lines[i].strip()
                if s and not s.startswith("#"):
                    waivers += 1
                    need(i, f"element {s.rstrip(',')}")
                i += 1
        i += 1
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
