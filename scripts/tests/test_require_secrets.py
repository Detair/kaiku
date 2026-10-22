"""Tests for scripts/require_secrets.py (secret gating for release jobs)."""
from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

CLI = Path(__file__).resolve().parents[1] / "require_secrets.py"
SECRET_VALUE = "super-secret-value-do-not-print"


class RequireSecretsTest(unittest.TestCase):
    def run_cli(self, mode: str, env_vars: dict[str, str], names: list[str]):
        with tempfile.TemporaryDirectory() as tmp:
            out_file = Path(tmp) / "github_output"
            out_file.write_text("", encoding="utf-8")
            env = {"PATH": os.environ.get("PATH", ""), "GITHUB_OUTPUT": str(out_file), **env_vars}
            proc = subprocess.run(
                [sys.executable, str(CLI), "--mode", mode, "--output", "signing", *names],
                env=env,
                capture_output=True,
                text=True,
            )
            return proc, out_file.read_text(encoding="utf-8")

    def test_publish_missing_fails_and_names_var(self):
        proc, output = self.run_cli(
            "publish",
            {"ANDROID_KEYSTORE": SECRET_VALUE},
            ["ANDROID_KEYSTORE", "ANDROID_KEY_ALIAS"],
        )
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("::error::", proc.stdout)
        self.assertIn("ANDROID_KEY_ALIAS", proc.stdout)
        self.assertNotIn("ANDROID_KEYSTORE,", proc.stdout)
        self.assertNotIn(SECRET_VALUE, proc.stdout + proc.stderr + output)
        self.assertNotIn("signing=true", output)

    def test_dry_run_missing_notices_and_continues(self):
        proc, output = self.run_cli("dry-run", {"A": SECRET_VALUE}, ["A", "B"])
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("signing=false", output)
        self.assertIn("::notice::", proc.stdout)
        self.assertIn("::notice::B not set — signing skipped (dry run)", proc.stdout)
        self.assertNotIn(SECRET_VALUE, proc.stdout + proc.stderr + output)

    def test_all_present_true(self):
        for mode in ("publish", "dry-run"):
            with self.subTest(mode=mode):
                proc, output = self.run_cli(mode, {"A": SECRET_VALUE, "B": "x"}, ["A", "B"])
                self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
                self.assertIn("signing=true", output)
                self.assertNotIn(SECRET_VALUE, proc.stdout + proc.stderr + output)

    def test_empty_string_counts_as_missing(self):
        proc, output = self.run_cli("dry-run", {"A": ""}, ["A"])
        self.assertEqual(proc.returncode, 0)
        self.assertIn("signing=false", output)
        proc, _ = self.run_cli("publish", {"A": ""}, ["A"])
        self.assertEqual(proc.returncode, 1)
        self.assertIn("A", proc.stdout)


if __name__ == "__main__":
    unittest.main()
