"""Tests for scripts/generate_release_notes.py."""
from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))

from generate_release_notes import extract_section  # noqa: E402

CHANGELOG = """# Changelog

## [Unreleased]

### Added
- unreleased thing

## [1.0.0] - 2026-12-10
### Added
- x
## [0.1.0] - 2026-01-18
### Added
- old
"""

ROADMAP = "**Current Phase:** Phase 7\n**Last Updated:** 2026-10-22\n"


class ExtractSectionTest(unittest.TestCase):
    def test_extract_section_returns_curated_block(self):
        body = extract_section(CHANGELOG, "1.0.0")
        self.assertIsNotNone(body)
        self.assertIn("- x", body)
        self.assertNotIn("0.1.0", body)
        self.assertNotIn("old", body)
        self.assertNotIn("unreleased thing", body)

    def test_extract_section_missing_returns_none(self):
        self.assertIsNone(extract_section(CHANGELOG, "2.0.0"))

    def test_extract_section_does_not_prefix_match(self):
        self.assertIsNone(extract_section(CHANGELOG, "1.0"))


class MainTest(unittest.TestCase):
    def run_main(self, version: str) -> str:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            (tmp_path / "CHANGELOG.md").write_text(CHANGELOG, encoding="utf-8")
            (tmp_path / "roadmap.md").write_text(ROADMAP, encoding="utf-8")
            out = tmp_path / "notes.md"
            proc = subprocess.run(
                [
                    sys.executable, str(SCRIPTS_DIR / "generate_release_notes.py"),
                    "--version", version,
                    "--changelog", str(tmp_path / "CHANGELOG.md"),
                    "--roadmap", str(tmp_path / "roadmap.md"),
                    "--output", str(out),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            return out.read_text(encoding="utf-8")

    def test_main_uses_curated_section_when_present(self):
        for version in ("1.0.0", "v1.0.0"):
            with self.subTest(version=version):
                notes = self.run_main(version)
                self.assertIn("- x", notes)
                self.assertNotIn("unreleased thing", notes)

    def test_main_falls_back_to_unreleased(self):
        notes = self.run_main("v1.1.0-rc.1")
        self.assertIn("unreleased thing", notes)
        self.assertNotIn("- x", notes)


if __name__ == "__main__":
    unittest.main()
