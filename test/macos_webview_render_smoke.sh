#!/usr/bin/env bash
set -euo pipefail

runner_temp="$(cd "${RUNNER_TEMP:-/tmp}" && pwd -P)"
test_root="$(mktemp -d "$runner_temp/boxy-macos-render-smoke.XXXXXX")"
test_root="$(cd "$test_root" && pwd -P)"
fixture_pid=""

cleanup() {
  if [[ -n "$fixture_pid" ]] && kill -0 "$fixture_pid" 2>/dev/null; then
    kill -TERM "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
  fi
  case "$test_root" in
    "$runner_temp"/boxy-macos-render-smoke.*) rm -rf "$test_root" ;;
    *) printf 'Refusing to remove unexpected smoke path: %s\n' "$test_root" >&2; return 1 ;;
  esac
}
trap cleanup EXIT

mkdir -p "$test_root/SV2/Config"
printf '\001\002\003\004\005\006\007\010\011\012\013\014\015\016\017\020' > "$test_root/SV2/Config/session.bin"
printf 'boxy-smoke-marker' > "$test_root/SV2/smoke-marker.txt"
node -e 'const fs=require("node:fs"),p=require("node:path"),root=process.argv[1],image=Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lXcAAAAASUVORK5CYII=","base64");fs.mkdirSync(p.join(root,"SV2/databases/meta"),{recursive:true});fs.writeFileSync(p.join(root,"SV2/databases/meta/fixture.png"),image);fs.writeFileSync(p.join(root,"SV2/outside.png"),image)' "$test_root"

node test/fixtures/tauri-webview-server.mjs \
  --state-file "$test_root/fixture-state.json" \
  --port-file "$test_root/fixture-port.txt" \
  --probe-select-service --include-local-image &
fixture_pid=$!

deadline=$((SECONDS + 15))
while [[ ! -f "$test_root/fixture-port.txt" ]]; do
  if ! kill -0 "$fixture_pid" 2>/dev/null; then
    printf 'The local protocol fixture exited before becoming ready.\n' >&2
    exit 1
  fi
  if (( SECONDS >= deadline )); then
    printf 'The local protocol fixture did not become ready.\n' >&2
    exit 1
  fi
  sleep 0.1
done

service_url="http://127.0.0.1:$(cat "$test_root/fixture-port.txt")"
env -u BOXY_API_URL \
  BOXY_RENDER_SMOKE_SERVICE_URL="$service_url" \
  BOXY_RENDER_SMOKE_STATE="$test_root/fixture-state.json" \
  cargo run --locked -p boxy --example desktop_render_smoke -- \
    --session "$test_root/SV2/Config/session.bin"
