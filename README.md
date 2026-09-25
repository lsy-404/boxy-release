# Boxy client

This repository contains the macOS and Windows Rust client for the Boxy cloud service. The Cloudflare Worker, browser editor, and Rust/Wasm session codec remain in the separate sv2-boxy repository. The client reads the local SV2 session as ciphertext, pairs with the selected service, performs bounded local tasks, and writes back only a response verified with the service signing key.

## Build

```sh
cargo test --locked -p boxy
cargo build --locked --release -p boxy
node scripts/package-portable.mjs "$(rustc -Vv | sed -n 's/^host: //p')"
```

This creates a portable macOS app and zip in dist/. Windows packaging uses a built x64 exe:

```sh
node scripts/package-portable.mjs x86_64-pc-windows-msvc
```

The Windows ZIP contains boxy.exe. When network blocking is first enabled, the client downloads the pinned Microsoft WebView2 Fixed Runtime, checks its size and SHA-256, then installs it beside Boxy. The macOS client downloads the pinned LuLu installer only when a rule is requested and LuLu is absent. The manual GitHub Actions workflow builds both client archives; it does not build or publish the cloud service.

## Start and choose a service

Before reading the session or contacting a service, Boxy shows a startup notice and an editable service address. Cancel exits. The address must be an HTTPS origin with no credentials, path, query, or fragment; HTTP is allowed only for localhost development.

The client can prefill the address from its filename:

| File or app bundle name | Prefilled address |
| --- | --- |
| boxy.a.b.exe or boxy.a.b.app | https://boxy.a.b |
| a.b.exe or a.b.app | https://a.b |
| boxy.voidcarve.com (1).exe | https://boxy.voidcarve.com |

Boxy uses the longest valid domain-style part of the filename as the HTTPS host, including when copy text such as ` (1)` appears around it. It does not add `boxy.`. Without a matching name, the default is https://boxy.voidcarve.com. BOXY_API_URL or --server URL overrides the filename suggestion, and the address remains editable in the startup prompt. On macOS the client reads the outer .app bundle name, not the internal binary name.

The server signing public key remains independent of the selected address. BOXY_SERVER_PUBLIC_KEY or --server-public-key BASE64 can set the expected key for another service; Boxy does not accept an unverified writeback. --session PATH selects a non-default SV2 session, and --public-ip-url URL changes the public IP lookup endpoint.

## Local operations

The client reports device information and the SV2 directory manifest when starting an operation. Bounded tasks include encrypted session and directory I/O, network block status or changes, SV2 launch or close, opening the session folder, and voice database inspection, download, installation, or removal. Windows network blocking uses WFP rules for SV2 and its private WebView2 runtime; macOS exports a LuLu rule for manual import into a dedicated profile. See docs/network-blocking.md for the verification boundary. Every arbitrary shell command requires separate approval. Close the client to stop the assistance session.

The HTTP JSON task and writeback protocol is implemented by the cloud service; changes to task fields must be verified against its Worker tests before a client release.
