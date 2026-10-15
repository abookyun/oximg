#!/usr/bin/env bash
# SessionStart: point this clone at .githooks/ so pre-push gates agent
# pushes. Leaves a person's hooks alone — an existing core.hooksPath, or
# hooks installed in the default directory, which core.hooksPath would
# silently switch off — and says so, since the gate is then off.
set -uo pipefail

root=${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null)} || exit 0
cd "$root" 2>/dev/null || exit 0
[[ -d .githooks ]] || exit 0

current=$(git config --get core.hooksPath 2>/dev/null || true)
[[ "$current" == ".githooks" ]] && exit 0
if [[ -n "$current" ]]; then
  echo "core.hooksPath is $current, so .githooks/pre-push is not active: run scripts/check.sh full before every push."
  exit 0
fi

default_dir="$(git rev-parse --git-common-dir)/hooks"
for f in "$default_dir"/*; do
  if [[ -f "$f" && -x "$f" && "$f" != *.sample ]]; then
    echo "$default_dir has its own hooks, so .githooks/pre-push is not active: run scripts/check.sh full before every push."
    exit 0
  fi
done

git config core.hooksPath .githooks
exit 0
