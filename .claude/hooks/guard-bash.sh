#!/usr/bin/env bash
# PreToolUse(Bash): two rules from AGENTS.md, enforced.
#
# 1. No killing oximg by name: other sessions on the host run their own
#    servers. Tear down only the pid you started.
# 2. `gh pr create` needs `scripts/check.sh full` to have passed on HEAD
#    with a clean tree, unless the branch touches only docs and agent
#    files.
#
# Only command positions count: heredoc bodies and quoted strings are
# dropped first, so a commit message that names `gh pr create` is not a
# PR. A guardrail, not a sandbox — `bash -c '…'` walks past it.
#
# Exit 2 blocks the command and shows stderr to the agent.
set -uo pipefail

input=$(cat)
if command -v python3 >/dev/null 2>&1; then
  cmd=$(python3 -c '
import json, re, sys
cmd = json.load(sys.stdin).get("tool_input", {}).get("command", "")
out, term = [], None
for line in cmd.split("\n"):
    if term is not None:
        if line.strip() == term:
            term = None
        continue
    m = re.search(r"<<-?\s*([\x27\x22]?)(\w+)\1", line)
    if m:
        term = m.group(2)
    out.append(line)
s = "\n".join(out)
s = re.sub(r"\x27[^\x27]*\x27", "\x27\x27", s)
s = re.sub(r"\x22(?:[^\x22\\\\]|\\\\.)*\x22", "\x22\x22", s)
print(s)
' <<<"$input")
elif command -v jq >/dev/null 2>&1; then
  cmd=$(jq -r '.tool_input.command // empty' <<<"$input")
else
  exit 0
fi
[[ -n "$cmd" ]] || exit 0

# Start of a line, or after ; & | ( or $( — then optional sudo.
at='(^|[;&|(]|\$\()[[:space:]]*(sudo[[:space:]]+)?'

if grep -Eq "${at}(killall|pkill)[^;&|]*oximg" <<<"$cmd" ||
  grep -Eq "${at}kill[^;&|]*\\\$\\([[:space:]]*pgrep[^)]*oximg" <<<"$cmd"; then
  echo "Blocked: never kill oximg by name — other sessions on this host run their own. Kill the pid you started (the oximg-ctl wrapper for serve)." >&2
  exit 2
fi

grep -Eq "${at}gh[[:space:]]+pr[[:space:]]+create([^[:alnum:]_-]|$)" <<<"$cmd" || exit 0

root=${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}
cd "$root" || exit 0

# Docs and agent instructions cannot break the build.
base=$(git merge-base origin/main HEAD 2>/dev/null || true)
if [[ -n "$base" ]]; then
  code=$(git diff --name-only "$base" HEAD |
    grep -Ev '\.md$|^\.claude/|^\.cursor/|^\.grok/|^docs/|^site/' || true)
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
