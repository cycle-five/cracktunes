#!/usr/bin/env python3
"""Check that CHANGELOG.md is ready to be a GitHub release's notes.

    scripts/check_changelog.py [--root DIR]

On a tag push, cargo-dist copies the CHANGELOG section headed with the tag's
version into the GitHub release. With no such heading the release gets no notes
and nothing says so, which is how v0.7.0 to v0.46.0 shipped. Rules (each
message names its rule):

1. version-heading: the version in the root Cargo.toml (`[workspace.package]`,
   else `[package]`) has a `## vX.Y.Z` heading, optionally followed by a date:
   `## v0.46.0 (2026-10-28)`. The bump commit renames `## Unreleased` to it.
2. mention: no `@name` outside code. GitHub turns one in a release into a
   mention and notifies whoever owns that login (ct#656 notified two real
   users this way). Put a Discord user like `@Jo` in backticks. Inline code
   spans and fenced code blocks are skipped; an email address is not a mention.

Exit codes: 0 pass, 1 a rule failed, 2 anything else (bad arguments, a missing
or unreadable file, no version). A broken input must never look like a pass.

Stdlib only.
"""
import argparse
import os
import re
import sys
import tomllib

CODE_SPAN = re.compile(r"(`+).+?\1")
FENCE = re.compile(r"\s*(```|~~~)")
# A GitHub login: alphanumerics and inner hyphens. Not after a word character
# (an email address) or a `/` (a URL path).
MENTION = re.compile(r"(?<![\w/.+-])@[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?")


def workspace_version(cargo_toml):
    """The root Cargo.toml's version. Raises KeyError when there is none."""
    data = tomllib.loads(cargo_toml)
    package = data.get("workspace", {}).get("package") or data.get("package") or {}
    version = package["version"]
    if not isinstance(version, str):
        raise KeyError("version")
    return version


def check(version, changelog):
    """Return problems, as (line, message), for one CHANGELOG text."""
    lines = changelog.splitlines()
    problems = []
    heading = re.compile(rf"## v{re.escape(version)}(?:\s|$)")
    if not any(heading.match(line) for line in lines):
        problems.append((1, f"version-heading: no `## v{version}` heading; the bump "
                         f"renames `## Unreleased` to `## v{version} (YYYY-MM-DD)`"))
    fenced = False
    for i, line in enumerate(lines):
        if FENCE.match(line):
            fenced = not fenced
            continue
        if fenced:
            continue
        for m in MENTION.finditer(CODE_SPAN.sub("", line)):
            problems.append((i + 1, f"mention: `{m.group()}` would notify a GitHub "
                             "user from the release notes; put it in backticks"))
    return problems


def main(argv=None):
    try:
        ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
        ap.add_argument("--root", default=os.path.dirname(
            os.path.dirname(os.path.abspath(__file__))))
        args = ap.parse_args(argv)
    except SystemExit as e:
        return 2 if e.code else 0
    try:
        with open(os.path.join(args.root, "Cargo.toml"), encoding="utf-8") as f:
            version = workspace_version(f.read())
        with open(os.path.join(args.root, "CHANGELOG.md"), encoding="utf-8") as f:
            problems = check(version, f.read())
    except Exception as e:  # noqa: BLE001 - any breakage is exit 2, never a pass
        print(f"check_changelog: error: {e!r}", file=sys.stderr)
        return 2
    for ln, msg in problems:
        print(f"CHANGELOG.md:{ln}: {msg}")
    print(f"v{version}: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
