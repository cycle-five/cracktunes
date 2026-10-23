"""Tests for mutants_check.py. Fixture lines come from
`cargo mutants --list -f crack-core/src/economy/earn.rs`.

The pipeline tests run real git on a throwaway repo and fake only
cargo-mutants (and the database check), so they need no cargo."""
import contextlib
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mutants_check as mc  # noqa: E402

A = "crack-core/src/economy/earn.rs:48:5: replace voice_earners -> Vec<UserId> with vec![]"
AND1 = "crack-core/src/economy/earn.rs:61:17: replace && with || in voice_earners"
AND2 = "crack-core/src/economy/earn.rs:60:17: replace && with || in voice_earners"
AND_BASE = "crack-core/src/economy/earn.rs:12:17: replace && with || in voice_earners"
KA = "crack-core/src/economy/earn.rs: replace voice_earners -> Vec<UserId> with vec![]"


class KeyTests(unittest.TestCase):
    def test_key_strips_line_and_column(self):
        self.assertEqual(mc.key(A), KA)

    def test_key_rejects_malformed(self):
        for bad in ["", "no position here", "file.rs:1: missing column", "file.rs:a:b: x"]:
            with self.assertRaises(ValueError):
                mc.key(bad)


class SplitTests(unittest.TestCase):
    def test_split_counts_copies(self):
        new, old = mc.split_new_old([AND1, AND2], [AND_BASE])
        self.assertEqual(new, [AND2])
        self.assertEqual(old, [AND1])

    def test_split_line_moves_are_old(self):
        head = "f.rs:90:5: replace g -> bool with true"
        new, old = mc.split_new_old([head], ["f.rs:12:5: replace g -> bool with true"])
        self.assertEqual(new, [])
        self.assertEqual(old, [head])

    def test_split_other_file_is_new(self):
        head = "g.rs:90:5: replace g -> bool with true"
        new, old = mc.split_new_old([head], ["f.rs:12:5: replace g -> bool with true"])
        self.assertEqual(new, [head])
        self.assertEqual(old, [])


class RegexTests(unittest.TestCase):
    def test_rust_escape(self):
        self.assertEqual(
            mc.rust_escape("replace voice_earners -> Vec<UserId> with vec![]"),
            r"replace voice_earners \-> Vec<UserId> with vec!\[\]")
        self.assertEqual(
            mc.rust_escape("a && b || c += d * e.f()"),
            r"a \&\& b \|\| c \+= d \* e\.f\(\)")
        self.assertEqual(mc.rust_escape("Default::default()"), r"Default::default\(\)")

    def test_name_regex(self):
        rx = mc.name_regex("crack-core/src/x.rs:48:5: replace f -> u32 with 0")
        self.assertEqual(rx, r"^crack\-core/src/x\.rs:[0-9]+:[0-9]+: replace f \-> u32 with 0$")
        self.assertTrue(re.search(rx, "crack-core/src/x.rs:7:1: replace f -> u32 with 0"))
        self.assertFalse(re.search(rx, "crack-core/src/x.rs:7:1: replace f -> u32 with 0 in g"))
        self.assertFalse(re.search(rx, "crack-core/src/y.rs:7:1: replace f -> u32 with 0"))


class ReadNamesTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)

    def write(self, lines):
        p = os.path.join(self._tmp.name, "missed.txt")
        with open(p, "w") as f:
            f.write("".join(l + "\n" for l in lines))
        return p

    def test_read_names_skips_blank_lines(self):
        self.assertEqual(mc.read_names(self.write([A, "", "  ", AND1])), [A, AND1])

    def test_read_names_missing_file_raises(self):
        with self.assertRaises(FileNotFoundError):
            mc.read_names(os.path.join(self._tmp.name, "nope.txt"))

    def test_read_names_rejects_garbage(self):
        with self.assertRaises(ValueError):
            mc.read_names(self.write([A, "garbage without position"]))


class SummaryTests(unittest.TestCase):
    COUNTS = {"caught": 5, "missed": 2, "unviable": 1, "timeout": 1}

    def test_summary_full(self):
        self.assertEqual(
            mc.summary(self.COUNTS, ["N"], ["O"], ["T"]),
            "cargo-mutants: 5 caught, 2 missed, 1 unviable, 1 timed out\n"
            "NEW misses (catch each with a test, or waive it with a reason):\n"
            "  N\n"
            "Old misses in the touched code (missed on the base commit too; not blocking):\n"
            "  O\n"
            "Timed out (counted as caught):\n"
            "  T\n"
            "FAIL: 1 new miss")

    def test_summary_pass_leaves_empty_sections_out(self):
        self.assertEqual(
            mc.summary(self.COUNTS, [], [], []),
            "cargo-mutants: 5 caught, 2 missed, 1 unviable, 1 timed out\n"
            "PASS: no new misses")

    def test_summary_plural(self):
        text = mc.summary(self.COUNTS, ["N1", "N2"], [], [])
        self.assertTrue(text.endswith("FAIL: 2 new misses"))


# --- The pipeline -----------------------------------------------------------

CARGO_TOML = '[package]\nname = "fixture"\nversion = "0.1.0"\nedition = "2021"\n'
LIB_RS = """\
pub fn legacy(x: u32) -> u32 {
    x + 1
}

pub fn covered(x: u32) -> u32 {
    x * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn covered_doubles() {
        assert_eq!(super::covered(3), 6);
    }
}
"""
LIB_TOUCHED = LIB_RS.replace("    x + 1\n", "    // touched\n    x + 1\n")
L0 = "src/lib.rs:3:5: replace legacy -> u32 with 0"
L1 = "src/lib.rs:3:5: replace legacy -> u32 with 1"
FRESH = "src/fresh.rs:1:33: replace fresh -> bool with true"


class GitFixture:
    """A throwaway git repo: commit files, get the commit's SHA back."""
    GIT = ["git", "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
           "-c", "commit.gpgsign=false"]

    def __init__(self, path: str):
        self.path = path
        os.makedirs(path)
        self.git("init", "-q", "-b", "master")

    def git(self, *args: str) -> str:
        return subprocess.run([*self.GIT, "-C", self.path, *args],
                              check=True, capture_output=True, text=True).stdout

    def commit(self, files: dict, message: str) -> str:
        """Write each file (str or bytes; None deletes it), add exactly those paths, commit."""
        for rel, text in files.items():
            path = os.path.join(self.path, rel)
            if text is None:
                self.git("rm", "-q", "--", rel)
            else:
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "wb" if isinstance(text, bytes) else "w") as f:
                    f.write(text)
                self.git("add", "--", rel)
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD").strip()


def sections(text: str) -> dict:
    """The names listed under each heading of the LAST summary in `text`:
    {"NEW": [...], "Old": [...], "Timed": [...]}."""
    lines = text.splitlines()
    starts = [i for i, line in enumerate(lines) if re.match(r"cargo-mutants: \d+ caught", line)]
    found, current = {}, None
    for line in lines[starts[-1] + 1:] if starts else []:
        if line.startswith("  ") and current:
            found[current].append(line.strip())
        elif line.endswith(":"):
            current = line.split(" ", 1)[0]
            found[current] = []
        else:
            current = None
    return found


class Run:
    """One scripted cargo-mutants call: its exit code and the lists it writes
    (none at all with write=False), or, for `--list`, the names it prints."""

    def __init__(self, code=0, missed=(), caught=(), timeout=(), listed=(), log="", write=True):
        self.code, self.missed, self.caught = code, list(missed), list(caught)
        self.timeout, self.listed, self.log, self.write = list(timeout), list(listed), log, write


class FakeTools(mc.Tools):
    """Real git; cargo-mutants replaced by scripted runs, the database always up."""

    def __init__(self, repo, runs, db_up=True, sweep_ok=True):
        super().__init__(repo)
        self.runs = list(runs)
        self.calls = []
        self.tree_existed = []
        self.db_up, self.sweep_ok, self.sweeps = db_up, sweep_ok, 0

    def database_ready(self):
        return self.db_up

    def sweep(self):
        self.sweeps += 1
        return self.sweep_ok

    def mutants(self, args, capture=False):
        self.calls.append(list(args))
        self.tree_existed.append(os.path.isdir(args[args.index("-d") + 1]))
        if not self.runs:
            raise AssertionError(f"unexpected cargo-mutants call: {args}")
        run = self.runs.pop(0)
        if "--list" in args:
            return run.code, "".join(n + "\n" for n in run.listed)
        if not run.write:
            return run.code, ""
        outdir = os.path.join(args[args.index("--output") + 1], "mutants.out")
        os.makedirs(os.path.join(outdir, "log"))
        lists = {"missed": run.missed, "caught": run.caught, "unviable": [],
                 "timeout": run.timeout}
        for name, names in lists.items():
            with open(os.path.join(outdir, f"{name}.txt"), "w") as f:
                f.write("".join(n + "\n" for n in names))
        if run.log:
            with open(os.path.join(outdir, "log", "baseline.log"), "w") as f:
                f.write(run.log)
        return run.code, ""


def flag_values(args: list, flag: str) -> list:
    return [args[i + 1] for i, a in enumerate(args) if a == flag]


class PipelineBase(unittest.TestCase):
    """A fixture repo with a base commit, and `check` to run main() on it with FakeTools."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        root = self._tmp.name
        self.repo = GitFixture(os.path.join(root, "repo"))
        self.base = self.repo.commit(
            {"Cargo.toml": CARGO_TOML, "src/lib.rs": LIB_RS, "README.md": "fixture\n"}, "base")
        self.out = os.path.join(root, "out")

    def check(self, runs, head, *extra, out=None, **fake_options):
        fake = FakeTools(self.repo.path, runs, **fake_options)
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = mc.main(["--repo", self.repo.path, "--base", self.base, "--head", head,
                            "--output", out or self.out, *extra],
                           make_tools=lambda repo: fake)
        return code, stdout.getvalue(), stderr.getvalue(), fake

    def assert_trees_removed(self):
        self.assertFalse(os.path.exists(os.path.join(self.out, "head-tree")))
        self.assertFalse(os.path.exists(os.path.join(self.out, "base-tree")))


class PipelineTests(PipelineBase):

    def test_no_rust_changes(self):
        head = self.repo.commit({"README.md": "fixture, edited\n"}, "docs only")
        code, out, err, fake = self.check([], head)
        self.assertEqual(code, 0, err)
        self.assertIn(f"no Rust changes between {self.base[:12]} and {head[:12]}", out)
        self.assertEqual(fake.calls, [])
        self.assertFalse(os.path.exists(self.out))  # nothing to run, nothing left behind

    def test_only_deleting_rust_has_nothing_to_mutate(self):
        self.repo.commit({"src/gone.rs": "pub fn gone() -> u32 { 1 }\n"}, "add a file")
        self.base = self.repo.git("rev-parse", "HEAD").strip()
        head = self.repo.commit({"src/gone.rs": None}, "delete it")
        code, out, err, fake = self.check([], head)
        self.assertEqual(code, 0, err)
        self.assertIn(f"no Rust lines to mutate on {head[:12]}", out)
        self.assertEqual(fake.calls, [])
        self.assertFalse(os.path.exists(self.out))

    def test_write_head_diff(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        path = os.path.join(self._tmp.name, "head.diff")
        code, out, err, fake = self.check([], head, "--write-head-diff", path)
        self.assertEqual(code, 0, err)
        self.assertEqual(fake.calls, [])  # it never runs cargo-mutants
        self.assertFalse(os.path.exists(self.out))
        with open(path, "rb") as f:
            written = f.read()
        # The same synthetic diff the head run would get: lines 1-3 (see
        # test_base_call_args), one single-line insertion hunk each.
        self.assertEqual(written, (
            b"diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n"
            b"@@ -0,0 +1,1 @@\n+pub fn legacy(x: u32) -> u32 {\n"
            b"@@ -1,0 +2,1 @@\n+    // touched\n"
            b"@@ -2,0 +3,1 @@\n+    x + 1\n"))

    def test_write_head_diff_without_rust_changes_is_empty(self):
        head = self.repo.commit({"README.md": "fixture, edited\n"}, "docs only")
        path = os.path.join(self._tmp.name, "head.diff")
        code, out, err, fake = self.check([], head, "--write-head-diff", path)
        self.assertEqual(code, 0, err)
        with open(path, "rb") as f:
            self.assertEqual(f.read(), b"")
        self.assertEqual(fake.calls, [])

    def test_bad_rev_exits_2(self):
        code, out, err, fake = self.check([], "no-such-rev")
        self.assertEqual(code, 2)
        self.assertIn("no-such-rev", err)
        self.assertEqual(fake.calls, [])

    def test_output_not_empty_exits_2(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        os.makedirs(self.out)
        with open(os.path.join(self.out, "left-over"), "w") as f:
            f.write("x")
        code, out, err, fake = self.check([], head)
        self.assertEqual(code, 2)
        self.assertIn("not empty", err)
        self.assertEqual(fake.calls, [])

    def test_baseline_failure_exits_2(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check([Run(code=4)], head)
        self.assertEqual(code, 2)
        self.assertIn("exited 4", err)
        self.assertEqual(len(fake.calls), 1)
        self.assert_trees_removed()

    def test_guard_database_down_exits_2(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check([Run(code=2, missed=[L0])], head, db_up=False)
        self.assertEqual(code, 2)
        self.assertIn("pg_isready", err)
        self.assertEqual(len(fake.calls), 1)
        self.assert_trees_removed()

    def test_broken_run_goes_to_the_summary_md(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        md = os.path.join(self._tmp.name, "summary.md")
        code, out, err, fake = self.check([Run(code=4)], head, "--summary-md", md)
        self.assertEqual(code, 2)
        with open(md) as f:
            self.assertEqual(f.read(), f"{mc.MD_HEADING}\n\nbroken run: cargo-mutants exited 4 "
                             "on the head run (the unmutated tree fails its tests)\n")

    def test_internal_error_goes_to_the_summary_md(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        md = os.path.join(self._tmp.name, "summary.md")
        code, out, err, fake = self.check([Run(code=0, missed=["garbage"])], head,
                                          "--summary-md", md)
        self.assertEqual(code, 2)
        self.assertIn("internal error", err)
        with open(md) as f:
            self.assertEqual(f.read(), f"{mc.MD_HEADING}\n\n"
                             "broken run: internal error (see the job log)\n")

    def test_sweeps_once_per_check(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check(
            [Run(code=2, missed=[L0]), Run(listed=[L0]), Run(code=2, missed=[L0])], head)
        self.assertEqual(code, 0, err)
        self.assertEqual(len(fake.calls), 3)  # three mutants.sh calls ...
        self.assertEqual(fake.sweeps, 1)  # ... one sweep

    def test_no_sweep_without_a_run(self):
        head = self.repo.commit({"README.md": "fixture, edited\n"}, "docs only")
        code, out, err, fake = self.check([], head)
        self.assertEqual(code, 0, err)
        self.assertEqual(fake.sweeps, 0)

    def test_a_failed_sweep_only_warns(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check([Run()], head, sweep_ok=False)
        self.assertEqual(code, 0, err)
        self.assertIn("warning: the target sweep failed", err)

    def test_guard_disk_full_exits_2(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        log = "error: failed to write: No space left on device (os error 28)\n"
        code, out, err, fake = self.check([Run(code=2, missed=[L0], log=log)], head)
        self.assertEqual(code, 2)
        self.assertIn("No space left on device", err)
        self.assertEqual(len(fake.calls), 1)
        self.assert_trees_removed()

    def test_new_file_miss_is_new(self):
        head = self.repo.commit(
            {"src/fresh.rs": "pub fn fresh(x: u32) -> bool { x > 3 }\n"}, "a new file")
        md = os.path.join(self._tmp.name, "summary.md")
        code, out, err, fake = self.check(
            [Run(code=2, missed=[FRESH], caught=[L0])], head, "--summary-md", md)
        self.assertEqual(code, 1, err)
        self.assertEqual(len(fake.calls), 1)  # no base run: the file is new
        self.assertEqual(sections(out), {"NEW": [FRESH]})
        self.assertIn(self.out, out)
        with open(md) as f:
            self.assertEqual(
                f.read(),
                "### cargo-mutants diff check\n\n```text\n"
                "cargo-mutants: 1 caught, 1 missed, 0 unviable, 0 timed out\n"
                "NEW misses (catch each with a test, or waive it with a reason):\n"
                f"  {FRESH}\n"
                "FAIL: 1 new miss\n```\n")
        self.assert_trees_removed()

    def test_base_list_empty_skips_base_run(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check([Run(code=2, missed=[L0, L1]), Run(listed=[])], head)
        self.assertEqual(code, 1, err)
        self.assertEqual(len(fake.calls), 2)  # head run, base --list; no base run
        self.assertIn("--list", fake.calls[1])
        self.assertEqual(sections(out), {"NEW": [L0, L1]})

    def test_base_call_args(self):
        head = self.repo.commit(
            {"src/lib.rs": LIB_TOUCHED,
             "src/fresh.rs": "pub fn fresh(x: u32) -> bool { x > 3 }\n"}, "touch and add")
        base_l0 = "src/lib.rs:2:5: replace legacy -> u32 with 0"
        base_l1 = "src/lib.rs:2:5: replace legacy -> u32 with 1"
        code, out, err, fake = self.check(
            [Run(code=2, missed=[L0, FRESH, L1]),
             Run(listed=[base_l0, base_l1]),
             Run(code=2, missed=[base_l0], caught=[base_l1])],
            head, "--shard", "1/3")
        self.assertEqual(code, 1, err)
        head_call, list_call, base_call = fake.calls
        out_dir = os.path.abspath(self.out)
        # The head run: the PR's diff on the head snapshot, sharded.
        self.assertEqual(flag_values(head_call, "-d"), [os.path.join(out_dir, "head-tree")])
        self.assertEqual(flag_values(head_call, "--in-diff"), [os.path.join(out_dir, "head.diff")])
        self.assertEqual(flag_values(head_call, "--output"), [os.path.join(out_dir, "head")])
        self.assertEqual(flag_values(head_call, "-j"), ["1"])
        self.assertEqual(flag_values(head_call, "--shard"), ["1/3"])
        self.assertEqual(flag_values(head_call, "--sharding"), ["round-robin"])
        # The base --list: the base's corresponding lines on the base snapshot,
        # one --re per miss whose file exists on the base; never sharded.
        self.assertEqual(flag_values(list_call, "-d"), [os.path.join(out_dir, "base-tree")])
        self.assertEqual(flag_values(list_call, "--in-diff"), [os.path.join(out_dir, "base.diff")])
        self.assertEqual(flag_values(list_call, "--re"), [  # name_regex(L0), name_regex(L1)
            r"^src/lib\.rs:[0-9]+:[0-9]+: replace legacy \-> u32 with 0$",
            r"^src/lib\.rs:[0-9]+:[0-9]+: replace legacy \-> u32 with 1$"])
        self.assertNotIn("--shard", list_call)
        # The base run: the same selection, run.
        self.assertEqual([a for a in base_call if a not in ("-j", "1", "--output",
                                                             os.path.join(out_dir, "base"))],
                         [a for a in list_call if a != "--list"])
        self.assertEqual(flag_values(base_call, "--output"), [os.path.join(out_dir, "base")])
        self.assertEqual(flag_values(base_call, "-j"), ["1"])
        # The head inserted line 2 (`// touched`), so the head run marks it; the
        # base marks the lines either side of the gap, its 1 and 2, which are
        # the head's 1 and 3. Each side gets the union, line for line.
        with open(os.path.join(out_dir, "head.diff"), "rb") as f:
            self.assertEqual(f.read(), (
                b"diff --git a/src/fresh.rs b/src/fresh.rs\n"
                b"--- a/src/fresh.rs\n+++ b/src/fresh.rs\n"
                b"@@ -0,0 +1,1 @@\n+pub fn fresh(x: u32) -> bool { x > 3 }\n"
                b"diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n"
                b"@@ -0,0 +1,1 @@\n+pub fn legacy(x: u32) -> u32 {\n"
                b"@@ -1,0 +2,1 @@\n+    // touched\n"
                b"@@ -2,0 +3,1 @@\n+    x + 1\n"))
        with open(os.path.join(out_dir, "base.diff"), "rb") as f:
            self.assertEqual(f.read(), (
                b"diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n"
                b"@@ -0,0 +1,1 @@\n+pub fn legacy(x: u32) -> u32 {\n"
                b"@@ -1,0 +2,1 @@\n+    x + 1\n"))
        # Each tree existed while cargo-mutants ran on it, and is gone afterwards.
        self.assertEqual(fake.tree_existed, [True, True, True])
        self.assert_trees_removed()
        self.assertEqual(sections(out), {"NEW": [FRESH, L1], "Old": [L0]})


    def test_no_mutants_in_changed_lines_passes(self):
        # cargo-mutants exits 0 without writing mutants.out when the changed
        # lines hold no mutant (a comment, test code).
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check([Run(code=0, write=False)], head)
        self.assertEqual(code, 0, err)
        self.assertIn("cargo-mutants found no mutants in the changed lines", out)
        self.assertIn("cargo-mutants: 0 caught, 0 missed, 0 unviable, 0 timed out\n"
                      "PASS: no new misses", out)
        self.assertEqual(len(fake.calls), 1)

    def test_no_lists_after_nonzero_exit_exits_2(self):
        # mutants.sh exits 2 itself when sweep_target.py refuses (set -e): the
        # same code as "some missed", but nothing ran.
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check([Run(code=2, write=False)], head)
        self.assertEqual(code, 2)
        lists_dir = os.path.join(os.path.abspath(self.out), "head", "mutants.out")
        self.assertIn(f"cargo-mutants exited 2 on the head run but wrote no {lists_dir}", err)
        self.assertNotIn("internal error", err)

    def test_no_lists_from_base_run_exits_2(self):
        # The base --list named mutants, so its run must write lists.
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check(
            [Run(code=2, missed=[L0]), Run(listed=[L0]), Run(code=0, write=False)], head)
        self.assertEqual(code, 2)
        lists_dir = os.path.join(os.path.abspath(self.out), "base", "mutants.out")
        self.assertIn(f"cargo-mutants exited 0 on the base run but wrote no {lists_dir}", err)

    def test_exit_code_contradicting_lists_exits_2(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        cases = [
            ("exit 2, nothing missed", Run(code=2, caught=[L0]), "exited 2"),
            ("exit 3, nothing timed out", Run(code=3, missed=[L0]), "exited 3"),
            ("exit 0, a miss", Run(code=0, missed=[L0]), "exited 0"),
        ]
        for i, (label, run, says) in enumerate(cases):
            with self.subTest(label):
                code, out, err, fake = self.check(
                    [run], head, out=os.path.join(self._tmp.name, f"out-{i}"))
                self.assertEqual(code, 2, out + err)
                self.assertIn(f"cargo-mutants {says} on the head run, but its lists", err)
                self.assertEqual(len(fake.calls), 1)

    def test_diff_ignores_user_prefix_config(self):
        # cargo-mutants strips only a literal b/: with other prefixes it would
        # match no file, select no mutant, and the check would pass.
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        configs = [("diff.noprefix", "true"), ("diff.mnemonicPrefix", "true"),
                   ("diff.srcPrefix", "x/"), ("diff.dstPrefix", "y/")]
        for i, (name, value) in enumerate(configs):
            with self.subTest(name):
                self.repo.git("config", name, value)
                out_dir = os.path.join(self._tmp.name, f"out-{i}")
                try:
                    code, out, err, fake = self.check([Run()], head, out=out_dir)
                finally:
                    self.repo.git("config", "--unset", name)
                self.assertEqual(code, 0, err)
                for name in ("change.diff", "head.diff", "base.diff"):
                    with open(os.path.join(out_dir, name), "rb") as f:
                        diff = f.read()
                    self.assertTrue(
                        diff.startswith(b"diff --git a/src/lib.rs b/src/lib.rs\n"), diff)
                    self.assertIn(b"\n--- a/src/lib.rs\n+++ b/src/lib.rs\n", diff)

    def test_diff_is_written_byte_for_byte(self):
        crlf = "pub fn crlf() -> u32 {\r\n    // café\r\n    1\r\n}\r\n".encode()
        head = self.repo.commit({"src/crlf.rs": crlf}, "a CRLF file")
        code, out, err, fake = self.check([Run()], head)
        self.assertEqual(code, 0, err)
        expected = subprocess.run(
            ["git", "-C", self.repo.path, "diff", "--no-ext-diff", "--no-color",
             "--src-prefix=a/", "--dst-prefix=b/", self.base, head, "--", "*.rs"],
            check=True, capture_output=True).stdout
        self.assertIn(b"\r\n", expected)
        with open(os.path.join(self.out, "change.diff"), "rb") as f:
            self.assertEqual(f.read(), expected)
        # The head run's diff carries the tree's lines with their CR too.
        with open(os.path.join(self.out, "head.diff"), "rb") as f:
            self.assertIn("+    // café\r\n".encode(), f.read())


class ToolsTests(unittest.TestCase):
    """The real Tools' database guard and mutants.sh call, against fake programs on PATH."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.bin = os.path.join(self._tmp.name, "bin")
        os.makedirs(self.bin)
        self.log = os.path.join(self._tmp.name, "log")
        env = dict(os.environ)
        self.addCleanup(lambda: (os.environ.clear(), os.environ.update(env)))
        os.environ.pop("DATABASE_URL", None)
        os.environ.pop("CRACK_MUTANTS_NO_SWEEP", None)

    def fake(self, name, code=0, body=""):
        path = os.path.join(self.bin, name)
        with open(path, "w") as f:
            f.write(f'#!/bin/sh\n{body}echo "{name} $*" >> "{self.log}"\nexit {code}\n')
        os.chmod(path, 0o755)
        return path

    def logged(self):
        with open(self.log) as f:
            return f.read().splitlines()

    def ready(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            ready = mc.Tools(self._tmp.name).database_ready()
        return ready, out.getvalue()

    def test_database_ready_falls_back_to_the_tests_default_url(self):
        # Locally DATABASE_URL is unset; the tests then use the ctor's default.
        self.fake("pg_isready", code=1)
        os.environ["PATH"] = self.bin + os.pathsep + os.environ["PATH"]
        ready, out = self.ready()
        self.assertFalse(ready)
        self.assertEqual(self.logged(), [
            "pg_isready --dbname=postgresql://postgres:mysecretpassword@localhost:5432/postgres"])

    def test_database_ready_uses_database_url(self):
        self.fake("pg_isready", code=0)
        os.environ["PATH"] = self.bin + os.pathsep + os.environ["PATH"]
        os.environ["DATABASE_URL"] = "postgresql://u:p@db:5433/x"
        ready, out = self.ready()
        self.assertTrue(ready)
        self.assertEqual(self.logged(), ["pg_isready --dbname=postgresql://u:p@db:5433/x"])

    def test_no_pg_isready_skips_the_guard_and_says_so(self):
        os.environ["PATH"] = self.bin  # nothing in it
        ready, out = self.ready()
        self.assertTrue(ready)
        self.assertEqual(out, "mutants_check: no pg_isready, so the Postgres guard is skipped\n")

    def test_mutants_sh_is_told_not_to_sweep(self):
        script = self.fake("mutants.sh", body=f'echo "NO_SWEEP=$CRACK_MUTANTS_NO_SWEEP" >> "{self.log}"\n')
        real = mc.MUTANTS_SH
        mc.MUTANTS_SH = script
        self.addCleanup(setattr, mc, "MUTANTS_SH", real)
        code, _ = mc.Tools(self._tmp.name).mutants(["--list"], capture=True)
        self.assertEqual(code, 0)
        self.assertEqual(self.logged(), ["NO_SWEEP=1", "mutants.sh --list"])


class MutantsShTests(unittest.TestCase):
    """scripts/mutants.sh sweeps unless CRACK_MUTANTS_NO_SWEEP is set."""

    def run_sh(self, **env):
        with tempfile.TemporaryDirectory() as tmp:
            log = os.path.join(tmp, "log")
            bin_dir = os.path.join(tmp, "bin")
            os.makedirs(bin_dir)
            # A fake cargo: `metadata` names a target dir that does not exist
            # yet (nothing to sweep there); anything else is logged.
            with open(os.path.join(bin_dir, "cargo"), "w") as f:
                f.write(f'#!/bin/sh\necho "cargo $1" >> "{log}"\n'
                        f'[ "$1" = mutants ] && echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR" >> "{log}"\n'
                        f'[ "$1" = metadata ] && echo \'{{"target_directory": "{tmp}/target"}}\'\n'
                        'exit 0\n')
            os.chmod(os.path.join(bin_dir, "cargo"), 0o755)
            full_env = {k: v for k, v in os.environ.items() if k not in (
                "CRACK_MUTANTS_NO_SWEEP", "CRACK_MUTANTS_SHARED_TARGET", "CARGO_TARGET_DIR")}
            full_env.update(PATH=bin_dir + os.pathsep + os.environ["PATH"],
                            CRACK_MUTANTS_SCRATCH=os.path.join(tmp, "scratch"), **env)
            proc = subprocess.run([mc.MUTANTS_SH, "--list"], env=full_env,
                                  capture_output=True, text=True)
            if not os.path.exists(log):
                return proc.returncode, []
            with open(log) as f:
                return proc.returncode, f.read().splitlines()

    def test_sweeps_by_default(self):
        self.assertEqual(self.run_sh(),
                         (0, ["cargo metadata", "cargo mutants", "CARGO_TARGET_DIR=target"]))

    def test_no_sweep_when_told(self):
        self.assertEqual(self.run_sh(CRACK_MUTANTS_NO_SWEEP="1"),
                         (0, ["cargo mutants", "CARGO_TARGET_DIR=target"]))

    def test_each_copy_builds_into_its_own_target_by_default(self):
        # A preset CARGO_TARGET_DIR is overridden: locally, parallel jobs
        # must never share one.
        code, log = self.run_sh(CRACK_MUTANTS_NO_SWEEP="1", CARGO_TARGET_DIR="/somewhere/target")
        self.assertEqual((code, log), (0, ["cargo mutants", "CARGO_TARGET_DIR=target"]))

    def test_shared_target_keeps_an_absolute_target_dir(self):
        code, log = self.run_sh(CRACK_MUTANTS_NO_SWEEP="1", CRACK_MUTANTS_SHARED_TARGET="1",
                                CARGO_TARGET_DIR="/ci/workspace/target")
        self.assertEqual((code, log), (0, ["cargo mutants", "CARGO_TARGET_DIR=/ci/workspace/target"]))

    def test_shared_target_refuses_a_relative_or_missing_target_dir(self):
        for target in ("target", ""):
            with self.subTest(target=target):
                env = {"CRACK_MUTANTS_NO_SWEEP": "1", "CRACK_MUTANTS_SHARED_TARGET": "1"}
                if target:
                    env["CARGO_TARGET_DIR"] = target
                code, log = self.run_sh(**env)
                self.assertEqual((code, log), (2, []))


# --- The corresponding selection ---------------------------------------------


def hunk(old_start, new_start, kinds):
    """A Hunk whose lengths are counted from kinds (' ', '-', '+')."""
    return mc.Hunk(old_start, kinds.count(" ") + kinds.count("-"),
                   new_start, kinds.count(" ") + kinds.count("+"), kinds)


class AffectedLinesTests(unittest.TestCase):
    """The port of cargo-mutants 27.1.0's in_diff.rs affected_lines: an
    inserted line marks itself; a deleted line marks the line before it and
    the line after it, in the new file."""

    def test_insert(self):
        # Lines 1-2 kept, one inserted as line 3, three kept.
        self.assertEqual(mc.affected_lines([hunk(1, 1, "  +   ")]), [3])

    def test_insert_run(self):
        self.assertEqual(mc.affected_lines([hunk(1, 1, "  ++ ")]), [3, 4])

    def test_delete(self):
        # Old line 3 deleted: new lines 2 (before) and 3 (after) are marked.
        self.assertEqual(mc.affected_lines([hunk(1, 1, "  -  ")]), [2, 3])

    def test_delete_at_line_1(self):
        # Nothing comes before line 1, so only the line after is marked.
        self.assertEqual(mc.affected_lines([hunk(1, 1, "-   ")]), [1])

    def test_delete_at_end_of_file(self):
        # Nothing comes after, so only the line before is marked.
        self.assertEqual(mc.affected_lines([hunk(1, 1, "  -")]), [2])

    def test_modify(self):
        # A modified line 2 is a deletion then an insertion: line 1 and line 2.
        self.assertEqual(mc.affected_lines([hunk(1, 1, " -+ ")]), [1, 2])

    def test_mixed_hunk(self):
        # " -+ + -  ": modify line 2, insert line 4, delete old line 5.
        #   ' ' new 1; '-' marks 1; '+' marks 2; ' ' 3; '+' marks 4; ' ' 5;
        #   '-' marks 5 (before); ' ' marks 6 (after); ' ' 7.
        self.assertEqual(mc.affected_lines([hunk(1, 1, " -+ + -  ")]), [1, 2, 4, 5, 6])

    def test_multiple_hunks(self):
        self.assertEqual(
            mc.affected_lines([hunk(1, 1, "  +   "), hunk(20, 21, "   -   ")]),
            [3, 23, 24])

    def test_whole_file_added_and_deleted(self):
        self.assertEqual(mc.affected_lines([hunk(0, 1, "+++")]), [1, 2, 3])
        # A deleted file's new side starts at 0: nothing in it is marked.
        self.assertEqual(mc.affected_lines([mc.Hunk(1, 3, 0, 0, "---")]), [])


class ReverseAndMapTests(unittest.TestCase):
    def test_reverse_swaps_sides_and_puts_deletions_first(self):
        # git prints a change's deletions before its insertions, in either
        # direction; the reversed modify must too.
        self.assertEqual(mc.reverse_hunk(hunk(3, 5, " -+ ++ ")),
                         mc.Hunk(5, 6, 3, 4, " -+ -- "))

    def test_map_lines_insert(self):
        h = [hunk(1, 1, "  +   ")]  # new line 3 is inserted
        self.assertEqual(mc.map_lines({1, 2, 3, 4, 5, 9}, h), {1, 2, 4, 5, 6, 10})

    def test_map_lines_delete(self):
        h = [hunk(1, 1, "  -  ")]  # old line 3 is deleted
        self.assertEqual(mc.map_lines({2, 3, 4, 9}, h), {2, 3, 8})

    def test_map_lines_zero_context(self):
        # git -U0: `@@ -5,0 +6,1 @@` inserts after old line 5.
        h = [mc.Hunk(5, 0, 6, 1, "+")]
        self.assertEqual(mc.map_lines({5, 6}, h), {5, 7})
        # `@@ -6,2 +5,0 @@` deletes old lines 6-7.
        h = [mc.Hunk(6, 2, 5, 0, "--")]
        self.assertEqual(mc.map_lines({5, 6, 7, 8}, h), {5, 6})

    def test_map_lines_two_hunks(self):
        h = [hunk(1, 1, "  +   "), hunk(20, 21, "   -   ")]
        # Old 10 is between the hunks (+1); old 23 is deleted; old 30 is after (+0).
        self.assertEqual(mc.map_lines({10, 23, 30}, h), {11, 30})


PICK = "fn pick(a: u32) -> u32 {\n    let x = if a > 1 { 10 } else { 20 };\n    let z = x + 1;\n    z\n}\n"

# A change that git's diff algorithms align differently (Bram Cohen's
# patience example, in Rust): myers and histogram disagree on it.
FROB_OLD = """use std::io;

// Frobs foo heartily
fn frobnitz(foo: u32) {
    for i in 0..10 {
        println!("Your answer is: ");
        println!("{}", foo);
    }
}

fn fact(n: u32) -> u32 {
    if n > 1 {
        return fact(n - 1) * n;
    }
    1
}

fn main() {
    frobnitz(fact(10));
}
"""
FROB_NEW = """use std::io;

fn fib(n: u32) -> u32 {
    if n > 2 {
        return fib(n - 1) + fib(n - 2);
    }
    1
}

// Frobs foo heartily
fn frobnitz(foo: u32) {
    for i in 0..10 {
        println!("{}", foo);
    }
}

fn main() {
    frobnitz(fib(10));
}
"""


class SelectionTests(unittest.TestCase):
    """corresponding_selection on real `git diff` output."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.repo = GitFixture(os.path.join(self._tmp.name, "repo"))
        self.tools = mc.Tools(self.repo.path)

    def select(self, base_files, head_files):
        base = self.repo.commit(base_files, "base")
        head = self.repo.commit(head_files, "head")
        patches = mc.parse_diff(mc.rust_diff(self.tools, base, head))
        return mc.corresponding_selection(patches)

    def test_pure_insertion(self):
        head = PICK.replace("    let z", "    let y = if a > 2 { 1 } else { 2 };\n    let z")
        h, b = self.select({"f.rs": PICK}, {"f.rs": head})
        # The head marks its inserted line 3; the base marks the lines around
        # the gap (2 and 3), which are the head's 2 and 4.
        self.assertEqual(h, {"f.rs": [2, 3, 4]})
        self.assertEqual(b, {"f.rs": [2, 3]})

    def test_pure_deletion(self):
        h, b = self.select({"f.rs": PICK}, {"f.rs": PICK.replace("    let z = x + 1;\n", "")})
        # The head marks the lines around the gap (2 and 3); the base marks
        # its deleted line 3, plus 2 and 4, which are the head's 2 and 3.
        self.assertEqual(h, {"f.rs": [2, 3]})
        self.assertEqual(b, {"f.rs": [2, 3, 4]})

    def test_modification_is_symmetric(self):
        h, b = self.select({"f.rs": PICK}, {"f.rs": PICK.replace("x + 1", "x + 2")})
        self.assertEqual(h, {"f.rs": [2, 3]})
        self.assertEqual(b, {"f.rs": [2, 3]})

    def test_added_and_deleted_files(self):
        h, b = self.select({"gone.rs": "fn a() {}\nfn b() {}\n", "keep.rs": "\n"},
                           {"gone.rs": None, "new.rs": "fn c() {}\n"})
        # A file only the head has selects on the head alone, and the other
        # way round; neither is an error.
        self.assertEqual(h, {"new.rs": [1]})
        self.assertEqual(b, {"gone.rs": [1, 2]})

    def test_rename_is_found_whatever_the_config(self):
        body = "".join(f"fn f{i}() -> u32 {{ {i} }}\n" for i in range(20))
        self.repo.git("config", "diff.renames", "false")
        h, b = self.select({"old.rs": body}, {"old.rs": None,
                                              "new.rs": body.replace("{ 7 }", "{ 70 }")})
        # Without --find-renames, new.rs would be a 20-line addition.
        self.assertEqual(h, {"new.rs": [7, 8]})
        self.assertEqual(b, {"old.rs": [7, 8]})

    def test_alignment_is_found_whatever_the_config(self):
        # histogram aligns this change differently from git's default (myers),
        # so without a pinned algorithm a local run and CI would select
        # different lines.
        base = self.repo.commit({"f.rs": FROB_OLD}, "base")
        head = self.repo.commit({"f.rs": FROB_NEW}, "head")
        pinned = mc.rust_diff(self.tools, base, head)
        for name, value in [("diff.algorithm", "histogram"),
                            ("diff.algorithm", "patience"),
                            ("diff.indentHeuristic", "false")]:
            with self.subTest(f"{name}={value}"):
                self.repo.git("config", name, value)
                try:
                    self.assertEqual(mc.rust_diff(self.tools, base, head), pinned)
                finally:
                    self.repo.git("config", "--unset", name)

    def test_parse_diff_rejects_missing_prefixes(self):
        with self.assertRaises(mc.CheckError):
            mc.parse_diff(b"diff --git x.rs x.rs\n--- x.rs\n+++ x.rs\n@@ -1 +1 @@\n-a\n+b\n")

    def test_parse_diff_rejects_a_truncated_hunk(self):
        with self.assertRaises(mc.CheckError):
            mc.parse_diff(b"diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n-a\n")


class SyntheticDiffTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.repo = GitFixture(os.path.join(self._tmp.name, "repo"))
        self.tools = mc.Tools(self.repo.path)

    def test_lines_equal_the_tree_byte_for_byte(self):
        files = {
            "src/a.rs": "fn a() {\n    // café\n    1\n}\n",
            "src/crlf.rs": "fn c() -> u32 {\r\n    2\r\n}\r\n".encode(),
            "src/b c.rs": "fn b() {}\n// no newline at the end",
        }
        rev = self.repo.commit(files, "files")
        selection = {"src/a.rs": [1, 2, 4], "src/crlf.rs": [2], "src/b c.rs": [2]}
        diff = mc.synthetic_diff(selection, lambda p: mc.tree_file(self.tools, rev, p))
        tree = os.path.join(self._tmp.name, "tree")
        self.tools.snapshot(rev, tree)
        patches = mc.parse_diff(diff)
        self.assertEqual([p.new for p in patches], ["src/a.rs", "src/b c.rs", "src/crlf.rs"])
        # Every hunk is a one-line insertion of exactly line L.
        lines_in_diff = {}
        for p in patches:
            self.assertEqual(p.old, p.new)
            for h in p.hunks:
                self.assertEqual((h.old_start, h.old_len, h.new_len, h.kinds),
                                 (h.new_start - 1, 0, 1, "+"))
                lines_in_diff.setdefault(p.new, []).append(h.new_start)
        self.assertEqual(lines_in_diff, selection)
        # The text of each `+` line is the snapshot's line, byte for byte.
        plus = [line[1:] for line in diff.split(b"\n")
                if line.startswith(b"+") and not line.startswith(b"+++ ")]
        expected = []
        for path in sorted(selection):
            with open(os.path.join(tree, path), "rb") as f:
                text = f.read().split(b"\n")
            expected += [text[n - 1] for n in selection[path]]
        self.assertEqual(plus, expected)
        self.assertIn(b"    2\r", plus)
        self.assertIn("    // café".encode(), plus)
        # A path with a space ends its ---/+++ name with a tab, as git's do.
        self.assertIn(b"\n--- a/src/b c.rs\t\n+++ b/src/b c.rs\t\n", diff)

    def test_line_past_the_end_is_an_error(self):
        rev = self.repo.commit({"x.rs": "fn x() {}\n"}, "x")
        with self.assertRaises(mc.CheckError):
            mc.synthetic_diff({"x.rs": [2]}, lambda p: mc.tree_file(self.tools, rev, p))

    def test_empty_selection_is_empty(self):
        self.assertEqual(mc.synthetic_diff({}, lambda p: b""), b"")


# --- Shards and the verdict ---------------------------------------------------


class ShardResultTests(PipelineBase):
    """The shard's side of the CI verdict: --shard-result writes its lists."""

    def test_shard_result_written(self):
        head = self.repo.commit(
            {"src/lib.rs": LIB_TOUCHED,
             "src/fresh.rs": "pub fn fresh(x: u32) -> bool { x > 3 }\n"}, "touch and add")
        base_l0 = "src/lib.rs:2:5: replace legacy -> u32 with 0"
        base_l1 = "src/lib.rs:2:5: replace legacy -> u32 with 1"
        path = os.path.join(self._tmp.name, "results", "shard-1.json")
        code, out, err, fake = self.check(
            [Run(code=2, missed=[L0, FRESH, L1], caught=["src/lib.rs:6:7: replace * with + in covered"]),
             Run(listed=[base_l0, base_l1]),
             Run(code=2, missed=[base_l0], caught=[base_l1])],
            head, "--shard", "1/3", "--shard-result", path)
        self.assertEqual(code, 1, err)  # this shard's own view
        self.assertIn("one shard's view", out)
        with open(path) as f:
            result = json.load(f)
        self.assertEqual(result, {
            "version": 1,
            "shard": "1/3",
            "base": self.base,
            "head": head,
            "head_lines": {"src/fresh.rs": [1], "src/lib.rs": [1, 2, 3]},
            "base_lines": {"src/lib.rs": [1, 2]},
            "missed": [L0, FRESH, L1],
            "caught": ["src/lib.rs:6:7: replace * with + in covered"],
            "unviable": [],
            "timeout": [],
            "base_missed": [base_l0],
        })

    def test_no_shard_result_on_a_broken_run(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        path = os.path.join(self._tmp.name, "shard-0.json")
        code, out, err, fake = self.check([Run(code=4)], head,
                                          "--shard", "0/2", "--shard-result", path)
        self.assertEqual(code, 2)
        self.assertFalse(os.path.exists(path))

    def test_shard_result_needs_a_shard(self):
        head = self.repo.commit({"src/lib.rs": LIB_TOUCHED}, "touch legacy")
        code, out, err, fake = self.check(
            [], head, "--shard-result", os.path.join(self._tmp.name, "r.json"))
        self.assertEqual(code, 2)
        self.assertIn("--shard-result needs --shard", err)


K10 = "f.rs:10:5: replace a > b with true in g"
K12 = "f.rs:12:5: replace a > b with true in g"
K8_BASE = "f.rs:8:5: replace a > b with true in g"
K20_BASE = "f.rs:20:5: replace a > b with true in g"
CAUGHT = "f.rs:30:5: replace h -> u32 with 0"


def shard_result(k, n, missed=(), caught=(), base_missed=(), base_lines=None):
    return {"version": 1, "shard": f"{k}/{n}", "base": "b" * 40, "head": "h" * 40,
            "head_lines": {"f.rs": [10, 11, 12]},
            "base_lines": base_lines or {"f.rs": [8, 9, 10]},
            "missed": list(missed), "caught": list(caught), "unviable": [], "timeout": [],
            "base_missed": list(base_missed)}


class CollectTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = os.path.join(self._tmp.name, "results")
        os.makedirs(self.dir)
        self.md = os.path.join(self._tmp.name, "summary.md")

    def write(self, name, result):
        with open(os.path.join(self.dir, name), "w") as f:
            json.dump(result, f)

    def collect(self, expect="2"):
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = mc.main(["--collect", self.dir, "--expect-shards", expect,
                            "--summary-md", self.md])
        return code, stdout.getvalue(), stderr.getvalue()

    def test_two_shards_one_base_copy_is_one_new(self):
        # Each shard alone saw one head copy of K and one base copy: 0 new
        # each. Together, two head copies against one base copy: 1 new.
        self.write("shard-0.json", shard_result(0, 2, missed=[K10], caught=[CAUGHT],
                                                base_missed=[K8_BASE]))
        self.write("shard-1.json", shard_result(1, 2, missed=[K12], base_missed=[K8_BASE]))
        code, out, err = self.collect()
        self.assertEqual(code, 1, err)
        self.assertEqual(sections(out), {"NEW": [K12], "Old": [K10]})
        text = mc.summary({"caught": 1, "missed": 2, "unviable": 0, "timeout": 0},
                          [K12], [K10], [])
        self.assertIn(text, out)
        with open(self.md) as f:  # ONE summary, the same text
            self.assertEqual(f.read(), f"{mc.MD_HEADING}\n\n```text\n{text}\n```\n")

    def test_base_misses_dedupe_by_full_name(self):
        # Both shards reran K on the base and found the same copy at line 8;
        # shard 0 also found a second copy at 20. Two base copies: both old.
        self.write("shard-0.json", shard_result(0, 2, missed=[K12],
                                                base_missed=[K8_BASE, K20_BASE]))
        self.write("shard-1.json", shard_result(1, 2, missed=[K10], base_missed=[K8_BASE]))
        code, out, err = self.collect()
        self.assertEqual(code, 0, err)
        # Merged in source order, whatever the shard order.
        self.assertEqual(sections(out), {"Old": [K10, K12]})
        self.assertIn("PASS: no new misses", out)

    def test_missing_shard_exits_2(self):
        self.write("shard-0.json", shard_result(0, 2, missed=[K10], base_missed=[K8_BASE]))
        code, out, err = self.collect()
        self.assertEqual(code, 2)
        self.assertIn("missing shard result(s): 1 of 0..1", err)
        with open(self.md) as f:
            self.assertEqual(f.read(), f"{mc.MD_HEADING}\n\n"
                             "broken run: missing shard result(s): 1 of 0..1\n")

    def test_no_results_exits_2(self):
        code, out, err = self.collect()
        self.assertEqual(code, 2)
        self.assertIn("no shard results", err)

    def test_shard_count_mismatch_exits_2(self):
        self.write("shard-0.json", shard_result(0, 3))
        self.write("shard-1.json", shard_result(1, 3))
        code, out, err = self.collect(expect="2")
        self.assertEqual(code, 2)
        self.assertIn("expected shards of 2", err)

    def test_duplicate_shard_exits_2(self):
        self.write("shard-0.json", shard_result(0, 2))
        self.write("shard-0-again.json", shard_result(0, 2))
        self.write("shard-1.json", shard_result(1, 2))
        code, out, err = self.collect()
        self.assertEqual(code, 2)
        self.assertIn("shard 0/2 reported twice", err)

    def test_shards_that_selected_differently_exit_2(self):
        self.write("shard-0.json", shard_result(0, 2))
        self.write("shard-1.json", shard_result(1, 2, base_lines={"f.rs": [8]}))
        code, out, err = self.collect()
        self.assertEqual(code, 2)
        self.assertIn("shards disagree", err)

    def test_garbage_name_exits_2(self):
        self.write("shard-0.json", shard_result(0, 2, missed=["not a mutant name"]))
        self.write("shard-1.json", shard_result(1, 2))
        code, out, err = self.collect()
        self.assertEqual(code, 2)
        self.assertNotIn("internal error", err)

    def test_collect_needs_expect_shards(self):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(mc.main(["--collect", self.dir]), 2)


if __name__ == "__main__":
    unittest.main()
