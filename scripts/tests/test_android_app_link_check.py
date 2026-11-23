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


def manifest(
    *filters: str,
    activity: str = "io.wolftown.kaiku.ui.MainActivity",
    alias: tuple[str, ...] = (),
    alias_target: str = "io.wolftown.kaiku.ui.MainActivity",
    package: str | None = None,
    exported: str | None = "true",
    enabled: str | None = None,
    app_enabled: str | None = None,
) -> str:
    """An aapt2 xmltree dump shaped like the real one, with the given intent filters on the activity.

    alias: intent filters of an activity-alias that targets alias_target.
    """
    lines = [
        "N: android=http://schemas.android.com/apk/res/android (line=2)",
        "  E: manifest (line=2)",
        *([attr(4, "package", package)] if package else []),
        "      E: application (line=10)",
        *([attr(8, "enabled", app_enabled, raw=False)] if app_enabled is not None else []),
        "          E: activity (line=60)",
        attr(12, "name", activity),
        *([attr(12, "exported", exported, raw=False)] if exported is not None else []),
        *([attr(12, "enabled", enabled, raw=False)] if enabled is not None else []),
    ]
    lines += filters
    if alias:
        lines += [
            "          E: activity-alias (line=90)",
            attr(12, "name", "io.wolftown.kaiku.ui.Links"),
            attr(12, "targetActivity", alias_target),
        ]
        lines += alias
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

    def test_a_filter_for_the_whole_host_fails_beside_the_exact_one(self):
        # scheme + host and no path: Android matches every path on the host.
        whole_host = setup_filter(data=[("scheme", "https"), ("host", HOST)])
        self.assertEqual([f"https filter for {HOST} without a path"], check.problems(manifest(setup_filter(), whole_host), HOST))
        self.assertTrue(check.problems(manifest(whole_host, setup_filter(), callback_filter()), HOST))
        self.assertTrue(check.problems(manifest(whole_host), HOST))
        # Without autoVerify too: it still claims every path for the app chooser.
        self.assertTrue(check.problems(manifest(setup_filter(), setup_filter(data=[("scheme", "https"), ("host", HOST)], verify=False)), HOST))
        # Other hosts' filters and exact paths of this host stay allowed.
        other_host = setup_filter(data=[("scheme", "https"), ("host", "other.example.org")])
        self.assertEqual([], check.problems(manifest(setup_filter(), other_host, callback_filter()), HOST))

    def test_a_wildcard_host_that_covers_the_host_fails(self):
        # Android matches a host that starts with * against every host that ends with the rest.
        for wild in ("*.example.com", "*", "*example.com", "*.com"):
            for data in (
                [("scheme", "https"), ("host", wild)],
                [("scheme", "https"), ("host", wild), ("path", "/setup")],
                [("scheme", "https"), ("host", HOST), ("host", wild), ("path", "/setup")],
            ):
                with self.subTest(wild=wild, data=data):
                    self.assertTrue(check.problems(manifest(setup_filter(), setup_filter(data=data, verify=False)), HOST))
                    self.assertTrue(check.problems(manifest(setup_filter(data=data)), HOST))
        # A wildcard that cannot match this host is another host's business.
        elsewhere = setup_filter(data=[("scheme", "https"), ("host", "*.example.org"), ("path", "/x")], verify=False)
        self.assertEqual([], check.problems(manifest(setup_filter(), elsewhere), HOST))

    def test_an_https_filter_without_a_host_fails(self):
        # Without a host Android matches every https link, whatever paths the filter names.
        for data in (
            [("scheme", "https")],
            [("scheme", "https"), ("path", "/setup")],
            [("scheme", "https"), ("pathPrefix", "/setup")],
        ):
            with self.subTest(data):
                hostless = setup_filter(data=data, verify=False)
                self.assertTrue(check.problems(manifest(setup_filter(), hostless), HOST))
                self.assertTrue(check.problems(manifest(hostless), HOST))
        # The app's own scheme without a host is not an https filter.
        own = setup_filter(data=[("scheme", "kaiku"), ("host", "setup")], verify=False)
        self.assertEqual([], check.problems(manifest(setup_filter(), own), HOST))

    def test_an_alias_of_main_activity_is_checked_like_it(self):
        whole_host = setup_filter(data=[("scheme", "https"), ("host", HOST)], verify=False)
        self.assertEqual(
            [f"https filter for {HOST} without a path (on an alias of MainActivity)"],
            check.problems(manifest(setup_filter(), alias=(whole_host,)), HOST),
        )
        for data in (
            [("scheme", "https"), ("host", HOST), ("pathPrefix", "/")],
            [("scheme", "https"), ("host", "*.example.com"), ("path", "/setup")],
            [("scheme", "https")],
        ):
            with self.subTest(data):
                self.assertTrue(check.problems(manifest(setup_filter(), alias=(setup_filter(data=data, verify=False),)), HOST))
        # An alias with exact paths of its own stays allowed, but it does not stand in for the
        # setup filter on MainActivity itself.
        self.assertEqual([], check.problems(manifest(setup_filter(), alias=(callback_filter(), setup_filter())), HOST))
        self.assertTrue(check.problems(manifest(alias=(setup_filter(),)), HOST))
        # An alias of another activity is not looked at.
        other = manifest(setup_filter(), alias=(whole_host,), alias_target="io.wolftown.kaiku.OtherActivity")
        self.assertEqual([], check.problems(other, HOST))

    def test_a_scheme_specific_part_matcher_fails_whatever_host_and_path(self):
        # Android matches a scheme-specific part before hosts and paths: such a filter can reach
        # links of the host whatever its own host and paths say.
        for ssp in ("ssp", "sspPrefix", "sspPattern", "sspAdvancedPattern", "sspSuffix"):
            for data in (
                [("scheme", "https"), ("host", HOST), ("path", "/callback"), (ssp, f"//{HOST}/")],
                [("scheme", "https"), ("host", "other.example.org"), ("path", "/x"), (ssp, f"//{HOST}/")],
                [("scheme", "https"), ("host", HOST), ("path", "/setup"), (ssp, f"//{HOST}/setup")],
            ):
                with self.subTest(ssp=ssp, data=data):
                    widened = setup_filter(data=data, verify=False)
                    self.assertTrue(check.problems(manifest(setup_filter(), widened), HOST))
                    self.assertTrue(check.problems(manifest(setup_filter(), alias=(widened,)), HOST))

    def test_the_setup_filter_counts_only_on_an_exported_enabled_activity(self):
        # A filter on an activity other apps cannot start, or that is turned off, opens nothing.
        for kwargs in (
            {"exported": "false"},
            {"exported": "0x0"},
            {"exported": None},
            {"enabled": "false"},
            {"app_enabled": "false"},
        ):
            with self.subTest(kwargs):
                self.assertTrue(check.problems(manifest(setup_filter(), **kwargs), HOST))
        self.assertEqual([], check.problems(manifest(setup_filter(), exported="0xffffffff", enabled="true", app_enabled="true"), HOST))

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

    def test_main_activity_is_named_in_full(self):
        # Another package's activity with the same simple name is not the app's MainActivity.
        other = manifest(setup_filter(), activity="example.other.ui.MainActivity", package="io.wolftown.kaiku")
        self.assertTrue(check.problems(other, HOST))
        other_alias = manifest(
            activity="example.other.Holder",
            alias=(setup_filter(),),
            alias_target="example.other.ui.MainActivity",
            package="io.wolftown.kaiku",
        )
        self.assertTrue(check.problems(other_alias, HOST))
        # A name relative to the manifest's package is resolved against it.
        self.assertEqual([], check.problems(manifest(setup_filter(), activity=".ui.MainActivity", package="io.wolftown.kaiku"), HOST))
        relative_other = manifest(setup_filter(), activity=".ui.MainActivity", package="example.other")
        self.assertTrue(check.problems(relative_other, HOST))

    def test_a_relative_filter_group_fails(self):
        # Android before 15 ignores the group: the filter would match every https path on the host.
        group = "\n".join([
            "                  E: uri-relative-filter-group (line=80)",
            "                      E: data (line=81)",
            attr(24, "path", "/setup"),
        ])
        only_in_group = manifest(setup_filter(data=[("scheme", "https"), ("host", HOST)]) + "\n" + group)
        found = check.problems(only_in_group, HOST)
        self.assertTrue(any("uri-relative-filter-group" in p for p in found), found)
        beside_exact = manifest(setup_filter() + "\n" + group.replace("/setup", "/other"))
        found = check.problems(beside_exact, HOST)
        self.assertTrue(any("uri-relative-filter-group" in p for p in found), found)
        # A group in a filter for another host is not this check's business.
        other = manifest(setup_filter(), setup_filter(data=[("scheme", "https"), ("host", "other.example.org"), ("path", "/x")], verify=False) + "\n" + group)
        self.assertEqual([], check.problems(other, HOST))

    def test_cli_exit_codes(self):
        def run(tree: str, *args: str) -> int:
            return subprocess.run([sys.executable, str(CLI), *args], input=tree, text=True, capture_output=True).returncode

        self.assertEqual(0, run(manifest(setup_filter()), HOST))
        prefix = manifest(setup_filter(data=[("scheme", "https"), ("host", HOST), ("pathPrefix", "/setup")]))
        self.assertEqual(1, run(prefix, HOST))
        whole_host = manifest(setup_filter(), setup_filter(data=[("scheme", "https"), ("host", HOST)]))
        self.assertEqual(1, run(whole_host, HOST))
        wildcard = manifest(setup_filter(), setup_filter(data=[("scheme", "https"), ("host", "*.example.com")], verify=False))
        self.assertEqual(1, run(wildcard, HOST))
        hostless = manifest(setup_filter(), setup_filter(data=[("scheme", "https")], verify=False))
        self.assertEqual(1, run(hostless, HOST))
        alias = manifest(setup_filter(), alias=(setup_filter(data=[("scheme", "https"), ("host", HOST)], verify=False),))
        self.assertEqual(1, run(alias, HOST))
        self.assertEqual(2, run(manifest(setup_filter())))


if __name__ == "__main__":
    unittest.main()
