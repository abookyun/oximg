"""Print a Bash tool call's command with heredoc bodies dropped.

Reads the PreToolUse JSON on stdin. A heredoc body is data (a commit
message, a PR body), never a command, so the guard should not match it.
"""

import json
import re
import sys

# `$((…))` / `((…))` arithmetic can hold `<<` shifts; blank it before
# looking for a heredoc operator.
ARITH = re.compile(r"\$?\(\(.*?\)\)")
HEREDOC = re.compile(r"(?<!<)<<(?!<)-?[ \t]*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\1")


def drop_heredocs(cmd):
    out, term = [], None
    for line in cmd.split("\n"):
        if term is not None:
            if line.strip() == term:
                term = None
            continue
        m = HEREDOC.search(ARITH.sub("", line))
        if m:
            term = m.group(2)
        out.append(line)
    return "\n".join(out)


def main():
    cmd = json.load(sys.stdin).get("tool_input", {}).get("command", "")
    print(drop_heredocs(cmd))


if __name__ == "__main__":
    main()
