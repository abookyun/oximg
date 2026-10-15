---
name: verify
description: >
  Close the loop on oximg changes by driving the compiled binary with
  oximg-ctl and matching JSON to docs/features/. cargo test green is
  not done. Use when changing HTTP routes, pixels, knobs, formats,
  CLI, signing, SIMD, or when asked to verify, prove, /verify,
  oximg-ctl, or whether a change actually works.
---

# Verify oximg

The drive book lives in
[`.claude/skills/verify/SKILL.md`](../../../.claude/skills/verify/SKILL.md),
shared by every agent. Read it and follow it; its feature files are
under [`.claude/skills/verify/features/`](../../../.claude/skills/verify/features/).
Do not copy it here — this file only points.
