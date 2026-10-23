#!/usr/bin/env python3
"""Sweep stale build state without ever clearing the shared target dir.

🔑 The cargo target directory here is shared: `build.target-dir` in
~/.cargo/config.toml points every project at one directory, so other
projects may be building into it right now. Deleting it, or running
`cargo clean`, is a person's call and never a script's. Incremental
compilation state is the part that grows without bound (cargo-mutants runs
add a `mutants/incremental` of their own) and it is safe to drop, because the
next build just recompiles. So this deletes exactly two things:

  * children of any `incremental/` directory 1-3 levels under the target dir
    (`debug/incremental`, `mutants/incremental`, `<triple>/debug/incremental`)
    untouched for more than 14 days;
  * `cargo-mutants-*` scratch copies under the scratch root (the wrapper's
    $TMPDIR) untouched for more than 24 hours, i.e. left by a killed run.

🪤 It refuses a directory with no CACHEDIR.TAG (cargo writes one in every
target directory), so a wrong `cargo metadata` answer can't point it at
something that isn't a target dir. Above 150 GiB it only warns.

    scripts/sweep_target.py [--dry-run] [--scratch-root DIR]

Stdlib only.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

INCREMENTAL_MAX_AGE_DAYS = 14
SCRATCH_MAX_AGE_HOURS = 24
WARN_BYTES = 150 * 1024**3
SCRATCH_PREFIX = "cargo-mutants-"
MAX_DEPTH = 3  # depth of an `incremental` dir below the target dir


def target_dir() -> Path:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        check=True, capture_output=True, text=True,
    ).stdout
    return Path(json.loads(out)["target_directory"])


def _incremental_dirs(target: Path):
    """Directories named `incremental` at depth 1..MAX_DEPTH, no symlinks."""
    stack = [(target, 0)]
    while stack:
        d, depth = stack.pop()
        try:
            entries = list(os.scandir(d))
        except OSError:
            continue
        for e in entries:
            if not e.is_dir(follow_symlinks=False):
                continue
            p = Path(e.path)
            if e.name == "incremental":
                yield p
            elif depth + 1 < MAX_DEPTH:
                stack.append((p, depth + 1))


def stale_incremental(target: Path, now: float,
                      max_age_days: int = INCREMENTAL_MAX_AGE_DAYS) -> list[Path]:
    cutoff = now - max_age_days * 86400
    found = []
    for inc in _incremental_dirs(target):
        try:
            children = list(inc.iterdir())
        except FileNotFoundError:
            continue  # pruned by a concurrent cargo; already gone
        for child in children:
            try:
                if child.lstat().st_mtime < cutoff:
                    found.append(child)
            except FileNotFoundError:
                continue
    return sorted(found)


def stale_scratch(root: Path, now: float,
                  max_age_hours: int = SCRATCH_MAX_AGE_HOURS) -> list[Path]:
    if not root.is_dir():
        return []
    cutoff = now - max_age_hours * 3600
    found = []
    for c in root.iterdir():
        try:
            if c.name.startswith(SCRATCH_PREFIX) and c.lstat().st_mtime < cutoff:
                found.append(c)
        except FileNotFoundError:
            continue
    return sorted(found)


def _remove(p: Path):
    """Remove a file, link (never its target) or tree; a path that is already
    gone is fine, since a concurrent cargo may have pruned it."""
    try:
        if p.is_symlink() or not p.is_dir():
            p.unlink()
        else:
            shutil.rmtree(p)
    except FileNotFoundError:
        pass


def _size(target: Path):
    r = subprocess.run(["du", "-sb", str(target)], capture_output=True, text=True)
    try:
        return int(r.stdout.split()[0])
    except (IndexError, ValueError):
        return None


def main(argv=None, target: Path | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--dry-run", action="store_true", help="list, delete nothing")
    p.add_argument("--scratch-root", type=Path, default=Path(
        os.environ.get("CRACK_MUTANTS_SCRATCH")
        or os.path.join(os.path.expanduser("~"), ".cache", "cargo-mutants")))
    args = p.parse_args(argv)

    target = target if target is not None else target_dir()
    # 🪤 A target dir that does not exist yet (a fresh CI checkout, before the
    # first build) has nothing to sweep and nothing to protect. Only a directory
    # that exists without the tag is refused.
    if not target.exists():
        print(f"{target} does not exist yet: nothing to sweep")
        return 0
    if not (target / "CACHEDIR.TAG").is_file():
        print(f"refusing: {target} has no CACHEDIR.TAG, so it is not a cargo target dir",
              file=sys.stderr)
        return 2

    now = time.time()
    paths = stale_incremental(target, now) + stale_scratch(args.scratch_root, now)
    verb = "would remove" if args.dry_run else "removed"
    for path in paths:
        if not args.dry_run:
            _remove(path)
        print(f"{verb} {path}")
    print(f"{len(paths)} path(s) {verb.replace('would remove', 'to remove')}")

    size = _size(target)
    if size is None:
        print(f"target dir: {target} (size unknown)")
    else:
        print(f"target dir: {target} is {size / 1024**3:.1f} GiB")
        if size > WARN_BYTES:
            print(f"WARNING: {target} is over {WARN_BYTES // 1024**3} GiB. It is SHARED with "
                  "other projects, so this script will not clear it; whether to is a "
                  "person's call.", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
