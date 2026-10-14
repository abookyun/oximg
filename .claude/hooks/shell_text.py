"""Reduce a Bash tool call's command to the text the hooks should match.

Reads the PreToolUse JSON on stdin and prints the command with:
  heredoc  heredoc bodies dropped (quoted arguments kept)
  code     heredoc bodies dropped and the inside of every quoted string
           emptied, so only command words remain

A best-effort lexer for a guardrail, not a shell parser.
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


def empty_quotes(s):
    """One left-to-right pass: keep the quote marks, drop what they hold."""
    res, quote, i = [], None, 0
    while i < len(s):
        c = s[i]
        if quote is None:
            if c == "\\" and i + 1 < len(s):
                res.append(s[i : i + 2])
                i += 2
                continue
            if c in "'\"":
                quote = c
            res.append(c)
        elif c == quote:
            quote = None
            res.append(c)
        elif quote == '"' and c == "\\" and i + 1 < len(s):
            i += 2
            continue
        i += 1
    return "".join(res)


def main():
    mode = sys.argv[1]
    cmd = json.load(sys.stdin).get("tool_input", {}).get("command", "")
    text = drop_heredocs(cmd)
    if mode == "code":
        text = empty_quotes(text)
    print(text)


if __name__ == "__main__":
    main()
