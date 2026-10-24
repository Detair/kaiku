#!/usr/bin/env bash
# Ask an expert through OpenCode (read-only `plan` agent), e.g. Go-subscription models.
# Usage: scripts/agents/ask-opencode-expert.sh <expert> "<question>" [provider/model]
# Experts: see docs/developer-guide/agents/experts/*.md
set -euo pipefail

expert="${1:?expert name required}"
question="${2:?question required}"
model="${3:-opencode-go/kimi-k3}"

repo_root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
brief="$repo_root/docs/developer-guide/agents/experts/$expert.md"
[[ -f "$brief" ]] || { echo "unknown expert: $expert" >&2; exit 2; }

cd "$repo_root"
exec "${OPENCODE_BIN:-$HOME/.opencode/bin/opencode}" run --agent plan -m "$model" \
  "$(cat "$brief")

Question:
$question"
