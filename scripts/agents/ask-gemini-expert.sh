#!/usr/bin/env bash
# Ask a Gemini-backed expert a focused question, read-only.
# Usage: scripts/agents/ask-gemini-expert.sh <expert> "<question>" [model]
# Experts: see docs/developer-guide/agents/experts/*.md
set -euo pipefail

expert="${1:?expert name required}"
question="${2:?question required}"
model="${3:-gemini-4-argon-high}"

repo_root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
brief="$repo_root/docs/developer-guide/agents/experts/$expert.md"
[[ -f "$brief" ]] || { echo "unknown expert: $expert" >&2; exit 2; }

cd "$repo_root"
# Headless agy auto-denies shell commands, so steer it to built-in tools.
prompt="$(cat "$brief")

Tooling: shell commands are unavailable (auto-denied); use only built-in
file read, list and search tools.

Question:
$question"
exec </dev/null agy --mode plan --model "$model" --print-timeout 900s --print="$prompt"
