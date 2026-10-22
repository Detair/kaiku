"""Tests for scripts/generate_updater_manifest.py (Tauri latest.json)."""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

CLI = Path(__file__).resolve().parents[1] / "generate_updater_manifest.py"
TAG = "v1.0.0-rc.1"
ASSETS = {
    "linux-x86_64": "Kaiku_1.0.0-rc.1_amd64.AppImage",
    "windows-x86_64": "Kaiku_1.0.0-rc.1_x64-setup.exe",
    "darwin-x86_64": "Kaiku-x86_64.app.tar.gz",
    "darwin-aarch64": "Kaiku-aarch64.app.tar.gz",
}


class UpdaterManifestTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.assets = self.tmp / "assets"
        self.assets.mkdir()
        self.output = self.tmp / "latest.json"

    def tearDown(self):
        self._tmp.cleanup()

    def add_assets(self, skip_sig: tuple[str, ...] = ()):
        for key, name in ASSETS.items():
            (self.assets / name).write_bytes(b"bin")
            if key not in skip_sig:
                (self.assets / f"{name}.sig").write_text(f"sig-{key}\n", encoding="utf-8")
        # Unrelated files must not be picked up.
        (self.assets / "Kaiku_1.0.0-rc.1_amd64.deb").write_bytes(b"deb")

    def run_cli(self, mode: str):
        return subprocess.run(
            [
                sys.executable, str(CLI),
                "--assets-dir", str(self.assets),
                "--tag", TAG,
                "--repo", "Detair/kaiku",
                "--mode", mode,
                "--output", str(self.output),
            ],
            capture_output=True,
            text=True,
        )

    def test_all_platforms_manifest(self):
        self.add_assets()
        proc = self.run_cli("publish")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        manifest = json.loads(self.output.read_text(encoding="utf-8"))
        self.assertEqual(manifest["version"], "1.0.0-rc.1")
        self.assertEqual(set(manifest["platforms"]), set(ASSETS))
        linux = manifest["platforms"]["linux-x86_64"]
        self.assertEqual(
            linux["url"],
            "https://github.com/Detair/kaiku/releases/download/v1.0.0-rc.1/Kaiku_1.0.0-rc.1_amd64.AppImage",
        )
        self.assertEqual(linux["signature"], "sig-linux-x86_64")
        self.assertIn("pub_date", manifest)
        self.assertIn(TAG, manifest["notes"])

    def test_publish_mode_requires_all_platforms(self):
        self.add_assets(skip_sig=("darwin-x86_64",))
        proc = self.run_cli("publish")
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        self.assertIn("darwin-x86_64", proc.stderr)
        self.assertFalse(self.output.exists())

    def test_dry_run_tolerates_missing(self):
        proc = self.run_cli("dry-run")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertFalse(self.output.exists())
        self.assertIn("::warning::", proc.stderr)

    def test_dry_run_partial_writes_present_platforms(self):
        self.add_assets(skip_sig=("windows-x86_64",))
        proc = self.run_cli("dry-run")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        manifest = json.loads(self.output.read_text(encoding="utf-8"))
        self.assertNotIn("windows-x86_64", manifest["platforms"])
        self.assertIn("windows-x86_64", proc.stderr)


if __name__ == "__main__":
    unittest.main()
