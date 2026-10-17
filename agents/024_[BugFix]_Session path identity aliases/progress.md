# Progress

- Added file-identity checks for existing targets and the active session on Unix and Windows.
- Added a hard-link alias regression that verifies rejection and unchanged session contents.
- Preserved within-cap invalid UTF-8 response bytes in the base64url receipt carrier while retaining the `invalid_response` error.
- `cargo +1.98.0 fmt --all` passed.
- `cargo +1.98.0 test --locked -p boxy` passed: 37 connector tests, 6 host-block tests, and 6 launch-option tests.
- `cargo +1.98.0 build --locked -p boxy` and `git diff --check` passed on Windows.
- Ready for local commit; no remote publication.
