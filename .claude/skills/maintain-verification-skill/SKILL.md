---
name: maintain-verification-skill
description: >
  Periodic pass that keeps the verify skill and its feature files
  honest against the source and the compiled binary. Use for
  /maintain-verification-skill, audit the verify skill, or after a
  route, knob, format, or status lands.
disable-model-invocation: true
---

# Maintain verify

**Target:** [`.claude/skills/verify/`](../verify/) — `SKILL.md`,
`features/`, `scripts/`. Drive and launch rules stay in that skill;
this file only owns the upkeep loop.

`docs/features/` is pinned only in part: `src/config.rs`
(`knobs_are_documented`, `feature_map_errors`,
`feature_map_format_tokens`) checks the knob table in `knobs.md`, the
ErrorKind/status table in `errors.md`, and the `@{fmt}` token list in
`formats.md`, and CI fails on drift there. Nothing checks `routes.md`,
`paths.md`, `invariants.md`, or the prose of the pinned files, and
nothing checks the skill's `features/` recipes or their `expect:`
comments. The recipes are what this pass fixes; drift it finds in the
unpinned map files is reported, not dismissed.

## Outcomes

Pick one, and say which:

- **clean** — every feature file was checked against the source and
  driven live; nothing worth shipping. No branch, no PR.
- **changed** — one PR of proven corrections inside the target
  directory (and the `.cursor/` / `.grok/` pointers, if a path moved).
- **blocked** — the build or a drive could not finish. Name the
  blocker.

## Edit scope

Only the target directory and the two pointer skills. Never edit
`src/`, `tests/`, or `docs/features/` in this run. A behavior a feature
file describes that the binary no longer does is either doc drift (fix
the file) or a product regression (report it, keep it out of this PR).

## Pass

1. **Index hygiene.** `features/README.md` lists every
   `features/*.md` except itself, and nothing else. Every route, `@{fmt}` behavior,
   and caller path in [`docs/features/`](../../../docs/features/)
   (`routes.md`, `paths.md`, `formats.md`) is reachable from some
   feature file, or the gap is named in the outcome. Do not write
   stubs for surfaces you have not driven.

2. **Source check.** For each feature file, find the code that owns it
   (route table in `src/main.rs`, `src/cli.rs`, `src/options.rs`,
   `src/bin/oximg-ctl.rs`) and compare the sub-features, flags, and
   status codes the file claims. One read-only subagent per feature
   file is fine; they never drive the binary and never edit.

3. **Live pass.** Required even when the source looks clean. Follow
   the verify skill's Launch, Doctor, and Cleanup:

   - `cargo build --release`, then
     `.claude/skills/verify/scripts/doctor.sh` must exit 0.
   - Run every **Driving it with oximg-ctl** block. Each status,
     content type, or geometry the block states (`--expect`, the
     `# expect:` comments) is a claim; the ctl JSON either matches it
     or the file is wrong. Capture JSON under
     `target/verify-evidence/`.
   - Drive the `SKILL.md` Drive block too — it is the first thing an
     agent copies.
   - Auto-spawn only. If a block needs `serve`, tear it down by the
     wrapper pid; never kill by name.

4. **Triage.** Wrong description or `expect:` → doc drift, fix it.
   Working behavior ctl cannot drive → harness gap, report it (a new
   ctl subcommand is a `src/` change, out of scope here). Binary
   wrong → product gap, report only. A `docs/features/` file that
   disagrees with the source or the live run → map drift, report it
   with the file, the line, and the ctl JSON; its fix is a separate
   PR.

5. **Ship or stop.** For **changed**: one PR, every changed file
   re-read, the ctl JSON that proves each correction pasted into the
   body. For **clean** or **blocked**: no PR; run notes stay in
   `target/`.
