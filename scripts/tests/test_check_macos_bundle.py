"""Run with: python3 -m unittest scripts/tests/test_check_macos_bundle.py"""
import importlib.util
import json
import plistlib
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "guardrails", Path(__file__).resolve().parents[1] / "check_ci_guardrails.py"
)
guardrails = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guardrails)

USAGE = {
    "NSMicrophoneUsageDescription": "mic",
    "NSCameraUsageDescription": "cam",
}
ENT = {
    "com.apple.security.device.audio-input": True,
    "com.apple.security.device.camera": True,
}


def build(tmp, info=USAGE, ent=ENT, conf=None):
    d = Path(tmp)
    if info is not None:
        (d / "Info.plist").write_bytes(info if isinstance(info, bytes) else plistlib.dumps(info))
    if ent is not None:
        (d / "Entitlements.plist").write_bytes(ent if isinstance(ent, bytes) else plistlib.dumps(ent))
    conf = conf if conf is not None else {"bundle": {"macOS": {"entitlements": "./Entitlements.plist"}}}
    (d / "tauri.conf.json").write_text(json.dumps(conf))
    return d


class CheckMacosBundle(unittest.TestCase):
    def run_check(self, **kw):
        with tempfile.TemporaryDirectory() as tmp:
            d = build(tmp, **kw)
            errors: list[str] = []
            guardrails.check_macos_bundle(errors, d)
            return errors

    def test_valid(self):
        self.assertEqual(self.run_check(), [])

    def test_empty_info_plist(self):
        self.assertTrue(self.run_check(info=b""))

    def test_commented_out_keys(self):
        commented = (
            b'<?xml version="1.0"?><plist version="1.0"><dict>'
            b"<!-- <key>NSMicrophoneUsageDescription</key><string>x</string>"
            b" <key>NSCameraUsageDescription</key><string>y</string> -->"
            b"</dict></plist>"
        )
        errors = self.run_check(info=commented)
        self.assertEqual(len(errors), 2)

    def test_blank_string(self):
        self.assertTrue(self.run_check(info={**USAGE, "NSCameraUsageDescription": "  "}))

    def test_non_string_value(self):
        self.assertTrue(self.run_check(info={**USAGE, "NSCameraUsageDescription": True}))

    def test_malformed_info(self):
        self.assertTrue(self.run_check(info=b"<plist><dict>"))

    def test_entitlement_false(self):
        ent = {**ENT, "com.apple.security.device.camera": False}
        self.assertTrue(self.run_check(ent=ent))

    def test_entitlement_string_true(self):
        ent = {**ENT, "com.apple.security.device.camera": "true"}
        self.assertTrue(self.run_check(ent=ent))

    def test_empty_entitlements(self):
        self.assertTrue(self.run_check(ent=b""))

    def test_missing_entitlements_file(self):
        self.assertTrue(self.run_check(ent=None))

    def test_entitlements_not_configured(self):
        self.assertTrue(self.run_check(conf={"bundle": {}}))


if __name__ == "__main__":
    unittest.main()
