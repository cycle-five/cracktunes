"""Tests for mutants_baseline.py. Fixture lines come from
`cargo mutants --list -f crack-core/src/economy/earn.rs`."""
import contextlib
import io
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mutants_baseline as mb  # noqa: E402

A = "crack-core/src/economy/earn.rs:48:5: replace voice_earners -> Vec<UserId> with vec![]"
B = "crack-core/src/economy/earn.rs:49:42: delete ! in voice_earners"
C = "crack-core/src/economy/earn.rs:50:50: replace += with -= in voice_earners"
AND1 = "crack-core/src/economy/earn.rs:61:17: replace && with || in voice_earners"
AND2 = "crack-core/src/economy/earn.rs:60:17: replace && with || in voice_earners"
KA = "crack-core/src/economy/earn.rs: replace voice_earners -> Vec<UserId> with vec![]"
KB = "crack-core/src/economy/earn.rs: delete ! in voice_earners"
KC = "crack-core/src/economy/earn.rs: replace += with -= in voice_earners"


class Base(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.dir = self._tmp.name

    def write(self, name, lines):
        p = os.path.join(self.dir, name)
        with open(p, "w") as f:
            f.write("".join(l + "\n" for l in lines))
        return p

    def run_cli(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            code = mb.main(list(argv))
        return code, out.getvalue()


class KeyTests(unittest.TestCase):
    def test_key_strips_line_and_column(self):
        self.assertEqual(mb.key(A), KA)

    def test_key_rejects_malformed(self):
        for bad in ["", "no position here", "file.rs:1: missing column", "file.rs:a:b: x"]:
            with self.assertRaises(ValueError):
                mb.key(bad)


class CompareTests(Base):
    def test_shifted_lines_are_not_new(self):
        base = self.write("base.txt", [mb.key(A), mb.key(B)])
        shifted = self.write("missed.txt", [
            A.replace(":48:5:", ":90:9:"), B.replace(":49:42:", ":91:3:")])
        code, _ = self.run_cli("compare", "--baseline", base, "--missed", shifted)
        self.assertEqual(code, 0)

    def test_duplicates_count_twice(self):
        base = self.write("base.txt", [mb.key(AND1)])
        missed = self.write("missed.txt", [AND1, AND2])
        code, out = self.run_cli("compare", "--baseline", base, "--missed", missed)
        self.assertEqual(code, 1)
        self.assertIn("1 new", out)

    def test_new_miss_fails_compare(self):
        base = self.write("base.txt", [mb.key(A)])
        missed = self.write("missed.txt", [A, B])
        code, out = self.run_cli("compare", "--baseline", base, "--missed", missed)
        self.assertEqual(code, 1)
        self.assertIn(B, out)

    def test_missing_baseline_means_all_new(self):
        missed = self.write("missed.txt", [A, B])
        code, out = self.run_cli(
            "compare", "--baseline", os.path.join(self.dir, "nope.txt"), "--missed", missed)
        self.assertEqual(code, 1)
        self.assertIn("2 new", out)

    def test_comments_and_blanks_ignored(self):
        base = self.write("base.txt", ["# header", "", mb.key(A), "   ", "# more"])
        self.assertEqual(sum(mb.load_baseline(base).values()), 1)
        missed = self.write("missed.txt", ["# note", "", A])
        code, _ = self.run_cli("compare", "--baseline", base, "--missed", missed)
        self.assertEqual(code, 0)


class BadInputTests(Base):
    def test_malformed_missed_line_exits_2(self):
        base = self.write("base.txt", [mb.key(A)])
        missed = self.write("missed.txt", [A, "garbage without position"])
        for cmd in ("compare", "refresh"):
            code, out = self.run_cli(cmd, "--baseline", base, "--missed", missed)
            self.assertEqual(code, 2, cmd)
            self.assertIn("garbage without position", out)
            self.assertIn(missed, out)

    def test_missing_missed_file_exits_2(self):
        base = self.write("base.txt", [mb.key(A)])
        gone = os.path.join(self.dir, "gone.txt")
        for cmd in ("compare", "refresh"):
            code, out = self.run_cli(cmd, "--baseline", base, "--missed", gone)
            self.assertEqual(code, 2, cmd)
            self.assertIn("gone.txt", out)


class RefreshTests(Base):
    def test_refresh_shrinks(self):
        base = self.write("base.txt", ["# header", mb.key(C), mb.key(A), mb.key(B)])
        missed = self.write("missed.txt", [C, A])
        code, out = self.run_cli("refresh", "--baseline", base, "--missed", missed)
        self.assertEqual(code, 0)
        self.assertIn("1", out)
        with open(base) as f:
            text = f.read()
        self.assertEqual(text.splitlines(), ["# header", *sorted([KA, KC])])

    def test_refresh_refuses_growth(self):
        base = self.write("base.txt", [mb.key(A)])
        before = open(base).read()
        missed = self.write("missed.txt", [A, B])
        code, _ = self.run_cli("refresh", "--baseline", base, "--missed", missed)
        self.assertEqual(code, 1)
        self.assertEqual(open(base).read(), before)

    def test_init_writes_sorted_keys(self):
        base = os.path.join(self.dir, "new.txt")
        missed = self.write("missed.txt", [C, A, B])
        code, _ = self.run_cli("refresh", "--baseline", base, "--missed", missed, "--init")
        self.assertEqual(code, 0)
        keys = [l for l in open(base).read().splitlines() if l and not l.startswith("#")]
        self.assertEqual(keys, sorted([KA, KB, KC]))

    def test_init_refuses_when_baseline_exists(self):
        base = self.write("base.txt", [mb.key(A)])
        before = open(base).read()
        missed = self.write("missed.txt", [A, B])
        code, _ = self.run_cli("refresh", "--baseline", base, "--missed", missed, "--init")
        self.assertNotEqual(code, 0)
        self.assertEqual(open(base).read(), before)


if __name__ == "__main__":
    unittest.main()
