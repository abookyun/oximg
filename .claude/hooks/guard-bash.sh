#!/usr/bin/env bash
# PreToolUse(Bash): never kill oximg by name — other sessions on the host
# run their own servers. Tear down only the pid you started.
#
# Matches `killall`/`pkill`/`kill $(pgrep …)` at a command position (line
# start, or after ; & | ( or $(), with quotes kept, since a -f pattern is
# usually quoted. Heredoc bodies are dropped first (shell_text.py), so a
# commit message written through a heredoc never trips it. Known false
# positive, accepted: a line inside a multi-line quoted string that starts
# with `pkill … oximg` is blocked — write such a message with a heredoc.
# A guardrail, not a sandbox: `bash -c '…'` walks past it.
#
# The PR gate is not here: .githooks/pre-push enforces it on the pushed
# commits themselves, without reading command text.
#
# Exit 2 blocks the command and shows stderr to the agent.
set -uo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
input=$(cat)
if command -v python3 >/dev/null 2>&1; then
  cmd=$(python3 "$here/shell_text.py" <<<"$input")
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
exit 0
