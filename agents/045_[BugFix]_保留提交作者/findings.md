# Findings

The normalization script currently replaces both identities with hardcoded generic values. `git show -s --format=%an%x00%ae%x00%cn%x00%ce <sha>` exposes the source commit's raw author/committer names and emails without applying a mailmap. `commit-tree` can receive those values through per-process Git environment variables while both dates remain `@0 +0000`.

The focused test already creates a local bare remote and checks tree, root shape, timestamps, signature absence, stale leases, and idempotence. The fixture will assign distinct source author and committer identities to prove both survive.

The first focused test run passed the normalization case but two pre-existing scenario commits failed because the fixture had relied on implicit local Git identity configuration. Restoring fixture-only local identity resolved this setup issue; the source commits explicitly override it with distinct identities. Final focused run passed all six tests.
