#!/usr/bin/env python3
"""Frontend bundle-size budget gate (Phase 8 performance budgets).

Measures the gzipped size of the INITIAL payload — every asset referenced
from ``client/dist/index.html`` (entry scripts, modulepreloads, stylesheets).
Lazy-loaded route/feature chunks are deliberately excluded: they don't block
startup, and the <3s startup target is what these budgets proxy.

Budgets fail CI loudly when exceeded. They carry ~25% headroom over the
2026-06-12 baseline (236 KB JS / 16 KB CSS gz) — a breach means a dependency
or import graph change moved real weight into the critical path, not normal
feature growth. Raise a budget only with a PR that explains the cost.

Run after `bun run build`:
    python3 scripts/check_bundle_budget.py
"""

from __future__ import annotations

import gzip
import re
import sys
from pathlib import Path

DIST = Path("client/dist")

# Budgets (gzipped bytes)
INITIAL_JS_BUDGET = 300_000
INITIAL_CSS_BUDGET = 25_000
LARGEST_CHUNK_BUDGET = 170_000


def gz_size(path: Path) -> int:
    return len(gzip.compress(path.read_bytes(), 6))


def main() -> int:
    index = DIST / "index.html"
    if not index.exists():
        print(f"ERROR: {index} not found — run `bun run build` in client/ first")
        return 1

    assets = re.findall(r'(?:src|href)="/?(assets/[^"]+)"', index.read_text())
    if not assets:
        print("ERROR: no assets referenced from index.html — build layout changed?")
        return 1

    total_js = 0
    total_css = 0
    largest_chunk = (0, "")
    for ref in assets:
        path = DIST / ref
        if not path.exists():
            print(f"ERROR: index.html references missing asset {ref}")
            return 1
        size = gz_size(path)
        if ref.endswith(".js"):
            total_js += size
            if size > largest_chunk[0]:
                largest_chunk = (size, ref)
        elif ref.endswith(".css"):
            total_css += size

    print(f"Initial JS  (gz): {total_js:>8} / {INITIAL_JS_BUDGET} budget")
    print(f"Initial CSS (gz): {total_css:>8} / {INITIAL_CSS_BUDGET} budget")
    print(
        f"Largest chunk (gz): {largest_chunk[0]:>6} / {LARGEST_CHUNK_BUDGET} budget"
        f"  ({largest_chunk[1]})"
    )

    failures = []
    if total_js > INITIAL_JS_BUDGET:
        failures.append(
            f"initial JS payload {total_js} gz exceeds budget {INITIAL_JS_BUDGET}"
        )
    if total_css > INITIAL_CSS_BUDGET:
        failures.append(
            f"initial CSS payload {total_css} gz exceeds budget {INITIAL_CSS_BUDGET}"
        )
    if largest_chunk[0] > LARGEST_CHUNK_BUDGET:
        failures.append(
            f"largest entry chunk {largest_chunk[1]} is {largest_chunk[0]} gz, "
            f"budget {LARGEST_CHUNK_BUDGET}"
        )

    if failures:
        print("\nBUNDLE BUDGET EXCEEDED:")
        for f in failures:
            print(f"  ✗ {f}")
        print(
            "\nA budget breach means real weight moved into the startup-critical"
            "\npath. Check for: a new dependency imported eagerly, a lazy route"
            "\nchunk that became static, or a util pulled into the entry graph."
            "\nIf the cost is intentional, raise the budget in"
            "\nscripts/check_bundle_budget.py in the same PR and justify it."
        )
        return 1

    print("Bundle budgets OK.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
