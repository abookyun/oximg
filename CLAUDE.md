@AGENTS.md

# Claude Code

Everything above is shared with every coding agent. The lines below
apply to Claude Code alone.

- Skills: `verify` (`.claude/skills/verify/`) is the drive book above;
  `/maintain-verification-skill` is a manual upkeep pass over it.
- A worktree under `.claude/worktrees/` starts without `target/`: the
  first `cargo build --release` there is a full ~100 s build.

## Compaction

When compacting, always preserve: modified file paths, the current
branch, failing test or ctl commands with their JSON, and any A/B
numbers measured so far with the CPU they ran on.
