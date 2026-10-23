#!/usr/bin/env python3
"""The cargo-mutants gate: mutation-test the lines a change touched, and fail
only on NEW misses.

    scripts/mutants_check.py [--base REV] [--head REV] [--shard K/N]
                             [--output DIR] [--summary-md FILE] [--repo DIR]
    scripts/mutants_check.py [--base REV] [--head REV] --write-head-diff FILE

The gate runs cargo-mutants on the code a PR touched and fails only on NEW
misses (spec section 4, "New or old"). A miss is old if the base commit has
the same mutant missed too, so a PR is not blamed for what it did not break.

The pipeline: the base→head diff gives each side a set of lines that
correspond line for line (see "The corresponding selection" below). The head's
lines select the mutants, which run on a snapshot of the head. If any are
missed, those mutants alone run again on a snapshot of the base, selected by
name (at any line) and by the base's lines. A miss the base misses too is old;
the rest are new.

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
import typing

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


class CheckError(Exception):
    """A broken input or environment: main() reports it and exits 2."""


# --- The corresponding selection ----------------------------------------------
#
# 🔑 Which lines each run tests (spec section 4, "New or old"). cargo-mutants'
# `--in-diff` marks lines by its own rule (in_diff.rs `affected_lines`): an
# inserted line marks itself, and a deleted line marks the line before it and
# the line after it in the new file. The rule is not symmetric, so the
# REVERSED diff on the base marks other code than the diff marks on the head:
# for a pure insertion the base side also picks up the line above the gap (an
# untested twin there excused the new line's miss: a silent pass), and for a
# pure deletion the head side picks up the gap's neighbours, which the base
# side never looked at (an old miss there read as new: a false block).
#
# So both sides are computed from ONE parsed diff: each side's marks by
# cargo-mutants' rule, mapped onto the other side through the diff's line
# correspondence, and unioned. Each run then gets a synthetic diff that marks
# exactly its set, one single-line insertion hunk per line, which cargo-mutants
# accepts (its new-text check) and marks as that line alone. The head run
# grows slightly; that is what makes the two runs' counts comparable.


class Hunk(typing.NamedTuple):
    """One hunk: its two ranges as the header gives them, and one kind per line."""
    old_start: int
    old_len: int
    new_start: int
    new_len: int
    kinds: str  # ' ' context, '-' deleted, '+' inserted


class FilePatch(typing.NamedTuple):
    old: str | None  # the path on the base; None for a file the head adds
    new: str | None  # the path on the head; None for a file the head deletes
    hunks: list[Hunk]


HUNK_RE = re.compile(rb"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def _patch_path(raw: bytes, prefix: bytes) -> str | None:
    if raw.endswith(b"\t"):  # git ends a name that has a space with a tab
        raw = raw[:-1]
    if raw == b"/dev/null":
        return None
    if raw.startswith(b'"'):
        raise CheckError(f"a quoted path in the diff is not supported: {raw!r}")
    if not raw.startswith(prefix):
        raise CheckError(f"a diff path without the {prefix.decode()} prefix: {raw!r}")
    try:
        return raw[len(prefix):].decode("utf-8")
    except UnicodeDecodeError:
        raise CheckError(f"a diff path that is not UTF-8: {raw!r}") from None


def parse_diff(diff: bytes) -> list[FilePatch]:
    """The file patches of a `git diff` (explicit a/ and b/ prefixes).

    Hunk lines are read by the header's counts, so a deleted line that reads
    `--- x` is never mistaken for a header. A short hunk is a broken diff."""
    lines = diff.split(b"\n")
    if lines and lines[-1] == b"":
        lines.pop()
    patches: list[FilePatch] = []
    old = new = None
    hunks: list[Hunk] = []
    started = False

    def flush():
        if started:
            patches.append(FilePatch(old, new, hunks))

    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith(b"diff --git "):
            flush()
            old, new, hunks, started = None, None, [], True
        elif not started:
            raise CheckError(f"not a git diff: {line[:80]!r}")
        elif line.startswith(b"--- ") and not hunks:
            old = _patch_path(line[4:], b"a/")
        elif line.startswith(b"+++ ") and not hunks:
            new = _patch_path(line[4:], b"b/")
        elif m := HUNK_RE.match(line):
            old_start, new_start = int(m.group(1)), int(m.group(3))
            old_len = int(m.group(2)) if m.group(2) is not None else 1
            new_len = int(m.group(4)) if m.group(4) is not None else 1
            want_old, want_new, kinds = old_len, new_len, []
            while want_old > 0 or want_new > 0:
                i += 1
                if i >= len(lines):
                    raise CheckError(f"a hunk ends early: {line!r}")
                c = lines[i][:1]
                if c == b"\\":  # "\ No newline at end of file"
                    continue
                if c in (b" ", b""):  # b"": diff.suppressBlankEmpty
                    want_old, want_new = want_old - 1, want_new - 1
                    kinds.append(" ")
                elif c == b"-":
                    want_old -= 1
                    kinds.append("-")
                elif c == b"+":
                    want_new -= 1
                    kinds.append("+")
                else:
                    raise CheckError(f"an unexpected line in a hunk: {lines[i][:80]!r}")
                if want_old < 0 or want_new < 0:
                    raise CheckError(f"a hunk longer than its header: {line!r}")
            hunks.append(Hunk(old_start, old_len, new_start, new_len, "".join(kinds)))
        # Anything else is an extended header line (index, mode, rename,
        # similarity, "Binary files ... differ") or a no-newline marker.
        i += 1
    flush()
    return patches


def affected_lines(hunks: list[Hunk]) -> list[int]:
    """The new side's lines cargo-mutants 27.1.0 marks for these hunks.

    A port of in_diff.rs `affected_lines`, line for line: an inserted line is
    marked; a deleted line marks the line before it (if any) and the next line
    that exists in the new file (if any)."""
    affected: list[int] = []
    for h in hunks:
        lineno = h.new_start
        prev_removed = False
        for kind in h.kinds:
            if kind != "-" and prev_removed:
                affected.append(lineno)
                prev_removed = False
            if kind == " ":
                lineno += 1
            elif kind == "+":
                if not affected or affected[-1] != lineno:
                    affected.append(lineno)
                lineno += 1
            else:
                prev_removed = True
                if lineno > 1 and (not affected or affected[-1] != lineno - 1):
                    affected.append(lineno - 1)
    return affected


def reverse_hunk(h: Hunk) -> Hunk:
    """The same hunk seen from the other side, as git would print it.

    🪤 Not just the signs swapped: git prints each change's deletions before
    its insertions, and the order matters to affected_lines (a deletion marks
    the line before it, an insertion run does not)."""
    kinds, minus, plus = [], [], []
    for kind in h.kinds + " ":
        if kind == " ":
            kinds += minus + plus + [" "]
            minus, plus = [], []
        elif kind == "+":
            minus.append("-")
        else:
            plus.append("+")
    return Hunk(h.new_start, h.new_len, h.old_start, h.old_len, "".join(kinds[:-1]))


def map_lines(lines, hunks: list[Hunk]) -> set[int]:
    """Map old-side line numbers to the new side. A deleted line has no
    counterpart and is dropped; lines outside the hunks shift by the
    preceding hunks' net growth."""
    inside: dict[int, int | None] = {}
    shifts: list[tuple[int, int]] = []  # (first old line after the hunk, shift from there)
    for h in hunks:
        # A zero-length range names the line BEFORE where it would start.
        o = h.old_start if h.old_len else h.old_start + 1
        n = h.new_start if h.new_len else h.new_start + 1
        for kind in h.kinds:
            if kind == " ":
                inside[o] = n
                o, n = o + 1, n + 1
            elif kind == "-":
                inside[o] = None
                o += 1
            else:
                n += 1
        shifts.append((o, n - o))
    mapped = set()
    for line in lines:
        if line in inside:
            target = inside[line]
        else:
            shift = 0
            for after, s in shifts:
                if line >= after:
                    shift = s
            target = line + shift
        if target is not None:
            mapped.add(target)
    return mapped


def corresponding_selection(patches: list[FilePatch]) -> tuple[dict[str, list[int]],
                                                               dict[str, list[int]]]:
    """(head lines, base lines) per file, each side's cargo-mutants marks
    unioned with the other side's marks mapped across the diff."""
    head: dict[str, list[int]] = {}
    base: dict[str, list[int]] = {}
    for p in patches:
        rev = [reverse_hunk(h) for h in p.hunks]
        head_marks = affected_lines(p.hunks) if p.new else []
        base_marks = affected_lines(rev) if p.old else []
        if p.new:
            lines = set(head_marks) | map_lines(base_marks, p.hunks)
            if lines:
                head[p.new] = sorted(lines)
        if p.old:
            lines = set(base_marks) | map_lines(head_marks, rev)
            if lines:
                base[p.old] = sorted(lines)
    return head, base


def synthetic_diff(selection: dict[str, list[int]], read) -> bytes:
    """A diff that makes cargo-mutants mark exactly `selection`: per line L,
    `@@ -(L-1),0 +L,1 @@` and `+` the tree's line L, byte for byte.

    `read(path)` returns a file's bytes as the run's tree has them."""
    out: list[bytes] = []
    for path in sorted(selection):
        text = read(path)
        lines = text.split(b"\n")
        if text.endswith(b"\n"):
            lines.pop()
        name = path.encode("utf-8")
        tab = b"\t" if b" " in name else b""
        out += [b"diff --git a/" + name + b" b/" + name,
                b"--- a/" + name + tab, b"+++ b/" + name + tab]
        for n in selection[path]:
            if not 1 <= n <= len(lines):
                raise CheckError(f"{path} has {len(lines)} lines, but the diff marks line {n}")
            out += [f"@@ -{n - 1},0 +{n},1 @@".encode(), b"+" + lines[n - 1]]
    return b"".join(line + b"\n" for line in out)


def tree_file(tools: "Tools", rev: str, path: str) -> bytes:
    """A file of commit rev as a checkout (and so `git archive`) writes it."""
    return tools.git_bytes("cat-file", "--filters", f"{rev}:{path}")


# --- The pipeline -----------------------------------------------------------


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


def rust_diff(tools: Tools, old: str, new: str) -> bytes:
    """The Rust part of the old→new diff, byte for byte.

    🔑 Every option that shapes it is pinned, so no user's git config changes
    what is selected: explicit a/ b/ prefixes (cargo-mutants strips only a
    literal `b/`; diff.noprefix or diff.dstPrefix would match no file and the
    check would pass), `--find-renames` (diff.renames=false turns a rename into
    a whole-file addition), `--unified=3` (diff.context moves the lines a
    deletion marks), `--no-textconv`, and unquoted UTF-8 paths.
    """
    return tools.git_bytes("-c", "core.quotePath=false", "diff", "--no-ext-diff",
                           "--no-color", "--no-textconv", "--unified=3", "--find-renames",
                           "--src-prefix=a/", "--dst-prefix=b/", old, new, "--", "*.rs")


def selection_diffs(tools: Tools, base: str, head: str) -> tuple[bytes, bytes, bytes]:
    """(the change's own diff, the head run's synthetic diff, the base run's).

    Both synthetic diffs come from the one change diff, so their lines
    correspond (see "The corresponding selection" above)."""
    change = rust_diff(tools, base, head)
    head_lines, base_lines = corresponding_selection(parse_diff(change))
    head_diff = synthetic_diff(head_lines, lambda path: tree_file(tools, head, path))
    base_diff = synthetic_diff(base_lines, lambda path: tree_file(tools, base, path))
    return change, head_diff, base_diff


def write_bytes(path: str, data: bytes) -> None:
    with open(path, "wb") as f:
        f.write(data)


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


def base_misses(tools: Tools, base: str, out: str, head_missed: list[str]) -> list[str]:
    """Run the head's missed mutants on a snapshot of the base; return the base's misses."""
    diff = os.path.join(out, "base.diff")
    if os.path.getsize(diff) == 0:
        # Every head line the run marked is new: nothing on the base corresponds.
        return []
    tree = os.path.join(out, "base-tree")
    tools.snapshot(base, tree)
    # A miss in a file the base lacks is new without a run.
    on_base = [m for m in head_missed if os.path.isfile(os.path.join(tree, mutant_file(m)))]
    if not on_base:
        return []
    # 🔑 --re alone is not enough: cargo-mutants 27.1 doesn't apply it to
    # "delete field" mutants (visit.rs:747). --in-diff does apply to them, so
    # the base's corresponding selection keeps those extras to the touched code.
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
    change, head_diff, base_diff = selection_diffs(tools, base, head)
    if args.write_head_diff:
        write_bytes(args.write_head_diff, head_diff)
        print(f"wrote the head selection for {base[:12]}..{head[:12]} to {args.write_head_diff}"
              f" ({head_diff.count(b'@@ -')} line(s))")
        return 0
    if not change:
        print(f"no Rust changes between {base[:12]} and {head[:12]}")
        return 0
    if not head_diff:
        print(f"no Rust lines to mutate on {head[:12]}: the change only deletes or renames")
        return 0
    out = prepare_output(args.output or os.path.join(
        scratch_root(), f"check-{head[:12]}-{int(time.time())}"))
    head_tree = os.path.join(out, "head-tree")
    try:
        write_bytes(os.path.join(out, "change.diff"), change)
        write_bytes(os.path.join(out, "head.diff"), head_diff)
        write_bytes(os.path.join(out, "base.diff"), base_diff)
        tools.snapshot(head, head_tree)
        shard = ["--shard", args.shard, "--sharding", "round-robin"] if args.shard else []
        head_out = os.path.join(out, "head")
        code = run_mutants(tools, ["-d", head_tree, "--in-diff", os.path.join(out, "head.diff"),
                                   *shard], head_out, "head")
        results = read_results(head_out, code, "head")
        missed = results["missed"]
        base_missed = base_misses(tools, base, out, missed) if missed else []
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
    p.add_argument("--write-head-diff", metavar="FILE",
                   help="write the diff that selects the head run's lines to FILE and exit 0 "
                   "without running cargo-mutants (an empty FILE: nothing to mutate); CI's Plan "
                   "job counts the mutants with it")
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
