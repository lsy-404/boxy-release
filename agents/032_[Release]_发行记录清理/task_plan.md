# Plan

- [x] Confirm the public release inventory and v2.2.0 asset IDs/digests without exposing release bodies.
- [x] Save full metadata and every older release asset to the dated external backup directory.
- [x] Verify backup file sizes and GitHub-provided digests; write a concise manifest.
- [x] Update v2.2.0 body and delete all older releases without deleting tags.
- [x] Re-read GitHub release inventory and verify v2.2.0 metadata/assets remain unchanged.
- [x] Commit only the release-cleanup audit records on this task branch.
