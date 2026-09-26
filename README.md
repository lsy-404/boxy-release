# Boxy client

This repository contains the macOS and Windows Rust client for a user-selected remote service. The companion Cloudflare Worker, browser editor, and Rust/Wasm session codec remain in the separate sv2-boxy repository. The client reads the local SV2 session as ciphertext, pairs with the selected service, performs bounded local tasks, and writes back only a response verified with the service signing key.

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

Windows users download and run `boxy-windows-x64.exe` directly. When network blocking is first enabled, the client downloads the pinned Microsoft WebView2 Fixed Runtime, checks its size and SHA-256, then installs it beside Boxy. The macOS client downloads the pinned LuLu installer only when a rule is requested and LuLu is absent. The manual GitHub Actions workflow uploads the Windows executable and macOS app archive; it does not build or publish the cloud service.

## Start and choose a service

Before reading the session or contacting a service, Boxy asks for the service address and signing key. Windows shows a native setup window; macOS uses system prompts. Both fields start blank unless supplied through runtime environment variables, command-line options, or a domain-style filename suggestion. No remote is built in; the user confirms the editable values before connecting. Cancel exits. The address must be an HTTPS origin with no credentials, path, query, or fragment; HTTP is allowed only for localhost development.

For example, `service.example.test.exe` or `service.example.test.app` suggests `https://service.example.test`; the downloaded `boxy-windows-x64.exe` starts with an empty address. The client reads the outer `.app` bundle name on macOS. The Windows window does not depend on Windows Script Host.

`BOXY_API_URL` or `--server URL` can prefill the service address. `BOXY_SERVER_PUBLIC_KEY` or `--server-public-key BASE64` can prefill its base64url Ed25519 public key. Boxy verifies signed writebacks with that key. The selected service controls the browser editor and the remote operations it requests. `--session PATH` selects a non-default SV2 session. The service observes the public IP from the connection, so the client does not contact a separate IP lookup site.

After pairing, Boxy automatically opens the validated browser editor URL. On Windows, a small status window remains visible through connection, remote activity, signed writeback, completion, or error. Its `Open browser editor` button reopens that page; `Stop Boxy` stops after the current operation. On macOS, Boxy uses informational prompts around the browser session.

## Local operations

The client reports device information and the SV2 directory manifest when starting an operation. Bounded tasks include encrypted session and directory I/O, network block status or changes, SV2 launch or close, opening the session folder, and voice database inspection, download, installation, or removal. Windows network blocking uses WFP rules for SV2 and its private WebView2 runtime; macOS exports a LuLu rule for manual import into a dedicated profile. See docs/network-blocking.md for the verification boundary. Every arbitrary shell command requires separate approval. Close the client to stop the assistance session.

The HTTP JSON task and writeback protocol is implemented by the cloud service; changes to task fields must be verified against its Worker tests before a client release.
