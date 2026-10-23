"""Tests for scripts/latest_tag_decision.py."""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from latest_tag_decision import classify_pull_error, decide, main  # noqa: E402


class DecideTests(unittest.TestCase):
    def test_absent_pushes(self):
        self.assertEqual(decide("1.0.0", None)[0], "push")

    def test_older_current_pushes(self):
        self.assertEqual(decide("1.1.0", "1.0.0")[0], "push")
        self.assertEqual(decide("1.0.0", "1.0.0-rc.3")[0], "push")

    def test_newer_current_skips(self):
        self.assertEqual(decide("1.0.0", "1.1.0")[0], "skip")

    def test_equal_pushes(self):
        self.assertEqual(decide("1.0.0", "1.0.0")[0], "push")

    def test_unparsable_label_skips(self):
        for label in ("", "garbage", "<no value>"):
            with self.subTest(label=label):
                self.assertEqual(decide("1.0.0", label)[0], "skip")

    def test_cli_invalid_new_version(self):
        self.assertEqual(main(["decide", "--new", "bad", "--absent"]), 2)


class ClassifyTests(unittest.TestCase):
    def test_registry_absent_responses(self):
        for text in (
            "Error response from daemon: manifest unknown",
            "Error response from daemon: not found: manifest unknown: manifest unknown",
            "Error response from daemon: name unknown: repository name not known to registry",
        ):
            with self.subTest(text=text):
                self.assertEqual(classify_pull_error(text), "absent")

    def test_unrelated_errors(self):
        for text in (
            "Error response from daemon: Get https://ghcr.io/v2/: dial tcp: lookup ghcr.io: no such host",
            "Error response from daemon: unauthorized: authentication required",
            "docker: command not found",
            "Error response from daemon: pull access denied, repository does not exist or may require authorization",
            "",
        ):
            with self.subTest(text=text):
                self.assertEqual(classify_pull_error(text), "error")


if __name__ == "__main__":
    unittest.main()
