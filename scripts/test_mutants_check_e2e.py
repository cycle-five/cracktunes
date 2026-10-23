"""End-to-end tests for mutants_check.py: real git, real cargo-mutants, on a
one-crate fixture repo a few KB in size.

They skip where `cargo mutants --version` fails (lint.yml's scripts job has
cargo but not cargo-mutants). Builds go to a fresh scratch directory under
$HOME/.cache/cargo-mutants, never /tmp, and it is removed afterwards."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from test_mutants_check import CARGO_TOML, LIB_RS, GitFixture, sections  # noqa: E402

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "mutants_check.py")
FRESH_FN = "pub fn fresh(x: u32) -> bool { x > 3 }\n"


def have_cargo_mutants() -> bool:
    try:
        return subprocess.run(["cargo", "mutants", "--version"],
                              capture_output=True).returncode == 0
    except OSError:
        return False


def touch(text: str, line: str) -> str:
    """Add a comment to the end of `line`, a function's only statement.

    🪤 Not a comment line of its own: cargo-mutants 27.1 spans a whole-body
    mutant from the first statement to the end of the last (visit.rs:757), so a
    comment line above a function's only statement selects nothing."""
    assert text.count(line) == 1, line
    return text.replace(line, line.rstrip("\n") + " // touched\n")


class E2EBase(unittest.TestCase):
    """A fixture repo whose base commit is LIB_RS, and `check` to run the script on it."""

    @classmethod
    def setUpClass(cls):
        cls.scratch = os.path.join(os.path.expanduser("~"), ".cache", "cargo-mutants",
                                   f"e2e-{os.getpid()}-{cls.__name__}")
        os.makedirs(cls.scratch)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.scratch)

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.repo = GitFixture(os.path.join(self._tmp.name, "repo"))
        self.base = self.repo.commit({"Cargo.toml": CARGO_TOML, "src/lib.rs": LIB_RS}, "base")

    def check(self, lib_rs: str):
        head = self.repo.commit({"src/lib.rs": lib_rs}, "head")
        proc = subprocess.run(
            [sys.executable, SCRIPT, "--repo", self.repo.path, "--base", self.base, "--head", head],
            capture_output=True, text=True,
            env={**os.environ, "CRACK_MUTANTS_SCRATCH": self.scratch})
        return proc.returncode, proc.stdout, proc.stdout + proc.stderr


@unittest.skipUnless(have_cargo_mutants(), "cargo-mutants is not installed")
class EndToEndTests(E2EBase):
    def test_touching_legacy_reports_old(self):
        code, out, log = self.check(touch(LIB_RS, "    x + 1\n"))
        self.assertEqual(code, 0, log)
        self.assertEqual(sections(out), {"Old": [
            "src/lib.rs:2:5: replace legacy -> u32 with 0",
            "src/lib.rs:2:5: replace legacy -> u32 with 1",
            "src/lib.rs:2:7: replace + with - in legacy",
            "src/lib.rs:2:7: replace + with * in legacy"]}, log)
        self.assertIn("PASS: no new misses", out)

    def test_untested_new_function_is_new(self):
        lib = touch(LIB_RS, "    x + 1\n").replace("#[cfg(test)]", FRESH_FN + "\n#[cfg(test)]")
        code, out, log = self.check(lib)
        self.assertEqual(code, 1, log)
        self.assertEqual(sections(out), {
            "NEW": ["src/lib.rs:9:32: replace fresh -> bool with true",
                    "src/lib.rs:9:32: replace fresh -> bool with false",
                    "src/lib.rs:9:34: replace > with == in fresh",
                    "src/lib.rs:9:34: replace > with < in fresh",
                    "src/lib.rs:9:34: replace > with >= in fresh"],
            "Old": ["src/lib.rs:2:5: replace legacy -> u32 with 0",
                    "src/lib.rs:2:5: replace legacy -> u32 with 1",
                    "src/lib.rs:2:7: replace + with - in legacy",
                    "src/lib.rs:2:7: replace + with * in legacy"]}, log)
        self.assertIn("FAIL: 5 new misses", out)

    def test_regex_selects_twin_on_base(self):
        # Every line moves, so the base's twins sit at other line numbers.
        code, out, log = self.check("\n" * 5 + touch(LIB_RS, "    x + 1\n"))
        self.assertEqual(code, 0, log)
        self.assertEqual(sections(out), {"Old": [
            "src/lib.rs:7:5: replace legacy -> u32 with 0",
            "src/lib.rs:7:5: replace legacy -> u32 with 1",
            "src/lib.rs:7:7: replace + with - in legacy",
            "src/lib.rs:7:7: replace + with * in legacy"]}, log)
        self.assertIn("PASS: no new misses", out)

    def test_deleting_the_test_makes_new(self):
        test_fn = ("    #[test]\n    fn covered_doubles() {\n"
                   "        assert_eq!(super::covered(3), 6);\n    }\n")
        self.assertIn(test_fn, LIB_RS)
        code, out, log = self.check(touch(LIB_RS, "    x * 2\n").replace(test_fn, ""))
        self.assertEqual(code, 1, log)
        # Caught on the base, so new.
        self.assertEqual(sections(out), {"NEW": [
            "src/lib.rs:6:5: replace covered -> u32 with 0",
            "src/lib.rs:6:5: replace covered -> u32 with 1",
            "src/lib.rs:6:7: replace * with + in covered",
            "src/lib.rs:6:7: replace * with / in covered"]}, log)
        self.assertIn("FAIL: 4 new misses", out)


# 🔑 The two tests below pin the corresponding selection (spec section 4, "New
# or old"). cargo-mutants marks an inserted line alone, but a deleted line's
# neighbours on both sides. A base run limited by the REVERSED diff therefore
# looked at other lines than the head run did: an untested line inserted next
# to an untested twin passed as old, and a deletion next to an old miss failed.
PICK_RS = """\
pub fn pick(a: u32, b: u32) -> u32 {
    let x = if a > 1 { 10 } else { 20 };
    let z = x + b;
    z
}
"""
PICK_DELETABLE_RS = """\
pub fn pick(a: u32) -> u32 {
    let x = if a > 1 { 10 } else { 20 };
    let _unused = a;
    x
}
"""


@unittest.skipUnless(have_cargo_mutants(), "cargo-mutants is not installed")
class CorrespondingSelectionTests(E2EBase):
    def rebase(self, lib_rs: str):
        """Make a base whose lib.rs is lib_rs: the e2e base plus an untested `pick`."""
        self.base = self.repo.commit({"src/lib.rs": lib_rs}, "base with pick")

    def test_inserted_twin_of_an_old_miss_is_new(self):
        # The head inserts an untested `b > 1` next to the untested `a > 1`.
        # Each `>` mutant now has two copies on the head and one on the base.
        self.rebase(PICK_RS + "\n" + LIB_RS)
        inserted = "    let _y = if b > 1 { 30 } else { 40 };\n"
        head = PICK_RS.replace("    let z = x + b;\n", inserted + "    let z = x + b;\n")
        code, out, log = self.check(head + "\n" + LIB_RS)
        self.assertEqual(code, 1, log)
        found = sections(out)
        self.assertEqual(found.get("NEW"), [
            "src/lib.rs:3:19: replace > with == in pick",
            "src/lib.rs:3:19: replace > with < in pick",
            "src/lib.rs:3:19: replace > with >= in pick"], log)
        self.assertIn("FAIL: 3 new misses", out)

    def test_deletion_beside_an_old_miss_keeps_it_old(self):
        # The head deletes a statement below the untested `a > 1`. The head
        # run marks that line (it neighbours the deletion), so the base run
        # must look at it too.
        self.rebase(PICK_DELETABLE_RS + "\n" + LIB_RS)
        head = PICK_DELETABLE_RS.replace("    let _unused = a;\n", "")
        code, out, log = self.check(head + "\n" + LIB_RS)
        self.assertEqual(code, 0, log)
        found = sections(out)
        self.assertNotIn("NEW", found, log)
        self.assertIn("src/lib.rs:2:18: replace > with == in pick", found.get("Old", []), log)
        self.assertIn("PASS: no new misses", out)


if __name__ == "__main__":
    unittest.main()
