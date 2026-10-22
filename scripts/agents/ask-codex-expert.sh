#!/usr/bin/env bash
# Ask a Codex-backed expert a focused question in a read-only sandbox.
# Usage: scripts/agents/ask-codex-expert.sh <expert> "<question>" [model] [effort]
# Experts: see docs/developer-guide/agents/experts/*.md
set -euo pipefail

expert="${1:?expert name required}"
question="${2:?question required}"
model="${3:-gpt-6-astra}"
effort="${4:-max}"

repo_root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
brief="$repo_root/docs/developer-guide/agents/experts/$expert.md"
[[ -f "$brief" ]] || { echo "unknown expert: $expert" >&2; exit 2; }

prompt="$(cat "$brief")

Question:
$question"
out="$(mktemp)"
trap 'rm -f "$out"' EXIT
codex exec -s read-only -m "$model" -c "model_reasoning_effort=\"$effort\"" \
  -C "$repo_root" -o "$out" "$prompt" >/dev/null 2>&1
cat "$out"
