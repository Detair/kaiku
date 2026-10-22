#!/usr/bin/env python3
"""Generate standardized milestone release notes from CHANGELOG.md.

Uses the curated `## [<version>]` section when it exists (final release)
and falls back to `[Unreleased]` otherwise (release candidates, CI).
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


SECTIONS = ("Added", "Changed", "Deprecated", "Removed", "Fixed", "Security")


def read_text(path: Path) -> str:
    if not path.exists():
        raise FileNotFoundError(f"Missing file: {path}")
    return path.read_text(encoding="utf-8")


def extract_unreleased(changelog: str) -> str:
    match = re.search(
        r"^## \[Unreleased\]\n(?P<body>.*?)(?=^## \[|\Z)",
        changelog,
        flags=re.MULTILINE | re.DOTALL,
    )
    if not match:
        raise ValueError("CHANGELOG.md is missing a [Unreleased] section")
    return match.group("body")


def extract_section(changelog: str, version: str) -> str | None:
    """Return the body of `## [<version>]` (without the heading) or None."""
    match = re.search(
        rf"^## \[{re.escape(version)}\][^\n]*\n(?P<body>.*?)(?=^## \[|\Z)",
        changelog,
        flags=re.MULTILINE | re.DOTALL,
    )
    return match.group("body") if match else None


def parse_roadmap_metadata(roadmap: str) -> tuple[str, str]:
    phase_match = re.search(
        r"^\*\*Current Phase:\*\*\s*(.+)$", roadmap, flags=re.MULTILINE
    )
    date_match = re.search(
        r"^\*\*Last Updated:\*\*\s*(\d{4}-\d{2}-\d{2})$",
        roadmap,
        flags=re.MULTILINE,
    )
    if not phase_match or not date_match:
        raise ValueError(
            "Roadmap metadata is incomplete (Current Phase / Last Updated)"
        )
    return phase_match.group(1).strip(), date_match.group(1).strip()


def collect_section_items(unreleased: str) -> dict[str, list[str]]:
    items = {section: [] for section in SECTIONS}
    current_section: str | None = None

    for raw_line in unreleased.splitlines():
        line = raw_line.rstrip()
        heading_match = re.match(r"^###\s+(.+)$", line)
        if heading_match:
            heading = heading_match.group(1).strip()
            current_section = heading if heading in items else None
            continue

        if current_section and re.match(r"^\s*-\s+", line):
            items[current_section].append(line)

    return items


def render_release_notes(
    version: str,
    roadmap_phase: str,
    roadmap_last_updated: str,
    section_items: dict[str, list[str]],
    source: str = "[Unreleased]",
) -> str:
    lines: list[str] = []
    lines.append("## Milestone")
    lines.append(f"- Version: {version}")
    lines.append(f"- Roadmap phase: {roadmap_phase}")
    lines.append(f"- Roadmap last updated: {roadmap_last_updated}")
    lines.append("")
    lines.append("## Release Summary")
    lines.append(f"- Notes generated from `CHANGELOG.md` `{source}` entries.")
    lines.append("- Sections follow Keep a Changelog categories.")
    lines.append("")

    for section in SECTIONS:
        lines.append(f"### {section}")
        entries = section_items.get(section, [])
        if entries:
            lines.extend(entries)
        else:
            lines.append("- None")
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--version", required=True, help="Release version (for example v0.2.0)"
    )
    parser.add_argument(
        "--changelog",
        default="CHANGELOG.md",
        help="Path to changelog file",
    )
    parser.add_argument(
        "--roadmap",
        default="docs/project/roadmap.md",
        help="Path to roadmap file",
    )
    parser.add_argument("--output", required=True, help="Output markdown file path")
    args = parser.parse_args()

    try:
        changelog = read_text(Path(args.changelog))
        roadmap = read_text(Path(args.roadmap))
        semver = args.version.removeprefix("v")
        body = extract_section(changelog, semver)
        source = f"[{semver}]"
        if body is None:
            body = extract_unreleased(changelog)
            source = "[Unreleased]"
        roadmap_phase, roadmap_last_updated = parse_roadmap_metadata(roadmap)
        section_items = collect_section_items(body)
        output = render_release_notes(
            args.version,
            roadmap_phase,
            roadmap_last_updated,
            section_items,
            source,
        )
        output_path = Path(args.output)
        output_path.write_text(output, encoding="utf-8")
        print(f"Generated release notes at {output_path}")
        return 0
    except (FileNotFoundError, ValueError) as err:
        print(f"Error: {err}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
