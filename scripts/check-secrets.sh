#!/usr/bin/env bash
# Fails if a tracked file contains something shaped like a real credential.
#
# Patterns match provider key *signatures* (for example, every Groq key embeds
# "WGdyb3FY" at a fixed offset), so the obviously fake keys used in tests do
# not trip it. Run from anywhere inside the repository.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

patterns=(
  'gsk_[A-Za-z0-9]{20}WGdyb3FY[A-Za-z0-9]{24}'          # Groq
  'sk-[A-Za-z0-9_-]{0,180}T3BlbkFJ[A-Za-z0-9_-]{8,}'     # OpenAI
  'sk-ant-(api|admin)[0-9]{2}-[A-Za-z0-9_-]{80,}'        # Anthropic
  '(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36}'                # GitHub tokens
  'github_pat_[A-Za-z0-9_]{80,}'                         # GitHub fine-grained tokens
  'xox[baprs]-[0-9]{10,}-[0-9A-Za-z-]{10,}'              # Slack
  'AKIA[0-9A-Z]{16}'                                     # AWS access key id
  '-----BEGIN ([A-Z]+ )?PRIVATE KEY-----'                # PEM private keys
)

args=()
for pattern in "${patterns[@]}"; do
  args+=(-e "$pattern")
done

if git grep -nIE "${args[@]}" -- . ':!scripts/check-secrets.sh'; then
  echo "::error::Possible credential committed (see matches above). Remove it and rotate the key." >&2
  exit 1
fi
echo "✓ no credentials found in tracked files"
