# Boxy client

This repository contains the macOS and Windows Rust client for a user-selected remote service. The companion Cloudflare Worker, browser editor, and Rust/Wasm session codec remain in the separate sv2-boxy repository. The client reads the local SV2 session as ciphertext, pairs with the selected service, watches for local session changes, performs local tasks, and writes back updated session ciphertext after checking that the local file has not changed. It continues watching and handling remote requests until stopped.

## Build

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo test --locked -p boxy
cargo build --locked --release -p boxy --target aarch64-apple-darwin
cargo build --locked --release -p boxy --target x86_64-apple-darwin
mkdir -p dist
lipo -create target/aarch64-apple-darwin/release/boxy target/x86_64-apple-darwin/release/boxy -output dist/boxy-universal
node scripts/package-portable.mjs universal-apple-darwin --binary dist/boxy-universal
```

This creates a universal macOS app and `boxy-macos-universal.zip` in `dist/`. Windows packaging creates a single portable x64 `.exe`:

```sh
node scripts/package-portable.mjs x86_64-pc-windows-msvc
```

Windows users download and run `boxy-windows-x64.exe` directly. Windows Firewall mode creates a rule for the installed SV2 executable. On macOS, LuLu rules remain manual because Boxy cannot apply or verify them. Boxy can remove its own old hosts entries, but no longer adds them. The manual GitHub Actions workflow uploads the Windows executable and macOS app archive; it does not build or publish the cloud service.

## Start and choose a service

Boxy connects using the service URL supplied through `--server`, `BOXY_API_URL`, or a domain-style executable name. Ordinary release filenames use `https://boxy.voidcarve.com`. The URL must be an HTTPS origin with no credentials, path, query, or fragment; HTTP is allowed only for localhost development. If the default SV2 session file is unavailable, Boxy opens a native file picker.

For example, `service.example.test.exe` or `service.example.test.app` suggests `https://service.example.test`; the downloaded `boxy-windows-x64.exe` uses the default service. The client reads the outer `.app` bundle name on macOS.

`BOXY_API_URL` or `--server URL` sets the service address. The selected service controls the browser editor and the remote operations it requests. `--session PATH` selects a non-default SV2 session. The service observes the public IP from the connection, so the client does not contact a separate IP lookup site.

After the service creates an operation and returns a validated browser URL, Boxy opens that URL in the system browser and continues monitoring the local session until the operation is no longer available or the process is stopped.

## Local operations

Before pairing, the client hashes files in the SV2 data directory and sends the encrypted session, source hash, device public key, complete bounded directory manifest, SV2-related installed applications, device information, available translation catalogs, and session path to the selected service. If the complete manifest and request do not fit the Worker request limit, the client stops before sending a partial manifest. While connected, Boxy checks the local session every two seconds and sends a new encrypted snapshot when its hash changes. The service can request an automatic session writeback; Boxy verifies the source hash before replacing the file, reports a signed receipt, and continues watching. Bounded local tasks include directory I/O, process-level network blocking on Windows, old Boxy hosts cleanup, SV2 launch or close, opening the session folder, and voice database inspection, download, installation, or removal. For network tasks, the service supplies the HTTP method, URL, headers, body, response encoding, timeout, and response size limit. The client executes the request once with redirects disabled, then reports the status, bounded response, headers, and transport metadata. The service chooses regional endpoints, constructs account parameters, parses responses, refreshes credentials, waits for session writeback acknowledgement, and decides whether to request another attempt. Product images use the same HTTP executor with server-selected file reads and writes. macOS LuLu rules are managed in LuLu. See docs/network-blocking.md for platform details. Every arbitrary shell command requires separate approval. Stop the connector process to end the local assistance session.

The connector and service require protocol version 3. Routine endpoint, parameter, parsing, or request-sequence changes are deployed on the service. New local capabilities or protocol changes can still require a client release. Session replacement keeps its dedicated source-hash check; generic file writes cannot replace the active session. Request and receipt contents are returned to the selected service for encrypted logging.
