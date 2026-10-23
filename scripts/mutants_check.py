#!/usr/bin/env python3
"""The cargo-mutants gate: mutation-test the lines a change touched, and fail
only on NEW misses.

    scripts/mutants_check.py [--base REV] [--head REV] [--shard K/N]
                             [--output DIR] [--summary-md FILE] [--repo DIR]

The gate runs cargo-mutants on the code a PR touched and fails only on NEW
misses (spec section 4, "New or old"). A miss is old if the base commit has
the same mutant missed too, so a PR is not blamed for what it did not break.

The pipeline: the base→head diff selects the mutants, which run on a snapshot
of the head. If any are missed, those mutants alone run again on a snapshot of
the base, selected by name (at any line) and by the reversed diff. A miss the
base misses too is old; the rest are new.

🔑 It reads commits, never the working tree: each side is a `git archive`
snapshot under the output directory, and cargo-mutants (through
scripts/mutants.sh) mutates a scratch copy of that.

Exit codes: 0 no new misses, 1 new misses, 2 anything else (bad arguments, a
broken environment, a cargo-mutants error). A broken input must never look
like a pass or an ordinary failure, so an unexpected exception is 2 as well.

🔑 Misses are compared by key, WITHOUT line and column. An unrelated edit above
a mutant shifts its line number, and a line-keyed comparison would call a known
miss new. The key is the file plus the description, so `file.rs:48:5: replace x
with y` becomes `file.rs: replace x with y`.

🪤 The same description can occur several times in one file (three
`replace && with ||` in one function), so copies are counted: a base with one
copy and a head with two is one new miss.

Stdlib only.
"""
import argparse
import collections
import os
import re
import shutil
import subprocess
import sys
import tarfile
import time
import traceback

LINE_RE = re.compile(r"^(.+?):\d+:\d+: (.+)$")
# The characters regex::escape escapes in Rust.
RUST_SPECIAL = set("\\.+*?()|[]{}^$#&-~")

SCRIPTS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(SCRIPTS)
MUTANTS_SH = os.path.join(SCRIPTS, "mutants.sh")
LISTS = ("missed", "caught", "unviable", "timeout")
# cargo-mutants exit codes that are results: all caught, some missed, some
# timed out. 4 means the unmutated tree fails its tests; others are errors.
RESULT_CODES = (0, 2, 3)
DISK_FULL = b"No space left on device"
MD_HEADING = "### cargo-mutants diff check"


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


def mutant_file(name: str) -> str:
    """The source file, relative to the tree root, that a mutant is in."""
    m = LINE_RE.match(name)
    if not m:
        raise ValueError(f"not a cargo-mutants line: {name!r}")
    return m.group(1)


# --- The pipeline -----------------------------------------------------------


class CheckError(Exception):
    """A broken input or environment: main() reports it and exits 2."""


class Tools:
    """The seam: every git, cargo-mutants and database command the check runs.

    Tests replace `mutants` and `database_ready`; git runs for real on a
    throwaway repo.
    """

    def __init__(self, repo: str):
        self.repo = repo

    def git_bytes(self, *args: str) -> bytes:
        proc = subprocess.run(["git", "-C", self.repo, *args], capture_output=True)
        if proc.returncode != 0:
            raise CheckError(f"git {' '.join(args)} exited {proc.returncode}: "
                             f"{proc.stderr.decode(errors='replace').strip()}")
        return proc.stdout

    def git(self, *args: str) -> str:
        return self.git_bytes(*args).decode("utf-8")

    def snapshot(self, rev: str, dest: str) -> None:
        """Extract `git archive <rev>` into dest: the tracked files of exactly that commit."""
        os.makedirs(dest)
        proc = subprocess.Popen(["git", "-C", self.repo, "archive", "--format=tar", rev],
                                stdout=subprocess.PIPE)
        try:
            with tarfile.open(fileobj=proc.stdout, mode="r|") as tar:
                tar.extractall(dest, filter="tar")
        finally:
            proc.stdout.close()
            code = proc.wait()
        if code != 0:
            raise CheckError(f"git archive {rev} exited {code}")

    def mutants(self, args: list[str], capture: bool = False) -> tuple[int, str]:
        """Run scripts/mutants.sh; return its exit code and, if captured, its stdout."""
        sys.stdout.flush()
        proc = subprocess.run([MUTANTS_SH, *args], text=True,
                              stdout=subprocess.PIPE if capture else None)
        return proc.returncode, proc.stdout or ""

    def database_ready(self) -> bool:
        """False only if DATABASE_URL is set, pg_isready exists, and it fails."""
        url = os.environ.get("DATABASE_URL")
        if not url or shutil.which("pg_isready") is None:
            return True
        return subprocess.run(["pg_isready", f"--dbname={url}"],
                              capture_output=True).returncode == 0


def resolve(tools: Tools, rev: str) -> str:
    try:
        return tools.git("rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}").strip()
    except CheckError:
        raise CheckError(f"not a commit: {rev!r}") from None


def default_base(tools: Tools, head: str) -> str:
    """The merge base of head and origin/master, or of head and master without an origin/master."""
    try:
        upstream = resolve(tools, "origin/master")
    except CheckError:
        upstream = resolve(tools, "master")
    try:
        return tools.git("merge-base", head, upstream).strip()
    except CheckError:
        raise CheckError(f"no merge base of {head[:12]} and {upstream[:12]}") from None


def scratch_root() -> str:
    """Where cargo-mutants builds: the same root scripts/mutants.sh uses."""
    return (os.environ.get("CRACK_MUTANTS_SCRATCH")
            or os.path.join(os.path.expanduser("~"), ".cache", "cargo-mutants"))


def prepare_output(path: str) -> str:
    path = os.path.abspath(path)
    if os.path.exists(path) and (not os.path.isdir(path) or os.listdir(path)):
        raise CheckError(f"--output is not empty: {path}")
    os.makedirs(path, exist_ok=True)
    return path


def write_diff(tools: Tools, old: str, new: str, path: str) -> bytes:
    """The Rust part of the old→new diff, written to path byte for byte, and returned.

    🔑 The prefixes are explicit: cargo-mutants strips only a literal `b/`, so a
    user's diff.noprefix or diff.dstPrefix would make it match no file, select
    no mutant, and the check would pass.
    """
    diff = tools.git_bytes("diff", "--no-ext-diff", "--no-color",
                           "--src-prefix=a/", "--dst-prefix=b/", old, new, "--", "*.rs")
    with open(path, "wb") as f:
        f.write(diff)
    return diff


def check_guards(tools: Tools, out: str, run: str) -> None:
    """A run whose environment broke is not a result, whatever it exited with.

    🪤 Never grep the logs for SIGKILL: nextest kills timed-out tests with it.
    """
    if not tools.database_ready():
        raise CheckError(f"guard tripped after the {run} run: pg_isready cannot reach "
                         "DATABASE_URL, so database tests may have failed for that reason alone")
    for dirpath, _, files in os.walk(os.path.join(out, "mutants.out", "log")):
        for name in files:
            path = os.path.join(dirpath, name)
            with open(path, "rb") as f:
                if DISK_FULL in f.read():
                    raise CheckError(f"guard tripped after the {run} run: the disk filled "
                                     f"({path} says 'No space left on device')")


def run_mutants(tools: Tools, selection: list[str], out: str, run: str) -> int:
    """Test the selected mutants, one job, into out; return the exit code if it is a result."""
    code, _ = tools.mutants([*selection, "-j", "1", "--output", out])
    check_guards(tools, out, run)
    if code not in RESULT_CODES:
        why = " (the unmutated tree fails its tests)" if code == 4 else ""
        raise CheckError(f"cargo-mutants exited {code} on the {run} run{why}")
    return code


def read_results(out: str, code: int, run: str, expect_lists: bool = False) -> dict[str, list[str]]:
    """A run's four lists, checked against its exit code: a mismatch is a broken run."""
    lists_dir = os.path.join(out, "mutants.out")
    if not os.path.exists(lists_dir):
        # cargo-mutants exits 0 before creating mutants.out when the diff
        # changes Rust but no line of it holds a mutant (a comment, a test).
        # 🪤 Any other code without lists is a run that never happened:
        # mutants.sh itself exits 2 (set -e) when sweep_target.py refuses.
        if code != 0 or expect_lists:
            raise CheckError(f"cargo-mutants exited {code} on the {run} run but wrote no {lists_dir}")
        print("cargo-mutants found no mutants in the changed lines")
        return {name: [] for name in LISTS}
    results = {name: read_names(os.path.join(lists_dir, f"{name}.txt")) for name in LISTS}
    # cargo-mutants' own rule (outcome.rs exit_code): any timeout 3, else any miss 2, else 0.
    expected = 3 if results["timeout"] else 2 if results["missed"] else 0
    if code != expected:
        raise CheckError(
            f"cargo-mutants exited {code} on the {run} run, but its lists "
            f"({len(results['missed'])} missed, {len(results['timeout'])} timed out) "
            f"mean exit {expected}")
    return results


def base_misses(tools: Tools, base: str, head: str, out: str, head_missed: list[str]) -> list[str]:
    """Run the head's missed mutants on a snapshot of the base; return the base's misses."""
    diff = os.path.join(out, "base.diff")
    write_diff(tools, head, base, diff)
    tree = os.path.join(out, "base-tree")
    tools.snapshot(base, tree)
    # A miss in a file the base lacks is new without a run.
    on_base = [m for m in head_missed if os.path.isfile(os.path.join(tree, mutant_file(m)))]
    if not on_base:
        return []
    # 🔑 --re alone is not enough: cargo-mutants 27.1 doesn't apply it to
    # "delete field" mutants (visit.rs:747). --in-diff does apply to them, so
    # the reversed diff keeps those extras to the touched code.
    selection = ["-d", tree, "--in-diff", diff]
    for m in on_base:
        selection += ["--re", name_regex(m)]
    code, listing = tools.mutants([*selection, "--list"], capture=True)
    if code != 0:
        raise CheckError(f"cargo-mutants --list exited {code} on the base tree")
    listed = [line for line in listing.splitlines() if line.strip()]
    for line in listed:
        key(line)  # each line must be a mutant name; anything else is a broken run
    if not listed:
        return []
    base_out = os.path.join(out, "base")
    code = run_mutants(tools, selection, base_out, "base")
    # --list named mutants, so the run must write lists.
    return read_results(base_out, code, "base", expect_lists=True)["missed"]


def report(text: str, summary_md: str | None) -> None:
    print(text)
    if summary_md:
        with open(summary_md, "a", encoding="utf-8") as f:
            f.write(f"{MD_HEADING}\n\n```text\n{text}\n```\n")


def check(args: argparse.Namespace, tools: Tools) -> int:
    head = resolve(tools, args.head)
    base = resolve(tools, args.base) if args.base else default_base(tools, head)
    out = prepare_output(args.output or os.path.join(
        scratch_root(), f"check-{head[:12]}-{int(time.time())}"))
    head_tree = os.path.join(out, "head-tree")
    try:
        head_diff = os.path.join(out, "head.diff")
        if not write_diff(tools, base, head, head_diff):
            print(f"no Rust changes between {base[:12]} and {head[:12]}")
            return 0
        tools.snapshot(head, head_tree)
        shard = ["--shard", args.shard, "--sharding", "round-robin"] if args.shard else []
        head_out = os.path.join(out, "head")
        code = run_mutants(tools, ["-d", head_tree, "--in-diff", head_diff, *shard],
                           head_out, "head")
        results = read_results(head_out, code, "head")
        missed = results["missed"]
        base_missed = base_misses(tools, base, head, out, missed) if missed else []
        new, old = split_new_old(missed, base_missed)
        counts = {name: len(names) for name, names in results.items()}
        report(summary(counts, new, old, results["timeout"]), args.summary_md)
        return 1 if new else 0
    finally:
        # Only the two snapshots go; the diffs and mutants.out stay for inspection.
        for tree in (head_tree, os.path.join(out, "base-tree")):
            shutil.rmtree(tree, ignore_errors=True)
        print(f"output: {out}")


def shard_arg(text: str) -> str:
    if not re.fullmatch(r"\d+/\d+", text):
        raise argparse.ArgumentTypeError(f"expected K/N, such as 0/4: {text!r}")
    return text


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        prog="mutants_check.py",
        description="Mutation-test the Rust lines a change touched (base..head) and fail only "
        "on NEW misses: mutants the head's tests miss that the base's tests catch, or that the "
        "base does not have. It reads commits, never the working tree. Exit codes: 0 no new "
        "misses; 1 new misses; 2 anything else (bad arguments, a broken environment, a "
        "cargo-mutants error).")
    p.add_argument("--base", metavar="REV",
                   help="the commit to compare with (default: the merge base of --head and "
                   "origin/master, or of --head and master when there is no origin/master)")
    p.add_argument("--head", metavar="REV", default="HEAD",
                   help="the commit to check (default: HEAD)")
    p.add_argument("--shard", metavar="K/N", type=shard_arg,
                   help="test only shard K of N (0-based, round-robin) of the head's mutants")
    p.add_argument("--output", metavar="DIR",
                   help="an empty or absent directory for the diffs and cargo-mutants output "
                   "(default: $CRACK_MUTANTS_SCRATCH or ~/.cache/cargo-mutants, "
                   "then check-<head>-<unix time>)")
    p.add_argument("--summary-md", metavar="FILE",
                   help="also append the summary to FILE as Markdown (CI: $GITHUB_STEP_SUMMARY)")
    p.add_argument("--repo", metavar="DIR", default=REPO,
                   help="the git repository the commits are in (default: this script's)")
    return p.parse_args(argv)


def main(argv: list[str] | None = None, make_tools=Tools) -> int:
    try:
        args = parse_args(argv)
    except SystemExit as e:  # argparse: 2 for bad arguments, 0 for --help
        return e.code
    try:
        return check(args, make_tools(os.path.abspath(args.repo)))
    except CheckError as e:
        print(f"mutants_check: {e}", file=sys.stderr)
    except Exception:  # uncaught, Python would exit 1, which reads as "new misses"
        traceback.print_exc()
        print("mutants_check: internal error (above)", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
