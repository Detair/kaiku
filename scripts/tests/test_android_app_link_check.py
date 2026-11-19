"""Tests for scripts/android_app_link_check.py (the verified setup App Link in a release APK)."""
from __future__ import annotations

import subprocess
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import android_app_link_check as check  # noqa: E402

CLI = Path(__file__).resolve().parents[1] / "android_app_link_check.py"
HOST = "chat.example.com"
NS = "http://schemas.android.com/apk/res/android"


def attr(depth: int, name: str, value: str, raw: bool = True) -> str:
    pad = " " * depth
    if raw:
        return f'{pad}A: {NS}:{name}(0x01010000)="{value}" (Raw: "{value}")'
    return f"{pad}A: {NS}:{name}(0x01010000)={value}"


def manifest(*filters: str, activity: str = "io.wolftown.kaiku.ui.MainActivity") -> str:
    """An aapt2 xmltree dump shaped like the real one, with the given intent filters on the activity."""
    lines = [
        "N: android=http://schemas.android.com/apk/res/android (line=2)",
        "  E: manifest (line=2)",
        "      E: application (line=10)",
        "          E: activity (line=60)",
        attr(12, "name", activity),
    ]
    lines += filters
    return "\n".join(lines) + "\n"


def setup_filter(
    data: list[tuple[str, str]] | None = None,
    verify: bool = True,
    actions: tuple[str, ...] = ("android.intent.action.VIEW",),
    categories: tuple[str, ...] = ("android.intent.category.DEFAULT", "android.intent.category.BROWSABLE"),
) -> str:
    data = data if data is not None else [("scheme", "https"), ("host", HOST), ("path", "/setup")]
    lines = ["              E: intent-filter (line=71)"]
    if verify:
        lines.append(attr(16, "autoVerify", "true", raw=False))
    for a in actions:
        lines += ["                  E: action (line=72)", attr(20, "name", a)]
    for c in categories:
        lines += ["                  E: category (line=74)", attr(20, "name", c)]
    lines.append("                  E: data (line=77)")
    lines += [attr(20, k, v) for k, v in data]
    return "\n".join(lines)


def callback_filter() -> str:
    return setup_filter(data=[("scheme", "https"), ("host", HOST), ("path", "/auth/callback")])


class AppLinkCheckTest(unittest.TestCase):
    def test_the_exact_filter_passes(self):
        self.assertEqual([], check.problems(manifest(setup_filter(), callback_filter()), HOST))

    def test_another_host_fails(self):
        self.assertTrue(check.problems(manifest(setup_filter()), "other.example.org"))

    def test_a_path_prefix_or_pattern_fails(self):
        for loose in ("pathPrefix", "pathPattern", "pathSuffix", "pathAdvancedPattern"):
            with self.subTest(loose):
                tree = manifest(setup_filter(data=[("scheme", "https"), ("host", HOST), (loose, "/setup")]))
                self.assertTrue(check.problems(tree, HOST))

    def test_a_loose_path_beside_the_exact_one_fails(self):
        tree = manifest(setup_filter(data=[("scheme", "https"), ("host", HOST), ("path", "/setup"), ("pathPrefix", "/setup")]))
        self.assertTrue(check.problems(tree, HOST))

    def test_a_second_loose_filter_for_the_host_fails(self):
        loose = setup_filter(data=[("scheme", "https"), ("host", HOST), ("pathPrefix", "/")])
        self.assertTrue(check.problems(manifest(setup_filter(), loose), HOST))

    def test_a_port_fails(self):
        tree = manifest(setup_filter(data=[("scheme", "https"), ("host", HOST), ("port", "8443"), ("path", "/setup")]))
        self.assertTrue(check.problems(tree, HOST))

    def test_a_second_path_or_scheme_fails(self):
        for data in (
            [("scheme", "https"), ("host", HOST), ("path", "/setup"), ("path", "/other")],
            [("scheme", "https"), ("scheme", "http"), ("host", HOST), ("path", "/setup")],
        ):
            with self.subTest(data):
                self.assertTrue(check.problems(manifest(setup_filter(data=data)), HOST))

    def test_view_default_and_browsable_are_required(self):
        for kwargs in (
            {"actions": ()},
            {"actions": ("android.intent.action.SEND",)},
            {"categories": ("android.intent.category.BROWSABLE",)},
            {"categories": ("android.intent.category.DEFAULT",)},
            {"verify": False},
        ):
            with self.subTest(kwargs):
                self.assertTrue(check.problems(manifest(setup_filter(**kwargs)), HOST))

    def test_only_main_activity_counts(self):
        tree = manifest(setup_filter(), activity="io.wolftown.kaiku.OtherActivity")
        self.assertTrue(check.problems(tree, HOST))

    def test_cli_exit_codes(self):
        def run(tree: str, *args: str) -> int:
            return subprocess.run([sys.executable, str(CLI), *args], input=tree, text=True, capture_output=True).returncode

        self.assertEqual(0, run(manifest(setup_filter()), HOST))
        prefix = manifest(setup_filter(data=[("scheme", "https"), ("host", HOST), ("pathPrefix", "/setup")]))
        self.assertEqual(1, run(prefix, HOST))
        self.assertEqual(2, run(manifest(setup_filter())))


if __name__ == "__main__":
    unittest.main()
