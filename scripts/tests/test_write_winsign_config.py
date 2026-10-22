"""Tests for scripts/write_winsign_config.py (Tauri Windows signCommand)."""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

CLI = Path(__file__).resolve().parents[1] / "write_winsign_config.py"
VARS = {
    "AZURE_TRUSTED_SIGNING_ENDPOINT": "https://weu.codesigning.azure.net",
    "AZURE_TRUSTED_SIGNING_ACCOUNT": "acct",
    "AZURE_TRUSTED_SIGNING_PROFILE": "prof",
}


class WriteWinsignConfigTest(unittest.TestCase):
    def run_cli(self, env_vars: dict[str, str]):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        out = Path(tmp.name) / "tauri-winsign.json"
        env = {"PATH": os.environ.get("PATH", ""), **env_vars}
        proc = subprocess.run(
            [sys.executable, str(CLI), "--output", str(out)], env=env, capture_output=True, text=True
        )
        return proc, out

    def test_writes_sign_command(self):
        proc, out = self.run_cli(VARS)
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        config = json.loads(out.read_text(encoding="utf-8"))
        self.assertEqual(
            config,
            {"bundle": {"windows": {"signCommand":
                "trusted-signing-cli -e https://weu.codesigning.azure.net -a acct -c prof -d Kaiku %1"}}},
        )

    def test_missing_var_fails_and_names_it(self):
        env = dict(VARS, AZURE_TRUSTED_SIGNING_PROFILE="")
        proc, out = self.run_cli(env)
        self.assertEqual(proc.returncode, 1)
        self.assertIn("AZURE_TRUSTED_SIGNING_PROFILE", proc.stdout + proc.stderr)
        self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
