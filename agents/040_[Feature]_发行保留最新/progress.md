# Progress

- Read the agent-mode skill and project instructions. Confirmed the target worktree is clean on `codex/release-retention`.
- Inspected `.github/workflows/manual-build.yml`; Windows and macOS uploads are independent jobs and both can gate a final retention job.
- Added `scripts/release-retention.mjs`, a retention job that depends on both platform uploads, and isolated tests under `/test`.
- `node --test test/release_retention.test.mjs`: 7 passed, 0 failed. `node --check scripts/release-retention.mjs` and scoped `git diff --check` passed.
- Added a workflow wiring assertion and reran the focused suite: 7 passed, 0 failed. Committed as `feat: retain only the latest complete release`.
- Existing packaging and protocol fixture tests passed (2/2). The Rust test command is blocked by the installed rustc 1.92 being older than the locked `sysinfo@0.39.6` minimum of 1.95.
