# Findings

- The manual workflow builds Windows and macOS assets in separate jobs, both upload to an existing release. Retention can run after both jobs succeed.
- The complete release must contain non-empty, uploaded `boxy-windows-x64.exe` and `boxy-macos-universal.zip` assets.
- Order releases by publication time, falling back to creation time for drafts. Delete only entries strictly older than the requested release; newer and same-time entries remain on older reruns and concurrent runs.
- The GitHub Release API deletes release records; the workflow does not call tag deletion endpoints. Per GitHub's [release REST API](https://docs.github.com/en/rest/releases/releases?apiVersion=latest), the release list excludes regular tags without releases, and the delete endpoint acts on a release ID.
- The behavior tests cover complete uploads, invalid/draft current releases, missing/incomplete assets, equal timestamps, prereleases, old drafts, and an older rerun. All seven pass; Node syntax check and scoped `git diff --check` pass.
- Existing package and protocol fixture Node tests pass (2/2). `cargo test --locked -p boxy` could not start because the host has rustc 1.92 while locked `sysinfo@0.39.6` requires rustc 1.95.
