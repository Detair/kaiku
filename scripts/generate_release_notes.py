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


# A GitHub handle after "@" that is not part of an e-mail address or URL.
MENTION_RE = re.compile(r"(?<![\w@./`])@([A-Za-z0-9][A-Za-z0-9-]*)")


def neutralize_mentions(line: str) -> str:
    """Wrap ``@word`` outside inline code in backticks.

    GitHub links every ``@handle`` in a release body to that account, notifies
    it and lists it under the release's contributors. Changelog prose such as
    "@everyone" or "@mention" names a feature, not a person, so it is rendered
    as code instead.
    """
    parts = re.split(r"(`[^`]*`)", line)
    return "".join(
        part if part.startswith("`") else MENTION_RE.sub(r"`@\1`", part) for part in parts
    )


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
            items[current_section].append(neutralize_mentions(line))

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


# Each section keeps at least this many characters of entries (when the
# budget allows), so a small section is not rounded down to nothing.
SECTION_FLOOR_CHARS = 1000


def fit_release_notes(
    header: str,
    section_items: dict[str, list[str]],
    max_chars: int,
    changelog_url: str,
) -> str:
    """Render the sections within ``max_chars``, dropping trailing entries.

    GitHub rejects release bodies above 125,000 characters, and a release
    candidate's notes come from the whole ``[Unreleased]`` section. Each
    section first gets up to ``SECTION_FLOOR_CHARS``, then a share of the
    rest proportional to its size, and keeps its entries in order until its
    share runs out, so no category (above all Security) loses all of its
    entries to the larger ones. A trimmed section ends with a pointer to the
    full changelog.
    """
    more_line = "- ... {n} more, see the [full changelog]({url})"
    entries_of = {section: section_items.get(section, []) for section in SECTIONS}
    sizes = {s: sum(len(e) + 1 for e in entries) for s, entries in entries_of.items()}
    # Every section always shows its heading and, when trimmed, the pointer
    # (sized for its real entry count).
    reserve = sum(
        len(f"### {s}\n") + len(more_line.format(n=len(entries_of[s]), url=changelog_url)) + 2
        for s in SECTIONS
    )
    available = max_chars - len(header) - reserve
    if available <= 0:
        raise ValueError(f"--max-chars {max_chars} is too small for the notes header")

    total = sum(sizes.values())
    if total <= available:
        shares = sizes
    else:
        floors = {s: min(size, SECTION_FLOOR_CHARS) for s, size in sizes.items()}
        floor_total = sum(floors.values())
        if floor_total > available:
            floors = {s: 0 for s in SECTIONS}
            floor_total = 0
        rest, rest_total = available - floor_total, total - floor_total
        shares = {
            s: floors[s] + (sizes[s] - floors[s]) * rest // rest_total for s in SECTIONS
        }

    lines: list[str] = []
    for section in SECTIONS:
        lines.append(f"### {section}")
        entries = entries_of[section]
        remaining = shares[section]
        kept = 0
        for entry in entries:
            if len(entry) + 1 > remaining:
                break
            lines.append(entry)
            remaining -= len(entry) + 1
            kept += 1
        if not entries:
            lines.append("- None")
        elif kept < len(entries):
            lines.append(more_line.format(n=len(entries) - kept, url=changelog_url))
        lines.append("")
    return header + "\n".join(lines).rstrip() + "\n"


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
    parser.add_argument(
        "--max-chars",
        type=int,
        help="Trim entries so the notes stay within this many characters",
    )
    parser.add_argument(
        "--changelog-url",
        default="CHANGELOG.md",
        help="Link to the full changelog, shown when entries are trimmed",
    )
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
        if args.max_chars is not None and len(output) > args.max_chars:
            header = render_release_notes(
                args.version, roadmap_phase, roadmap_last_updated, {}, source
            ).split("### ", 1)[0]
            output = fit_release_notes(
                header, section_items, args.max_chars, args.changelog_url
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
