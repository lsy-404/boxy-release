# Progress

- Read `agent-mode/SKILL.md`, `agents/project.md`, `agents/tasks.md`, and the project instructions.
- Confirmed the isolated checkout is on `codex/preserve-author-core` at base `fda72af`; its worktree was clean before changes.
- Updated `scripts/normalize-history.mjs` to read source identities using raw Git pretty-format fields and use those identities for the root snapshot while retaining both epoch dates.
- Updated `test/normalize_history.test.mjs` with distinct source author/committer values and ambient-identity interference assertions; restored fixture-local identity for unrelated scenario commits.
- `node --test test/normalize_history.test.mjs`: all 6 tests passed.
- Committed the five scoped files locally as `7201b2e` (`Preserve source identity in normalized history`); commit author and committer use the checkout's configured repository-owner identity.
