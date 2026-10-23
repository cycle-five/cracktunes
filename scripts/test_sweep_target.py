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

    def test_a_target_dir_that_does_not_exist_yet_is_fine(self):
        # A fresh CI checkout has no target dir until the first build.
        import shutil
        shutil.rmtree(self.target)
        code, out = self.run_main()
        self.assertEqual(code, 0)
        self.assertIn("does not exist yet", out)

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


class SafetyTests(SweepBase):
    def setUp(self):
        super().setUp()
        self.outside = Path(self._tmp.name) / "outside"
        self.outside.mkdir()

    def test_symlinked_incremental_dir_is_not_followed(self):
        victim = self.outside / "precious"
        victim.write_text("x")
        age(victim, 60)
        (self.target / "debug").mkdir()
        (self.target / "debug/incremental").symlink_to(self.outside)
        self.assertEqual(st.stale_incremental(self.target, time.time()), [])
        self.assertEqual(self.run_main()[0], 0)
        self.assertTrue(victim.exists())

    def test_symlinked_intermediate_dir_is_not_followed(self):
        old = self.outside / "incremental/crate-old"
        old.mkdir(parents=True)
        age(old, 60)
        (self.target / "linked").symlink_to(self.outside)
        self.assertEqual(st.stale_incremental(self.target, time.time()), [])
        self.assertEqual(self.run_main()[0], 0)
        self.assertTrue(old.exists())

    def test_symlinked_stale_child_unlinks_link_not_target(self):
        inc = self.target / "debug/incremental"
        inc.mkdir(parents=True)
        victim = self.outside / "precious"
        victim.write_text("x")
        link = inc / "crate-link"
        link.symlink_to(self.outside)
        t = time.time() - 30 * DAY
        os.utime(link, (t, t), follow_symlinks=False)
        self.assertEqual(self.run_main()[0], 0)
        self.assertFalse(link.is_symlink())
        self.assertTrue(victim.exists())

    def test_incremental_at_depth_four_is_untouched(self):
        deep = self.target / "a/b/c/incremental/crate-old"
        deep.mkdir(parents=True)
        age(deep, 60)
        ok = self.target / "a/b/incremental/crate-old"
        ok.mkdir(parents=True)
        age(ok, 60)
        self.assertEqual(st.stale_incremental(self.target, time.time()), [ok])
        self.assertEqual(self.run_main()[0], 0)
        self.assertTrue(deep.exists())
        self.assertFalse(ok.exists())

    def test_main_deletes_stale_and_keeps_fresh(self):
        inc = self.target / "debug/incremental"
        old, new = inc / "crate-old", inc / "crate-new"
        old.mkdir(parents=True)
        (old / "f").write_text("x")
        new.mkdir()
        age(old, 30)
        code, out = self.run_main()
        self.assertEqual(code, 0)
        self.assertFalse(old.exists())
        self.assertTrue(new.exists())
        self.assertIn(str(old), out)

    def test_path_vanishing_mid_sweep_is_tolerated(self):
        inc = self.target / "debug/incremental"
        a, b = inc / "crate-a", inc / "crate-b"
        a.mkdir(parents=True)
        b.mkdir()
        age(a, 30)
        age(b, 30)
        real = st.shutil.rmtree
        calls = []

        def racing(p, *args, **kw):
            if not calls:  # a concurrent cargo removes it first
                calls.append(p)
                real(p)
            return real(p, *args, **kw)

        st.shutil.rmtree = racing
        self.addCleanup(setattr, st.shutil, "rmtree", real)
        code, _ = self.run_main()
        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        self.assertFalse(a.exists())
        self.assertFalse(b.exists())

    def test_remove_missing_path_is_ok(self):
        st._remove(self.target / "never-existed")


if __name__ == "__main__":
    unittest.main()
