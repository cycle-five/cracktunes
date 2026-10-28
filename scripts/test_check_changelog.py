"""Tests for check_changelog.py. Each test writes a Cargo.toml and a
CHANGELOG.md into a throwaway directory and runs the checker on it."""
import contextlib
import io
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import check_changelog as cc  # noqa: E402

CARGO = '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.46.0"\n'
HEAD = "# Change Log\n\n## Unreleased\n\n## v0.46.0 (2026-10-28)\n\n### Added\n\n"


def run(changelog, cargo=CARGO):
    """Write the files (None skips one); return (code, stdout)."""
    with tempfile.TemporaryDirectory() as root:
        for name, text in (("Cargo.toml", cargo), ("CHANGELOG.md", changelog)):
            if text is not None:
                with open(os.path.join(root, name), "w", encoding="utf-8") as f:
                    f.write(text)
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = cc.main(["--root", root])
        return code, out.getvalue()


class VersionHeadingTests(unittest.TestCase):
    def test_dated_heading_passes(self):
        code, out = run(HEAD + "- A thing.\n")
        self.assertEqual(code, 0, out)
        self.assertIn("v0.46.0: 0 problem(s)", out)

    def test_undated_heading_passes(self):
        code, out = run("## v0.46.0\n\n- A thing.\n")
        self.assertEqual(code, 0, out)

    def test_only_unreleased_fails(self):
        code, out = run("# Change Log\n\n## Unreleased\n\n- A thing.\n\n## v0.45.3 (2026-10-27)\n")
        self.assertEqual(code, 1)
        self.assertIn("version-heading", out)
        self.assertIn("`## v0.46.0`", out)

    def test_a_longer_version_is_not_this_one(self):
        for heading in ("## v0.46.01 (2026-10-28)", "## v0.46.0-rc1", "## v0.46.0.1"):
            code, out = run(f"{heading}\n\n- A thing.\n")
            self.assertEqual(code, 1, heading)
            self.assertIn("version-heading", out)

    def test_another_level_is_not_the_heading(self):
        for heading in ("### v0.46.0", "# v0.46.0", " ## v0.46.0"):
            code, _ = run(f"{heading}\n\n- A thing.\n")
            self.assertEqual(code, 1, heading)

    def test_package_version_without_a_workspace(self):
        code, out = run("## v1.2.3\n", cargo='[package]\nname = "x"\nversion = "1.2.3"\n')
        self.assertEqual(code, 0, out)


class MentionTests(unittest.TestCase):
    def test_bare_mention_fails_with_its_line(self):
        code, out = run(HEAD + '- Logged as "@Mika used Rename on @Jo".\n')
        self.assertEqual(code, 1)
        self.assertIn("CHANGELOG.md:9: mention: `@Mika`", out)
        self.assertIn("CHANGELOG.md:9: mention: `@Jo`", out)

    def test_mention_at_line_start_and_in_angle_brackets_fails(self):
        for line in ("@everyone is pinged.", "- Pings <@user> now."):
            code, out = run(HEAD + line + "\n")
            self.assertEqual(code, 1, line)
            self.assertIn("mention:", out)

    def test_code_span_passes(self):
        code, out = run(HEAD + "- Logged as `@Mika used Rename on @Jo`, "
                        "and ``@everyone`` stays text.\n")
        self.assertEqual(code, 0, out)

    def test_fenced_block_passes(self):
        for fence in ("```", "~~~"):
            code, out = run(HEAD + f"- Example:\n\n  {fence}text\n  @Mika renamed @Jo\n  {fence}\n")
            self.assertEqual(code, 0, out)

    def test_text_after_a_fence_is_checked(self):
        code, out = run(HEAD + "```\n@Mika\n```\n@Jo\n")
        self.assertEqual(code, 1)
        self.assertIn("`@Jo`", out)
        self.assertNotIn("`@Mika`", out)

    def test_email_and_url_pass(self):
        code, out = run(HEAD + "- Mail cycle.five@proton.me, see https://example.com/@user.\n")
        self.assertEqual(code, 0, out)

    def test_both_rules_report_together(self):
        code, out = run("## Unreleased\n\n- @Jo\n")
        self.assertEqual(code, 1)
        self.assertIn("version-heading", out)
        self.assertIn("mention", out)


class ErrorTests(unittest.TestCase):
    def test_missing_changelog_is_an_error(self):
        code, _ = run(None)
        self.assertEqual(code, 2)

    def test_no_version_is_an_error(self):
        code, _ = run(HEAD, cargo="[workspace]\nmembers = []\n")
        self.assertEqual(code, 2)

    def test_bad_toml_is_an_error(self):
        code, _ = run(HEAD, cargo="[workspace\n")
        self.assertEqual(code, 2)

    def test_unknown_argument_is_an_error(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(cc.main(["--nope"]), 2)


class RealRepoTests(unittest.TestCase):
    def test_this_repo_passes(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = cc.main([])
        self.assertEqual(code, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
