"""Tests for sweep_target.py. Only temporary directories: never the real
shared target dir."""
import contextlib
import io
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sweep_target as st  # noqa: E402

DAY = 86400


def age(path: Path, days: float):
    t = time.time() - days * DAY
    os.utime(path, (t, t))


class SweepBase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        root = Path(self._tmp.name)
        self.target = root / "target"
        self.target.mkdir()
        (self.target / "CACHEDIR.TAG").write_text("Signature: 8a477f597d28d172789f06886806bc55\n")
        self.scratch = root / "scratch"
        self.scratch.mkdir()

    def run_main(self, *extra):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            code = st.main(["--scratch-root", str(self.scratch), *extra], target=self.target)
        return code, out.getvalue()


class SweepTests(SweepBase):
    def test_finds_stale_incremental_children(self):
        old1 = self.target / "debug/incremental/crate-old"
        old2 = self.target / "x86_64-unknown-linux-gnu/debug/incremental/crate-old"
        old3 = self.target / "mutants/incremental/crate-old"
        for p in (old1, old2, old3):
            p.mkdir(parents=True)
            age(p, 30)
        found = st.stale_incremental(self.target, time.time())
        self.assertEqual(set(found), {old1, old2, old3})

    def test_keeps_recent_and_non_incremental(self):
        fresh = self.target / "debug/incremental/crate-new"
        fresh.mkdir(parents=True)
        stale_dep = self.target / "debug/deps/libold.rlib"
        stale_dep.parent.mkdir(parents=True)
        stale_dep.write_text("x")
        age(stale_dep, 60)
        self.assertEqual(st.stale_incremental(self.target, time.time()), [])
        code, _ = self.run_main()
        self.assertEqual(code, 0)
        self.assertTrue(fresh.exists())
        self.assertTrue(stale_dep.exists())

    def test_refuses_a_directory_without_cachedir_tag(self):
        (self.target / "CACHEDIR.TAG").unlink()
        old = self.target / "debug/incremental/crate-old"
        old.mkdir(parents=True)
        age(old, 30)
        code, _ = self.run_main()
        self.assertEqual(code, 2)
        self.assertTrue(old.exists())

    def test_stale_scratch_only_matches_cargo_mutants_prefix(self):
        mine = self.scratch / "cargo-mutants-crack-AbCd.tmp"
        other = self.scratch / "something-else"
        fresh = self.scratch / "cargo-mutants-crack-fresh.tmp"
        for p in (mine, other, fresh):
            p.mkdir()
        age(mine, 3)
        age(other, 3)
        self.assertEqual(st.stale_scratch(self.scratch, time.time()), [mine])
        code, _ = self.run_main()
        self.assertEqual(code, 0)
        self.assertFalse(mine.exists())
        self.assertTrue(other.exists())
        self.assertTrue(fresh.exists())

    def test_dry_run_deletes_nothing(self):
        old = self.target / "debug/incremental/crate-old"
        old.mkdir(parents=True)
        age(old, 30)
        mine = self.scratch / "cargo-mutants-crack-AbCd.tmp"
        mine.mkdir()
        age(mine, 3)
        code, out = self.run_main("--dry-run")
        self.assertEqual(code, 0)
        self.assertTrue(old.exists())
        self.assertTrue(mine.exists())
        self.assertIn(str(old), out)
        self.assertIn(str(mine), out)


if __name__ == "__main__":
    unittest.main()
