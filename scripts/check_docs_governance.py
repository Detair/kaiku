#!/usr/bin/env python3
"""Validate roadmap/changelog consistency and plan lifecycle metadata."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ROADMAP_PATH = ROOT / "docs/project/roadmap.md"
CHANGELOG_PATH = ROOT / "CHANGELOG.md"
PLAN_LIFECYCLE_PATH = ROOT / "docs/plans/PLAN_LIFECYCLE.md"
RELEASE_TEMPLATE_PATH = ROOT / "docs/project/RELEASE_NOTES_TEMPLATE.md"

LIFECYCLE_STATUSES = {"Active", "Superseded", "Archived"}
REQUIRED_TEMPLATE_HEADINGS = [
    "## Milestone",
    "## Release Summary",
    "### Added",
    "### Changed",
    "### Deprecated",
    "### Removed",
    "### Fixed",
    "### Security",
]


def read_text(path: Path, errors: list[str]) -> str:
    if not path.exists():
        errors.append(f"Missing required file: {path.relative_to(ROOT)}")
        return ""
    return path.read_text(encoding="utf-8")


def extract_unreleased(changelog: str, errors: list[str]) -> str:
    match = re.search(
        r"^## \[Unreleased\]\n(?P<body>.*?)(?=^## \[|\Z)",
        changelog,
        flags=re.MULTILINE | re.DOTALL,
    )
    if not match:
        errors.append("CHANGELOG.md is missing a [Unreleased] section")
        return ""
    return match.group("body")


def parse_roadmap_metadata(roadmap: str, errors: list[str]) -> tuple[str, str]:
    phase_match = re.search(
        r"^\*\*Current Phase:\*\*\s*(.+)$", roadmap, flags=re.MULTILINE
    )
    date_match = re.search(
        r"^\*\*Last Updated:\*\*\s*(\d{4}-\d{2}-\d{2})$",
        roadmap,
        flags=re.MULTILINE,
    )

    if not phase_match:
        errors.append("Roadmap is missing '**Current Phase:**' metadata")
    if not date_match:
        errors.append("Roadmap is missing '**Last Updated:**' metadata")

    return (
        phase_match.group(1).strip() if phase_match else "",
        date_match.group(1).strip() if date_match else "",
    )


def validate_roadmap_links(roadmap: str, errors: list[str]) -> None:
    rel_links = sorted(set(re.findall(r"\((\.\./plans/[^)]+\.md)\)", roadmap)))
    for rel in rel_links:
        target = (ROADMAP_PATH.parent / rel).resolve()
        if not target.exists():
            errors.append(f"Roadmap link target does not exist: {rel}")


def validate_roadmap_alignment_block(
    roadmap_phase: str,
    roadmap_last_updated: str,
    unreleased: str,
    errors: list[str],
) -> None:
    block_match = re.search(
        r"^### Roadmap Alignment\n(?P<body>.*?)(?=^### |\Z)",
        unreleased,
        flags=re.MULTILINE | re.DOTALL,
    )
    if not block_match:
        errors.append("CHANGELOG [Unreleased] is missing '### Roadmap Alignment' block")
        return

    block = block_match.group("body")
    phase_match = re.search(
        r"^- Current roadmap phase:\s*(.+)$",
        block,
        flags=re.MULTILINE,
    )
    updated_match = re.search(
        r"^- Roadmap last updated:\s*(\d{4}-\d{2}-\d{2})$",
        block,
        flags=re.MULTILINE,
    )

    if not phase_match:
        errors.append("Roadmap Alignment block missing '- Current roadmap phase:'")
    if not updated_match:
        errors.append("Roadmap Alignment block missing '- Roadmap last updated:'")

    if phase_match and phase_match.group(1).strip() != roadmap_phase:
        errors.append(
            "Roadmap/Changelog phase mismatch: "
            f"roadmap='{roadmap_phase}', changelog='{phase_match.group(1).strip()}'"
        )
    if updated_match and updated_match.group(1).strip() != roadmap_last_updated:
        errors.append(
            "Roadmap/Changelog last-updated mismatch: "
            f"roadmap='{roadmap_last_updated}', changelog='{updated_match.group(1).strip()}'"
        )


def parse_table_rows(markdown: str) -> list[list[str]]:
    rows: list[list[str]] = []
    for line in markdown.splitlines():
        line = line.strip()
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if len(cells) < 4:
            continue
        if cells[0] == "Plan" or set(cells[0]) == {"-"}:
            continue
        rows.append(cells[:4])
    return rows


def parse_code_path(cell: str) -> str | None:
    cell = cell.strip()
    if cell == "-":
        return "-"
    if cell.startswith("`") and cell.endswith("`"):
        return cell[1:-1]
    return None


def validate_plan_lifecycle(errors: list[str]) -> None:
    lifecycle = read_text(PLAN_LIFECYCLE_PATH, errors)
    if not lifecycle:
        return

    rows = parse_table_rows(lifecycle)
    if not rows:
        errors.append("PLAN_LIFECYCLE.md must define at least one lifecycle row")
        return

    superseded_count = 0

    for plan_cell, status, superseded_by_cell, _notes in rows:
        plan_rel = parse_code_path(plan_cell)
        if not plan_rel or plan_rel == "-":
            errors.append(f"Invalid plan path cell in PLAN_LIFECYCLE.md: {plan_cell}")
            continue

        if status not in LIFECYCLE_STATUSES:
            errors.append(
                f"Invalid lifecycle status '{status}' for plan '{plan_rel}' "
                f"(allowed: {', '.join(sorted(LIFECYCLE_STATUSES))})"
            )

        plan_path = ROOT / "docs/plans" / plan_rel
        if not plan_path.exists():
            errors.append(
                f"Lifecycle entry references missing file: docs/plans/{plan_rel}"
            )
            continue

        plan_text = plan_path.read_text(encoding="utf-8")

        if status == "Superseded":
            superseded_count += 1
            if "**Lifecycle:** Superseded" not in plan_text:
                errors.append(
                    f"Superseded plan missing lifecycle marker: docs/plans/{plan_rel}"
                )

            superseded_rel = parse_code_path(superseded_by_cell)
            if not superseded_rel or superseded_rel == "-":
                errors.append(
                    f"Superseded plan must define 'Superseded By' target: docs/plans/{plan_rel}"
                )
                continue

            superseded_path = ROOT / "docs/plans" / superseded_rel
            if not superseded_path.exists():
                errors.append(
                    f"Superseded target does not exist: docs/plans/{superseded_rel}"
                )

            if superseded_rel not in plan_text:
                errors.append(
                    f"Superseded source missing explicit target reference '{superseded_rel}': "
                    f"docs/plans/{plan_rel}"
                )

        if status == "Active" and "**Lifecycle:** Active" not in plan_text:
            errors.append(
                f"Active plan missing lifecycle marker: docs/plans/{plan_rel}"
            )

    if superseded_count == 0:
        errors.append("PLAN_LIFECYCLE.md should include at least one Superseded entry")


def validate_release_template(errors: list[str]) -> None:
    template = read_text(RELEASE_TEMPLATE_PATH, errors)
    if not template:
        return
    for heading in REQUIRED_TEMPLATE_HEADINGS:
        if heading not in template:
            errors.append(
                f"Release notes template missing required heading '{heading}'"
            )


def check_observability_plan_linkage(errors: list[str]) -> None:
    """Verify observability plan linkage: plan file, ops docs, and roadmap reference."""
    plan_path = ROOT / "docs/plans/2026-02-27-phase-7-observability-telemetry-task-plan.md"
    contract_path = ROOT / "docs/ops/observability-contract.md"
    runbook_path = ROOT / "docs/ops/observability-runbook.md"

    if not plan_path.exists():
        errors.append(
            "Missing observability implementation plan: "
            "docs/plans/2026-02-27-phase-7-observability-telemetry-task-plan.md"
        )
    else:
        plan_text = plan_path.read_text(encoding="utf-8")
        if "**Status:**" not in plan_text:
            errors.append(
                "Observability plan missing lifecycle metadata '**Status:**': "
                "docs/plans/2026-02-27-phase-7-observability-telemetry-task-plan.md"
            )

    if not contract_path.exists():
        errors.append("Missing observability contract: docs/ops/observability-contract.md")

    if not runbook_path.exists():
        errors.append("Missing observability runbook: docs/ops/observability-runbook.md")

    roadmap_text = ROADMAP_PATH.read_text(encoding="utf-8") if ROADMAP_PATH.exists() else ""
    if "[Infra] SaaS Observability & Telemetry" not in roadmap_text:
        errors.append(
            "Roadmap does not reference '[Infra] SaaS Observability & Telemetry'"
        )



def main() -> int:
    errors: list[str] = []

    roadmap = read_text(ROADMAP_PATH, errors)
    changelog = read_text(CHANGELOG_PATH, errors)

    if roadmap:
        validate_roadmap_links(roadmap, errors)
    if roadmap and changelog:
        roadmap_phase, roadmap_last_updated = parse_roadmap_metadata(roadmap, errors)
        unreleased = extract_unreleased(changelog, errors)
        if roadmap_phase and roadmap_last_updated and unreleased:
            validate_roadmap_alignment_block(
                roadmap_phase,
                roadmap_last_updated,
                unreleased,
                errors,
            )

    validate_plan_lifecycle(errors)
    validate_release_template(errors)
    check_observability_plan_linkage(errors)

    if errors:
        print("Docs governance checks failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print("Docs governance checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
