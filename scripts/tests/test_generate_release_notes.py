"""Tests for scripts/generate_release_notes.py."""
from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))

from generate_release_notes import SECTIONS, extract_section, fit_release_notes  # noqa: E402

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


class FitReleaseNotesTest(unittest.TestCase):
    URL = "https://example.invalid/CHANGELOG.md"
    HEADER = "## Milestone\n- Version: 1.0.0-rc.1\n\n"

    def items(self) -> dict[str, list[str]]:
        items = {section: [] for section in SECTIONS}
        items["Added"] = [f"- added entry {i} " + "a" * 200 for i in range(400)]
        items["Security"] = [f"- security entry {i} " + "s" * 200 for i in range(100)]
        return items

    def test_trimmed_notes_stay_within_budget(self):
        notes = fit_release_notes(self.HEADER, self.items(), 20_000, self.URL)
        self.assertLessEqual(len(notes), 20_000)
        self.assertTrue(notes.startswith(self.HEADER))

    def test_every_section_keeps_its_heading_and_a_share(self):
        notes = fit_release_notes(self.HEADER, self.items(), 20_000, self.URL)
        for section in SECTIONS:
            self.assertIn(f"### {section}\n", notes)
        self.assertIn("- security entry 0 ", notes)
        self.assertIn("- None", notes)

    def test_trimmed_section_points_to_full_changelog(self):
        notes = fit_release_notes(self.HEADER, self.items(), 20_000, self.URL)
        self.assertIn(f"more, see the [full changelog]({self.URL})", notes)
        self.assertNotIn("- added entry 399 ", notes)

    def test_entries_keep_their_order(self):
        notes = fit_release_notes(self.HEADER, self.items(), 20_000, self.URL)
        self.assertLess(notes.index("- added entry 0 "), notes.index("- added entry 1 "))

    def test_small_section_keeps_its_entry(self):
        items = self.items()
        items["Security"] = ["- the only security entry"]
        notes = fit_release_notes(self.HEADER, items, 20_000, self.URL)
        self.assertIn("- the only security entry\n", notes)
        self.assertLessEqual(len(notes), 20_000)

    def test_huge_entry_counts_stay_within_budget(self):
        items = {section: ["- x"] * 100_000 for section in SECTIONS}
        for max_chars in (1_000, 5_000):
            with self.subTest(max_chars=max_chars):
                notes = fit_release_notes(self.HEADER, items, max_chars, self.URL)
                self.assertLessEqual(len(notes), max_chars)
                self.assertEqual(notes.count("more, see the [full changelog]"), len(SECTIONS))

    def test_budget_below_header_is_rejected(self):
        with self.assertRaises(ValueError):
            fit_release_notes(self.HEADER, self.items(), 100, self.URL)


class MainTest(unittest.TestCase):
    def run_main(self, version: str, *extra: str) -> str:
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
                    *extra,
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

    def test_main_leaves_notes_untouched_within_max_chars(self):
        self.assertEqual(
            self.run_main("v1.1.0-rc.1"),
            self.run_main("v1.1.0-rc.1", "--max-chars", "100000"),
        )


if __name__ == "__main__":
    unittest.main()
