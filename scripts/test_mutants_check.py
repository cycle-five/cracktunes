"""Tests for mutants_check.py. Fixture lines come from
`cargo mutants --list -f crack-core/src/economy/earn.rs`."""
import os
import re
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


if __name__ == "__main__":
    unittest.main()
