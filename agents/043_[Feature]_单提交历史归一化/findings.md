# Findings

- The repository has no root `package.json`; existing Node integration tests live directly under `/test` and use Node's built-in test runner.
- The CLI requires the triggering commit as `GITHUB_SHA`; the exported API is `normalizeHistory({ cwd, expectedSha })`.
- The script checks `HEAD` against the full expected SHA, creates the root commit from that tree with fixed identity/date and explicit signing disabled, then reads `origin`'s `refs/heads/main`.
- A matching root SHA is an idempotent success; a stale remote tip skips the push; a matching event SHA permits only the exact `main` ref update with an explicit force-with-lease.
- A local bare-origin race test updates `main` from a pre-push hook after inspection and confirms the lease rejects the stale push.
- No GitHub push or live remote write was performed.