# Session alias protection and raw response preservation

## Goal

Prevent generic file writes from modifying the active session through an alternate path that resolves to the same file, and preserve bounded raw response bytes when a UTF-8 response is invalid.

## Scope

- Compare existing target and session file identity on Unix and Windows.
- Add a root `/test` regression using a hard-link alias.
- Return invalid UTF-8 response bytes in the binary receipt carrier while reporting the decoding error.
- Run formatting, tests, and build; commit locally without pushing.

## Completion criteria

- File identity aliases are rejected before any write.
- Existing path-based protection remains in place.
- Tests and build pass.
