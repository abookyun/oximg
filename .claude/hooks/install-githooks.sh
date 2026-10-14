#!/usr/bin/env bash
# SessionStart: point this clone at .githooks/ so pre-push gates agent
# pushes. Leaves an existing core.hooksPath alone (a person's global or
# local hooks win) and says so, since the gate is then off.
set -uo pipefail

root=${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null)} || exit 0
cd "$root" 2>/dev/null || exit 0
[[ -d .githooks ]] || exit 0

current=$(git config --get core.hooksPath 2>/dev/null || true)
if [[ -z "$current" ]]; then
  git config core.hooksPath .githooks
elif [[ "$current" != ".githooks" ]]; then
  echo "core.hooksPath is $current, so .githooks/pre-push is not active: run scripts/check.sh full before every push."
fi
exit 0
