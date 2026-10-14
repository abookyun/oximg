#!/usr/bin/env bash
# PostToolUse(Edit|Write|MultiEdit): rustfmt the Rust file just written,
# so `cargo fmt --check` never fails on an agent's diff. A file that does
# not parse yet is left alone; the next build reports it.
set -uo pipefail

input=$(cat)
if command -v jq >/dev/null 2>&1; then
  file=$(jq -r '.tool_input.file_path // empty' <<<"$input")
else
  file=$(python3 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input", {}).get("file_path", ""))' <<<"$input")
fi

[[ "$file" == *.rs && -f "$file" ]] || exit 0
command -v rustfmt >/dev/null 2>&1 || exit 0
# Through stdin, not as a path: given a crate root (lib.rs, main.rs),
# rustfmt also rewrites every out-of-line module it declares, including
# files with someone's unstaged work. Match Cargo.toml's edition; rustfmt
# alone defaults to 2015.
tmp=$(mktemp) || exit 0
if rustfmt --edition 2024 <"$file" >"$tmp" 2>/dev/null && ! cmp -s "$tmp" "$file"; then
  cat "$tmp" >"$file"
fi
rm -f "$tmp"
exit 0
