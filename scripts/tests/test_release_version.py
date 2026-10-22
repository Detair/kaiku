"""Tests for scripts/release_version.py (single source of release version)."""
from __future__ import annotations

import difflib
import shutil
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = SCRIPTS_DIR.parent
sys.path.insert(0, str(SCRIPTS_DIR))

from release_version import (  # noqa: E402
    VersionError,
    check_consistency,
    parse_version,
    read_versions,
    write_version,
)

CLI = SCRIPTS_DIR / "release_version.py"
COPIED_FILES = (
    "client/src-tauri/tauri.conf.json",
    "Cargo.toml",
    "client/package.json",
    "mobile/android/version.properties",
    "Cargo.lock",
)
BASELINE = parse_version("0.1.0")


def run_cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(CLI), *args],
        capture_output=True,
        text=True,
        check=False,
        cwd=REPO_ROOT,
    )


class ReleaseVersionTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        for rel in COPIED_FILES:
            dst = self.tmp / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO_ROOT / rel, dst)
        # Normalise to a fixed baseline so the tests do not depend on the repo's current version.
        write_version(self.tmp, BASELINE)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def test_parse_final(self):
        v = parse_version("v1.0.0")
        self.assertEqual((v.major, v.minor, v.patch, v.rc), (1, 0, 0, None))
        self.assertEqual(v.semver, "1.0.0")
        self.assertFalse(v.is_prerelease)

    def test_parse_rc(self):
        v = parse_version("1.0.0-rc.3")
        self.assertEqual(v.rc, 3)
        self.assertEqual(v.tag, "v1.0.0-rc.3")
        self.assertTrue(v.is_prerelease)

    def test_parse_rejects_out_of_range(self):
        for bad in [
            "1.0",
            "v1.0.0-beta.1",
            "v1.0.0-rc.0",
            "v1.0.0-rc.99",
            "v1.100.0",
            "v1.0.100",
            "v1.0.0-rc.1+meta",
            " v1.0.0",
            "v01.0.0",
        ]:
            with self.subTest(bad=bad), self.assertRaises(VersionError):
                parse_version(bad)

    def test_android_code_values(self):
        self.assertEqual(parse_version("1.0.0").android_version_code, 1_000_099)
        self.assertEqual(parse_version("1.0.0-rc.1").android_version_code, 1_000_001)
        self.assertEqual(parse_version("0.1.0").android_version_code, 10_099)

    def test_android_code_ordering(self):
        order = ["0.9.0-rc.1", "1.0.0-rc.1", "1.0.0-rc.98", "1.0.0", "1.0.1-rc.1", "1.0.1", "1.1.0-rc.1"]
        codes = [parse_version(s).android_version_code for s in order]
        self.assertEqual(codes, sorted(codes))
        self.assertEqual(len(set(codes)), len(codes))

    def test_repo_is_consistent(self):
        # Guards against version drift on main.
        self.assertEqual(check_consistency(REPO_ROOT, None), [])

    def test_bump_rewrites_only_version_lines(self):
        before = {rel: (self.tmp / rel).read_text(encoding="utf-8").splitlines() for rel in COPIED_FILES}
        write_version(self.tmp, parse_version("1.0.0-rc.1"))
        versions = read_versions(self.tmp)
        for label, value in versions.items():
            expected = "1000001" if label.endswith(":versionCode") else "1.0.0-rc.1"
            self.assertEqual(value, expected, label)
        expected_changes = {
            "client/src-tauri/tauri.conf.json": 1,
            "Cargo.toml": 1,
            "client/package.json": 1,
            "mobile/android/version.properties": 2,
            "Cargo.lock": 5,
        }
        for rel, count in expected_changes.items():
            after = (self.tmp / rel).read_text(encoding="utf-8").splitlines()
            removed = [
                line
                for line in difflib.unified_diff(before[rel], after, lineterm="", n=0)
                if line.startswith("-") and not line.startswith("---")
            ]
            self.assertEqual(len(after), len(before[rel]), rel)
            self.assertEqual(len(removed), count, rel)

    def test_bump_preserves_line_endings(self):
        # Windows runners would otherwise rewrite LF files with CRLF (and the
        # universal-newline read would turn CRLF files into LF everywhere).
        crlf = "mobile/android/version.properties"
        crlf_path = self.tmp / crlf
        crlf_path.write_bytes(crlf_path.read_bytes().replace(b"\r\n", b"\n").replace(b"\n", b"\r\n"))
        write_version(self.tmp, parse_version("1.0.0-rc.1"))
        for rel in COPIED_FILES:
            data = (self.tmp / rel).read_bytes()
            if rel == crlf:
                self.assertNotIn(b"\n", data.replace(b"\r\n", b""), rel)
                self.assertIn(b"versionName=1.0.0-rc.1\r\n", data, rel)
            else:
                self.assertNotIn(b"\r\n", data, rel)

    def test_write_uses_no_newline_translation(self):
        # On Windows text-mode writes translate "\n"; the writer must disable that.
        import release_version

        calls = []
        real_open = Path.open

        def spy(path, mode="r", *args, **kwargs):
            calls.append((path.name, mode, kwargs.get("newline")))
            return real_open(path, mode, *args, **kwargs)

        with unittest.mock.patch.object(Path, "open", spy):
            release_version.write_version(self.tmp, parse_version("2.0.0"))
        writes = [c for c in calls if "w" in c[1]]
        self.assertTrue(writes)
        self.assertTrue(all(newline == "" for _, _, newline in calls), calls)

    def test_check_detects_mismatch(self):
        pkg = self.tmp / "client/package.json"
        current = read_versions(self.tmp)["client/package.json"]
        original = pkg.read_text(encoding="utf-8")
        edited = original.replace(f'  "version": "{current}",', '  "version": "9.9.9",', 1)
        self.assertNotEqual(edited, original, "mismatch edit did not change package.json")
        pkg.write_text(edited, encoding="utf-8")
        errs = check_consistency(self.tmp, None)
        self.assertEqual(len(errs), 1, errs)
        self.assertIn("client/package.json", errs[0])

    def test_check_tag_mismatch_lists_each_file(self):
        errs = check_consistency(self.tmp, parse_version("v1.0.0-rc.1"))
        self.assertEqual(len(errs), 5, errs)
        self.assertTrue(all("expected 1.0.0-rc.1" in e or "expected 1000001" in e for e in errs), errs)

    def test_cli_parse_outputs_github_output_lines(self):
        result = run_cli("parse", "v1.0.0-rc.2")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout.splitlines(),
            ["version=1.0.0-rc.2", "tag=v1.0.0-rc.2", "is_prerelease=true", "android_version_code=1000002"],
        )

    def test_cli_invalid_exit_2(self):
        self.assertEqual(run_cli("parse", "v1.0").returncode, 2)


if __name__ == "__main__":
    unittest.main()
