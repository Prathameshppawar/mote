#!/usr/bin/env bash
# Creates or updates the labels in .github/labels.yml on the GitHub repository.
#
#   scripts/sync-labels.sh                 # repository of the current checkout
#   scripts/sync-labels.sh owner/repo
#
# Requires the GitHub CLI (gh), authenticated with access to the repository.
# Existing labels are updated in place; labels not in the file are left alone.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
repo_args=()
if [ $# -gt 0 ]; then
  repo_args=(--repo "$1")
fi

name="" color="" description=""
flush() {
  if [ -n "$name" ]; then
    gh label create "$name" --color "$color" --description "$description" --force "${repo_args[@]}"
    echo "✓ $name"
  fi
  name="" color="" description=""
}

while IFS= read -r line; do
  case "$line" in
    "- name: "*) flush; name="${line#- name: }" ;;
    "  color: "*) color="${line#  color: }" ;;
    "  description: "*) description="${line#  description: }" ;;
  esac
done < "$root/.github/labels.yml"
flush
