# Progress

- Loaded agent-mode instructions and inspected repository index/task records.
- Confirmed the release worktree was clean and `main` resolved to the current origin/main commit before creating this task branch.
- Saved full GitHub release JSON for all 10 releases, old release assets (18 files), latest asset verification downloads, tag snapshot, and a metadata-only manifest under `C:\Users\User\.codex\backups\boxy-release\20261019-history-cleanup\releases`.
- Verified every old and retained asset's byte size and SHA-256 against GitHub's API digest before/after the release mutations.
- Changed release 417493925 notes to the exact requested sentence and deleted nine older release records without passing `--cleanup-tag`.
- Re-read release and tag APIs: one release remains; all 11 v2 tags are unchanged; v2.2.0 ID, tag, target commitish, asset IDs, sizes, and digests are preserved.
