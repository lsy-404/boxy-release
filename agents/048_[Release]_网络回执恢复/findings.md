# Findings

- [Observed] The current client version is 2.2.3 in `connector/Cargo.toml`, `connector/tauri.conf.json`, and the root `Cargo.lock` package named `boxy`. A separate dependency version `ed25519` is also 2.2.3 and must remain unchanged.
- [Scope] Prepare version metadata only. The parent task owns task indexes, live merge, and release operations; do not push, tag, or dispatch.
- [Verified] Cargo metadata with `--locked` reports `boxy` 2.2.4; Cargo.toml, Tauri config, and only the `boxy` package entry in Cargo.lock agree. The unrelated `ed25519` 2.2.3 dependency remains unchanged.
- [Validation] `node --test test/package_portable.test.mjs` and `node --test test/tauri_webview_fixture.test.mjs` passed. README is unchanged.
