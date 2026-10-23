"""Tests for scripts/verify_updater_signatures.py (updater key vs. configured pubkey).

The minisign binary is replaced by a fake that logs its arguments; real
ed25519 verification is minisign's job and is exercised in publish runs.
"""
from __future__ import annotations

import base64
import contextlib
import io
import json
import os
import stat
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS_DIR))

import verify_updater_signatures as vus  # noqa: E402

KEY_ID = bytes.fromhex("0123456789abcdef")
OTHER_ID = bytes.fromhex("fedcba9876543210")
ASSETS = {
    "linux-x86_64": "Kaiku_1.0.0-rc.1_amd64.AppImage",
    "windows-x86_64": "Kaiku_1.0.0-rc.1_x64-setup.exe",
    "darwin-x86_64": "Kaiku-x86_64.app.tar.gz",
    "darwin-aarch64": "Kaiku-aarch64.app.tar.gz",
}

FAKE_MINISIGN = """#!/bin/sh
# Log argv and copies of the key/signature files, then exit as configured.
log="$FAKE_LOG_DIR"
n=$(ls "$log" | grep -c '^call' || true)
printf '%s\\n' "$@" > "$log/call$n.args"
while [ $# -gt 0 ]; do
  case "$1" in
    -p) cp "$2" "$log/call$n.pub"; shift ;;
    -x) cp "$2" "$log/call$n.sig"; shift ;;
  esac
  shift
done
if [ -n "$FAKE_FAIL_MATCH" ] && grep -q "$FAKE_FAIL_MATCH" "$log/call$n.args"; then
  echo "Signature verification failed" >&2
  exit 1
fi
exit 0
"""


def minisign_file(comment: str, payload: bytes) -> str:
    return f"untrusted comment: {comment}\n{base64.b64encode(payload).decode()}\n"


def pubkey_b64(key_id: bytes = KEY_ID) -> str:
    text = minisign_file("minisign public key", b"Ed" + key_id + bytes(32))
    return base64.b64encode(text.encode()).decode()


def sig_b64(key_id: bytes = KEY_ID) -> str:
    text = minisign_file("signature from tauri secret key", b"ED" + key_id + bytes(64))
    text += "trusted comment: timestamp:0\tfile:x\n" + base64.b64encode(bytes(64)).decode() + "\n"
    return base64.b64encode(text.encode()).decode()


class VerifyUpdaterSignaturesTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.assets = self.tmp / "assets"
        self.assets.mkdir()
        self.log = self.tmp / "log"
        self.log.mkdir()
        self.binary = self.tmp / "bin" / "minisign"
        self.binary.parent.mkdir()
        self.binary.write_text(FAKE_MINISIGN, encoding="utf-8")
        self.binary.chmod(self.binary.stat().st_mode | stat.S_IEXEC)
        self.config = self.tmp / "tauri.conf.json"
        self.write_config(pubkey_b64())
        self.manifest = self.tmp / "latest.json"
        mock_env = {"FAKE_LOG_DIR": str(self.log), "FAKE_FAIL_MATCH": ""}
        self._env = unittest.mock.patch.dict(os.environ, mock_env)
        self._env.start()

    def tearDown(self):
        self._env.stop()
        self._tmp.cleanup()

    def write_config(self, pubkey: str | None):
        updater = {"endpoints": []} if pubkey is None else {"pubkey": pubkey, "endpoints": []}
        self.config.write_text(json.dumps({"plugins": {"updater": updater}}), encoding="utf-8")

    def write_manifest(self, sig_ids: dict[str, bytes] | None = None):
        platforms = {}
        for key, name in ASSETS.items():
            (self.assets / name).write_bytes(b"bin")
            key_id = (sig_ids or {}).get(key, KEY_ID)
            platforms[key] = {
                "signature": sig_b64(key_id),
                "url": f"https://github.com/Detair/kaiku/releases/download/v1.0.0-rc.1/{name}",
            }
        self.manifest.write_text(json.dumps({"version": "1.0.0-rc.1", "platforms": platforms}), encoding="utf-8")

    def run_main(self, mode: str = "publish", binary: str | None = None) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = vus.main([
                "--manifest", str(self.manifest), "--assets-dir", str(self.assets), "--mode", mode,
                "--config", str(self.config), "--minisign", binary or str(self.binary),
            ])
        return code, out.getvalue(), err.getvalue()

    def calls(self) -> list[list[str]]:
        return [p.read_text().splitlines() for p in sorted(self.log.glob("call*.args"))]

    def test_repo_pubkey_is_parsed_from_tauri_conf(self):
        pub = vus.load_pubkey(vus.DEFAULT_CONFIG)
        self.assertTrue(pub.startswith("untrusted comment: minisign public key"))
        self.assertEqual(len(pub.splitlines()), 2)
        # The id in the comment is the id encoded in the key itself.
        self.assertIn(vus._key_id(pub, "pubkey"), pub.splitlines()[0])

    def test_all_platforms_verified_against_configured_pubkey(self):
        self.write_manifest()
        code, out, err = self.run_main()
        self.assertEqual(code, 0, err)
        calls = self.calls()
        self.assertEqual(len(calls), len(ASSETS))
        verified = set()
        for args in calls:
            self.assertEqual(args[:2], ["-V", "-q"])
            self.assertEqual(args[args.index("-m") + 1], str(self.assets / Path(args[args.index("-m") + 1]).name))
            verified.add(Path(args[args.index("-m") + 1]).name)
        self.assertEqual(verified, set(ASSETS.values()))
        expected_pub = base64.b64decode(pubkey_b64()).decode()
        for pub in self.log.glob("call*.pub"):
            self.assertEqual(pub.read_text(), expected_pub)
        for sig in self.log.glob("call*.sig"):
            self.assertEqual(sig.read_text(), base64.b64decode(sig_b64()).decode())
        self.assertIn("signature OK", out)

    def test_verifier_failure_names_platform(self):
        self.write_manifest()
        os.environ["FAKE_FAIL_MATCH"] = "x64-setup.exe"
        code, _, err = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("::error::updater signature check: windows-x86_64:", err)
        self.assertNotIn("linux-x86_64:", err)
        self.assertIn("refusing to publish", err)

    def test_key_id_mismatch_names_platform_without_calling_binary(self):
        self.write_manifest({"darwin-aarch64": OTHER_ID})
        code, _, err = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("darwin-aarch64:", err)
        self.assertIn("wrong TAURI_SIGNING_PRIVATE_KEY", err)
        self.assertEqual(len(self.calls()), len(ASSETS) - 1)

    def test_dry_run_skips_with_notice(self):
        code, out, _ = self.run_main(mode="dry-run")
        self.assertEqual(code, 0)
        self.assertIn("::notice::", out)
        self.assertEqual(self.calls(), [])

    def test_missing_binary_fails_publish(self):
        self.write_manifest()
        code, _, err = self.run_main(binary=str(self.tmp / "nope" / "minisign"))
        self.assertEqual(code, 1)
        self.assertIn("not found", err)

    def test_missing_pubkey_fails_publish(self):
        self.write_config(None)
        self.write_manifest()
        code, _, err = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("plugins.updater.pubkey is missing", err)

    def test_missing_asset_names_platform(self):
        self.write_manifest()
        (self.assets / ASSETS["linux-x86_64"]).unlink()
        code, _, err = self.run_main()
        self.assertEqual(code, 1)
        self.assertIn("linux-x86_64: asset", err)


if __name__ == "__main__":
    unittest.main()
