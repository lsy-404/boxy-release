# Connector regression verification

- [x] Map the public connector's v2 task, session-watch, writeback, Dreamtonics, local operation, process-block, and packaging contracts.
- [x] Compare v1 local features against the public connector and identify the missing local session selector.
- [x] Restore pre-connect session path choice/default reset; keep the selected path fixed for the active operation.
- [x] Add root `/test` coverage for session I/O/CAS, task-path bounds, Dreamtonics request constraints, protocol snapshots, and path setup UI.
- [x] Run formatting, Cargo checks/tests/build, and portable package tests after prerequisites are ready.
- [x] Restore Cargo and portable package regression steps on both CI platforms and Windows startup smoke on the packaged executable before upload.
- [x] Record final evidence and commit only task changes.
- [x] Restore the v1 bounded product-logo fetch action without exposing session plaintext or editing Worker code.
- [x] Add root `/test` coverage for metadata matching, image URL trust, image formats/size bounds, licenses fallback, and non-overwriting cache behavior.
- [x] Run formatting, locked Cargo tests/check/build, portable packaging tests, update audit evidence, and commit the follow-up.
