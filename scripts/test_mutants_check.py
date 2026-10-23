"""Tests for mutants_check.py. Fixture lines come from
`cargo mutants --list -f crack-core/src/economy/earn.rs`.

The pipeline tests run real git on a throwaway repo and fake only
cargo-mutants (and the database check), so they need no cargo."""
import contextlib
import io
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

    def __init__(self, repo, runs):
        super().__init__(repo)
        self.runs = list(runs)
        self.calls = []
        self.tree_existed = []

    def database_ready(self):
        return True

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


class PipelineTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        root = self._tmp.name
        self.repo = GitFixture(os.path.join(root, "repo"))
        self.base = self.repo.commit(
            {"Cargo.toml": CARGO_TOML, "src/lib.rs": LIB_RS, "README.md": "fixture\n"}, "base")
        self.out = os.path.join(root, "out")

    def check(self, runs, head, *extra, out=None):
        fake = FakeTools(self.repo.path, runs)
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = mc.main(["--repo", self.repo.path, "--base", self.base, "--head", head,
                            "--output", out or self.out, *extra],
                           make_tools=lambda repo: fake)
        return code, stdout.getvalue(), stderr.getvalue(), fake

    def assert_trees_removed(self):
        self.assertFalse(os.path.exists(os.path.join(self.out, "head-tree")))
        self.assertFalse(os.path.exists(os.path.join(self.out, "base-tree")))

    def test_no_rust_changes(self):
        head = self.repo.commit({"README.md": "fixture, edited\n"}, "docs only")
        code, out, err, fake = self.check([], head)
        self.assertEqual(code, 0, err)
        self.assertIn(f"no Rust changes between {self.base[:12]} and {head[:12]}", out)
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
        # The base --list: the reversed diff on the base snapshot, one --re per miss
        # whose file exists on the base; never sharded.
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
        with open(os.path.join(out_dir, "base.diff")) as f:
            self.assertIn("-    // touched", f.read())
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
                with open(os.path.join(out_dir, "head.diff"), "rb") as f:
                    diff = f.read()
                self.assertTrue(diff.startswith(b"diff --git a/src/lib.rs b/src/lib.rs\n"), diff)
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
        with open(os.path.join(self.out, "head.diff"), "rb") as f:
            self.assertEqual(f.read(), expected)


if __name__ == "__main__":
    unittest.main()
