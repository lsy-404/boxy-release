# Progress

- Loaded the `agent-mode` workflow and reviewed the project index, local instructions, and existing task history.
- Confirmed this worktree is `codex/task-receipt-network` at `f0adab8`, with no initial working-tree changes.
- Located receipt callsites in `connector/src/lib.rs`, existing retry utilities, and local TCP HTTP fixtures in `test/core_runner.rs` and `test/connector_regression.rs`.
- Coordinated with the parent on the service contract: exact authenticated receipt replay returns a completed ACK without repeating journal/business effects; conflicting or unknown receipts return terminal errors.
- Added `complete_directory_task` and a bounded retry sender used only for directory-task completion receipts. Each attempt uses fresh signing data over identical zeroizing serialized bytes; transport and truncated response reads retry, while HTTP and JSON errors terminate.
- Added fixtures for accepted-then-truncated ACK replay, stable body/fresh signatures, terminal auth response, retry bound, cancellation, and one-task/two-receipt end-to-end behavior.
- `cargo +1.98.0 test --manifest-path connector/Cargo.toml --lib --no-default-features`: 51 passed.
- `cargo +1.98.0 fmt --manifest-path connector/Cargo.toml -- --check`: passed. `git diff --check`: passed.
- Added the service's authenticated processing status 425 as the sole retryable HTTP response; all other statuses remain terminal.
- Expanded receipt and connected-run fixtures to cover 425, an accepted receipt with a truncated ACK, and successful exact-body replay, while confirming one task delivery and distinct nonces on each receipt attempt.
- First focused run exposed that 425 had been added to the poll handler instead of the receipt handler; moved it to the dedicated receipt sender and reran the receipt and connected-run tests successfully.
- Full Rust 1.98.0 library suite passed: 51 tests. Formatting check and `git diff --check` passed.
- Root review found command final receipts still used the old single-shot sender. Unified command and other directory completions into `complete_task`, keeping the first final/cancel send permitted after stop and making retries stop-aware.
- Expanded the connected lost-receipt fixture to run a shell command, assert exactly one command approval/execution, compare all receipt bodies, and simulate 425 plus lost ACK before replay acceptance.
- Removed the obsolete single-shot completion sender and updated its signed-body fixture to use the unified helper. Full library suite: 51 passed; formatting and diff checks passed.
