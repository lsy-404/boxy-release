#!/usr/bin/env bash
set -euo pipefail

runner_temp="$(cd "${RUNNER_TEMP:-/tmp}" && pwd -P)"
test_root="$(mktemp -d "$runner_temp/boxy-v23-native.XXXXXX")"
test_root="$(cd "$test_root" && pwd -P)"
fixture_pid=""

cleanup() {
  if [[ -n "$fixture_pid" ]] && kill -0 "$fixture_pid" 2>/dev/null; then
    kill -TERM "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
  fi
  case "$test_root" in
    "$runner_temp"/boxy-v23-native.*) rm -rf "$test_root" ;;
    *) printf 'Refusing to remove unexpected smoke path: %s\n' "$test_root" >&2; return 1 ;;
  esac
}
trap cleanup EXIT

mkdir -p "$test_root/scope/scope"
printf 'isolated probe' > "$test_root/scope/scope/probe.txt"
node test/fixtures/v23_service.mjs \
  --state-file "$test_root/state.json" \
  --port-file "$test_root/port.txt" \
  --root "$test_root/scope" &
fixture_pid=$!

deadline=$((SECONDS + 15))
while [[ ! -f "$test_root/port.txt" ]]; do
  if ! kill -0 "$fixture_pid" 2>/dev/null; then
    printf 'The v4 fixture exited before becoming ready.\n' >&2
    exit 1
  fi
  if (( SECONDS >= deadline )); then
    printf 'The v4 fixture did not become ready.\n' >&2
    exit 1
  fi
  sleep 0.1
done

service_url="http://127.0.0.1:$(cat "$test_root/port.txt")"
BOXY_RENDER_SMOKE_SERVICE_URL="$service_url" \
BOXY_RENDER_SMOKE_STATE="$test_root/state.json" \
  cargo +1.98.0 run --locked -p boxy --example desktop_render_smoke
