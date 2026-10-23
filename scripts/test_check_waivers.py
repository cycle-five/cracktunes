"""Tests for check_waivers.py. Each test builds a throwaway git repo, since the
checker lists Rust files with `git ls-files`."""
import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import check_waivers as cw  # noqa: E402

REAL_CONFIG = os.path.join(os.path.dirname(HERE), ".cargo", "mutants.toml")


def run(files):
    """Create `files` ({path: text}) in a fresh git repo; return (code, output)."""
    with tempfile.TemporaryDirectory() as root:
        subprocess.run(["git", "init", "-q", root], check=True)
        for path, text in files.items():
            full = os.path.join(root, path)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "w", encoding="utf-8") as f:
                f.write(text)
        subprocess.run(["git", "-C", root, "add", "."], check=True)
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = cw.main(["--root", root])
        return code, out.getvalue()


SKIP = "#[cfg_attr(test, mutants::skip)]"


class RustTests(unittest.TestCase):
    def test_reasoned_waiver_passes(self):
        code, out = run({"a.rs": f"// Logging only.\n{SKIP}\nfn f() {{}}\n"})
        self.assertEqual(code, 0, out)
        self.assertIn("1 waiver(s) checked, 0 problem(s)", out)

    def test_indented_reasoned_waiver_passes(self):
        code, out = run({"a.rs": f"impl X {{\n    // Why.\n    {SKIP}\n    fn f() {{}}\n}}\n"})
        self.assertEqual(code, 0, out)

    def test_no_comment_fails(self):
        code, out = run({"a.rs": f"fn g() {{}}\n{SKIP}\nfn f() {{}}\n"})
        self.assertEqual(code, 1)
        self.assertIn("a.rs:2:", out)
        self.assertIn("reason", out)

    def test_first_line_fails(self):
        code, out = run({"a.rs": f"{SKIP}\nfn f() {{}}\n"})
        self.assertEqual(code, 1)
        self.assertIn("a.rs:1:", out)

    def test_doc_comment_is_not_a_reason(self):
        for c in ("/// Docs.", "//! Docs."):
            code, out = run({"a.rs": f"{c}\n{SKIP}\nfn f() {{}}\n"})
            self.assertEqual(code, 1, c)
            self.assertIn("a.rs:2:", out)

    def test_empty_comment_is_not_a_reason(self):
        for c in ("//", "//   "):
            code, out = run({"a.rs": f"{c}\n{SKIP}\nfn f() {{}}\n"})
            self.assertEqual(code, 1, c)

    def test_blank_line_between_is_not_a_reason(self):
        code, out = run({"a.rs": f"// Why.\n\n{SKIP}\nfn f() {{}}\n"})
        self.assertEqual(code, 1)

    def test_bare_form_fails(self):
        code, out = run({"a.rs": "// Why.\n#[mutants::skip]\nfn f() {}\n"})
        self.assertEqual(code, 1)
        self.assertIn("a.rs:2:", out)
        self.assertIn("cfg_attr(test, mutants::skip)", out)

    def test_commented_out_waiver_is_not_a_waiver(self):
        code, out = run({"a.rs": "// #[mutants::skip]\n// " + SKIP + "\nfn f() {}\n"})
        self.assertEqual(code, 0, out)
        self.assertIn("0 waiver(s) checked", out)

    def test_only_tracked_rs_files_are_checked(self):
        code, out = run({"a.txt": f"{SKIP}\n"})
        self.assertEqual(code, 0, out)

    def test_inner_attribute_is_a_waiver(self):
        inner = "#![cfg_attr(test, mutants::skip)]"
        code, out = run({"a.rs": f"{inner}\nfn f() {{}}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("a.rs:1: rust-reason", out)
        code, out = run({"a.rs": f"// The whole module is glue.\n{inner}\nfn f() {{}}\n"})
        self.assertEqual(code, 0, out)
        self.assertIn("1 waiver(s) checked", out)

    def test_other_cfg_predicates_are_waivers(self):
        for attr in ("#[cfg_attr(debug_assertions, mutants::skip)]",
                     "#[cfg_attr(all(test, feature = \"x\"), mutants::skip)]",
                     "#[inline] #[cfg_attr(test, mutants::skip)]"):
            with self.subTest(attr):
                code, out = run({"a.rs": f"fn g() {{}}\n{attr}\nfn f() {{}}\n"})
                self.assertEqual(code, 1, out)
                self.assertIn("a.rs:2: rust-reason", out)
                code, out = run({"a.rs": f"// Why.\n{attr}\nfn f() {{}}\n"})
                self.assertEqual(code, 0, out)

    def test_wrapped_attribute_is_a_waiver(self):
        code, out = run({"a.rs": "#[cfg_attr(\n    test,\n    mutants::skip\n)]\nfn f() {}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("a.rs:3: rust-reason", out)

    def test_inner_bare_form_fails(self):
        code, out = run({"a.rs": "// Why.\n#![mutants::skip]\nfn f() {}\n"})
        self.assertEqual(code, 1)
        self.assertIn("a.rs:2: rust-bare-form", out)

    def test_block_comments_are_not_waivers(self):
        code, out = run({"a.rs": f"/* {SKIP}\n * {SKIP}\n */\nfn f() {{}}\n"})
        self.assertEqual(code, 0, out)
        self.assertIn("0 waiver(s) checked", out)


class ConfigTests(unittest.TestCase):
    def test_element_without_comment_fails(self):
        toml = 'exclude_globs = [\n    "a/**",\n]\n'
        code, out = run({".cargo/mutants.toml": toml})
        self.assertEqual(code, 1)
        self.assertIn(".cargo/mutants.toml:2:", out)

    def test_second_element_without_comment_fails(self):
        toml = 'exclude_re = [\n    # Why a.\n    "a",\n    "b",\n]\n'
        code, out = run({".cargo/mutants.toml": toml})
        self.assertEqual(code, 1)
        self.assertIn(".cargo/mutants.toml:4:", out)
        self.assertNotIn(".cargo/mutants.toml:3:", out)

    def test_commented_elements_pass(self):
        toml = ('exclude_globs = [\n    # Why a.\n    # More.\n    "a/**",\n'
                '    # Why b.\n    "b/**",\n]\n')
        code, out = run({".cargo/mutants.toml": toml})
        self.assertEqual(code, 0, out)
        self.assertIn("2 waiver(s) checked, 0 problem(s)", out)

    def test_one_line_array(self):
        code, out = run({".cargo/mutants.toml": '# Why.\nexclude_re = ["x"]\n'})
        self.assertEqual(code, 0, out)
        code, out = run({".cargo/mutants.toml": 'exclude_re = ["x"]\n'})
        self.assertEqual(code, 1)
        self.assertIn(".cargo/mutants.toml:1:", out)

    def test_every_narrowing_key_is_checked(self):
        for key in ("exclude_globs", "exclude_re", "examine_globs", "examine_re", "skip_calls"):
            with self.subTest(key):
                code, out = run({".cargo/mutants.toml": f'{key} = [\n    "a",\n]\n'})
                self.assertEqual(code, 1, out)
                self.assertIn(".cargo/mutants.toml:2: config-reason", out)

    def test_quoted_keys_are_checked(self):
        for key in ('"exclude_re"', "'exclude_re'"):
            with self.subTest(key):
                code, out = run({".cargo/mutants.toml": f'{key} = [\n    "a",\n]\n'})
                self.assertEqual(code, 1, out)
                self.assertIn(".cargo/mutants.toml:2: config-reason", out)

    def test_a_bare_hash_is_not_a_reason(self):
        for comment in ("#", "#   ", "##"):
            with self.subTest(comment):
                code, out = run({".cargo/mutants.toml":
                                 f'exclude_globs = [\n    {comment}\n    "a/**",\n]\n'})
                self.assertEqual(code, 1, out)
                self.assertIn(".cargo/mutants.toml:3: config-reason", out)

    def test_hash_inside_a_string_does_not_swallow_later_keys(self):
        toml = ('# Why.\nexclude_globs = ["a#b"]\n'
                'exclude_re = [\n    "x",\n]\n')
        code, out = run({".cargo/mutants.toml": toml})
        self.assertEqual(code, 1, out)
        self.assertIn(".cargo/mutants.toml:4: config-reason", out)
        self.assertIn("2 waiver(s) checked", out)

    def test_two_elements_on_one_line_fail(self):
        code, out = run({".cargo/mutants.toml": '# Why.\nexclude_re = ["a", "b"]\n'})
        self.assertEqual(code, 1, out)
        self.assertIn(".cargo/mutants.toml:2: config-reason: 2 elements", out)

    def test_count_mismatch_with_tomllib_is_a_problem(self):
        # An element the line scanner can't see (not a string) must not pass.
        code, out = run({".cargo/mutants.toml": 'exclude_re = [\n    # Why.\n    1,\n]\n'})
        self.assertEqual(code, 1, out)
        self.assertIn("config-count: `exclude_re` has 1 element(s)", out)

    def test_key_the_scanner_cannot_find_is_a_problem(self):
        code, out = run({".cargo/mutants.toml": '"exclude\\u005fre" = ["x"]\n'})
        self.assertEqual(code, 1, out)
        self.assertIn("config-count: `exclude_re` is set", out)

    def test_other_arrays_are_ignored(self):
        code, out = run({".cargo/mutants.toml": 'features = [\n    "x",\n]\n'})
        self.assertEqual(code, 0, out)

    def test_missing_file_is_fine(self):
        code, out = run({"a.rs": "fn f() {}\n"})
        self.assertEqual(code, 0, out)

    def test_unreadable_toml_is_exit_2(self):
        code, out = run({".cargo/mutants.toml": 'exclude_re = [\n'})
        self.assertEqual(code, 2)

    def test_real_config_passes(self):
        with open(REAL_CONFIG, encoding="utf-8") as f:
            code, out = run({".cargo/mutants.toml": f.read()})
        self.assertEqual(code, 0, out)


class ArgsTests(unittest.TestCase):
    def test_not_a_git_repo_is_exit_2(self):
        with tempfile.TemporaryDirectory() as root:
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(cw.main(["--root", root]), 2)

    def test_bad_args_is_exit_2(self):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(cw.main(["--nope"]), 2)


if __name__ == "__main__":
    unittest.main()
