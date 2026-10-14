#!/usr/bin/env bash
# PreToolUse(Bash): two rules from AGENTS.md, enforced.
#
# 1. No killing oximg by name: other sessions on the host run their own
#    servers. Tear down only the pid you started.
# 2. `gh pr create` needs `scripts/check.sh full` to have passed on HEAD
#    with a clean tree, unless the branch touches only agent files,
#    site/, and Markdown the build does not read.
#
# Only command positions count: heredoc bodies and quoted strings are
# dropped first (shell_text.py), so a commit message that names
# `gh pr create` is not a PR. A guardrail, not a sandbox — `bash -c '…'`
# walks past it.
#
# Exit 2 blocks the command and shows stderr to the agent.
set -uo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
input=$(cat)
if command -v python3 >/dev/null 2>&1; then
  cmd=$(python3 "$here/shell_text.py" code <<<"$input")
  # Kill patterns are usually quoted (`pkill -f 'oximg serve'`), so the
  # kill rule matches with quotes kept; only heredoc bodies go.
  kill_text=$(python3 "$here/shell_text.py" heredoc <<<"$input")
elif command -v jq >/dev/null 2>&1; then
  cmd=$(jq -r '.tool_input.command // empty' <<<"$input")
  kill_text=$cmd
else
  exit 0
fi
[[ -n "$cmd" ]] || exit 0

# Start of a line, or after ; & | ( or $( — then optional sudo.
at='(^|[;&|(]|\$\()[[:space:]]*(sudo[[:space:]]+)?'

if grep -Eq "${at}(killall|pkill)[^;&|]*oximg" <<<"$kill_text" ||
  grep -Eq "${at}kill[^;&|]*\\\$\\([[:space:]]*pgrep[^)]*oximg" <<<"$kill_text"; then
  echo "Blocked: never kill oximg by name — other sessions on this host run their own. Kill the pid you started (the oximg-ctl wrapper for serve)." >&2
  exit 2
fi

grep -Eq "${at}gh[[:space:]]+pr[[:space:]]+create([^[:alnum:]_-]|$)" <<<"$cmd" || exit 0

root=${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}
cd "$root" || exit 0

# Agent files, the site (CI skips it), and prose cannot break the build —
# except the Markdown src/config.rs reads with include_str! and checks
# against the code: README.md and docs/features/.
base=$(git merge-base origin/main HEAD 2>/dev/null || true)
if [[ -n "$base" ]]; then
  code=$(git diff --name-only "$base" HEAD | awk '
    /^README\.md$/ || /^docs\/features\// { print; next }
    /^\.claude\// || /^\.cursor\// || /^\.grok\// || /^site\// || /\.md$/ { next }
    { print }')
  [[ -z "$code" ]] && exit 0
fi

last="$root/target/check/last.json"
head=$(git rev-parse HEAD)
why=""
if [[ ! -f "$last" ]]; then
  why="no target/check/last.json"
elif ! grep -q "\"head\": \"$head\"" "$last"; then
  why="the last check ran on another commit"
elif ! grep -q '"mode": "full"' "$last"; then
  why="the last check was quick, not full"
elif ! grep -q '"ok": true' "$last"; then
  why="the last full check failed"
elif ! grep -q '"dirty": false' "$last"; then
  why="the last full check ran on a dirty tree"
fi
if [[ -n "$why" ]]; then
  echo "Blocked: gh pr create needs scripts/check.sh full to pass on HEAD ($why). Run it, fix what fails, then retry." >&2
  exit 2
fi
exit 0
