@AGENTS.md

# Claude Code

Everything above is shared with every coding agent. The lines below
apply to Claude Code alone.

- Skills: `verify` (`.claude/skills/verify/`) is the drive book above;
  `/maintain-verification-skill` is a manual upkeep pass over it.
- Hooks (`.claude/settings.json`): an edited `.rs` file is run through
  rustfmt; Bash refuses to kill oximg by name; session start sets
  `core.hooksPath` to `.githooks` when the clone has none, which turns
  on the pre-push gate above (it says so if another hooks path wins).
- A worktree under `.claude/worktrees/` starts without `target/`: the
  first `cargo build --profile agent` there is a full build (~45 s).

## Compaction

When compacting, always preserve: modified file paths, the current
branch, failing test or ctl commands with their JSON, and any A/B
numbers measured so far with the CPU they ran on.
