import assert from "node:assert/strict";
import { generateKeyPairSync, sign } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = fileURLToPath(new URL("..", import.meta.url));
const html = readFileSync(`${root}/connector/bootstrap/index.html`, "utf8");
const script = readFileSync(`${root}/connector/bootstrap/selection.js`, "utf8");
const desktop = readFileSync(`${root}/connector/src/desktop.rs`, "utf8");
const core = readFileSync(`${root}/connector/src/lib.rs`, "utf8");
const manifest = readFileSync(`${root}/connector/Cargo.toml`, "utf8");
const connectorVersion = /^version\s*=\s*"([^"]+)"/m.exec(manifest)?.[1];
const fixture = `${root}/test/fixtures/v23_service.mjs`;

test("startup contains one blank, accessible URL field and submits on Enter", () => {
  assert.match(html, /<title>Boxy Connector<\/title>/);
  assert.equal((html.match(/<input\b/g) ?? []).length, 1);
  assert.match(html, /id="service-url"[^>]*type="text"/);
  assert.match(html, /id="service-url"[^>]*aria-label="Service address"/);
  assert.match(html, /id="service-url"[^>]*autofocus/);
  assert.match(html, /id="service-url"[^>]*placeholder="Type Server Link"/);
  assert.doesNotMatch(html, /<h[1-6]\b|<button\b|<select\b|<textarea\b/);
  assert.doesNotMatch(html, /<p(?![^>]*id="error")/);
  assert.match(script, /service-form.*submit/s);
  assert.match(script, /invoke\('select_service'/);
  assert.doesNotMatch(script, /session|device|inventory|open_folder/i);
});

test("startup and bridge windows use a fixed native title", () => {
  assert.equal((desktop.match(/\.title\("Boxy Connector"\)/g) ?? []).length, 2);
  assert.doesNotMatch(desktop, /on_document_title_changed/);
  const versionedUserAgents = desktop.match(
    /\.user_agent\(concat!\("BoxyConnector\/", env!\("CARGO_PKG_VERSION"\)\)\)/g,
  ) ?? [];
  assert.equal(versionedUserAgents.length, 2);
});

test("connection failures offer signed remote assistance and keep the connection available", () => {
  assert.match(desktop, /需要远程协助/);
  assert.match(desktop, /assistance\.request\(\)/);
  assert.match(desktop, /assistance\.close\(\)/);
  assert.match(core, /\/api\/connections\/\{\}\/assistance/);
  assert.match(core, /Status\(425 \| 503/);
  assert.match(core, /close_connection = false/);
});

test("startup infers only a filename URL and does not use environment or command-line options", () => {
  const launch = readFileSync(`${root}/connector/src/launch.rs`, "utf8");
  assert.match(launch, /current_exe\(\)[\s\S]*inferred_service_url/);
  assert.doesNotMatch(launch, /BOXY_API_URL|session_path|FileDialog/);
});

test("v4 fixture gates scoped file tasks behind authorization and verifies receipts", async (t) => {
  const directory = mkdtempSync(join(tmpdir(), "boxy-v23-fixture-"));
  const scope = join(directory, "scope");
  mkdirSync(scope);
  writeFileSync(join(scope, "probe.txt"), "isolated probe");
  const statePath = join(directory, "state.json");
  const portPath = join(directory, "port.txt");
  const child = spawn(process.execPath, [fixture, "--state-file", statePath, "--port-file", portPath, "--root", directory], { stdio: "ignore" });
  t.after(async () => {
    if (child.exitCode === null) {
      await new Promise((resolve) => {
        child.once("exit", resolve);
        child.kill("SIGTERM");
      });
    }
    rmSync(directory, { recursive: true, force: true });
  });

  let port;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      port = Number(readFileSync(portPath, "utf8"));
      if (port > 0) break;
    } catch {}
    if (child.exitCode !== null) throw new Error("v4 fixture exited during startup");
    await delay(50);
  }
  assert.ok(port > 0, "fixture should publish its loopback port");
  const base = `http://127.0.0.1:${port}`;
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const rawKey = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const connect = await fetch(`${base}/api/connect`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-boxy-connector-version": connectorVersion },
    body: JSON.stringify({
      protocolVersion: 4,
      platform: "windows",
      capabilities: ["fs.list"],
      publicKey: rawKey.toString("base64url"),
    }),
  });
  assert.equal(connect.status, 200);
  const connection = await connect.json();
  assert.equal(connection.protocolVersion, 4);
  assert.equal(typeof connection.expiresAt, "number");
  assert.ok(connection.expiresAt > Date.now());
  assert.equal(connection.browserUrl, `${base}/`);
  assert.deepEqual(connection.browserCookies, [{ name: "boxy_connection", value: "synthetic-session" }]);
  const browserResponse = await fetch(connection.browserUrl, {
    headers: {
      cookie: connection.browserCookies.map(({ name, value }) => `${name}=${value}`).join("; "),
      "user-agent": `BoxyConnector/${connectorVersion}`,
    },
  });
  assert.equal(browserResponse.status, 200);

  async function poll(nonce) {
    const path = "/api/connections/smoke_connection";
    const signature = sign(null, Buffer.from(`GET\n${path}\n${nonce}`), privateKey).toString("base64url");
    const response = await fetch(`${base}${path}`, { headers: { "x-boxy-connector-version": connectorVersion, "x-connector-nonce": nonce, "x-connector-signature": signature } });
    assert.equal(response.status, 200);
    return response.json();
  }
  const unauthenticated = await poll("poll_before_auth");
  assert.deepEqual(unauthenticated, { protocolVersion: 4, state: "awaiting_auth", tasks: [] });
  const authorized = await poll("poll_after_auth");
  assert.equal(authorized.state, "ready");
  assert.equal(authorized.tasks[0].kind, "fs.list");
  assert.deepEqual(authorized.tasks[0].params, { root: directory, paths: ["."] });

  const receiptPath = "/api/connections/smoke_connection/tasks/scope_list/receipt";
  const receiptBody = JSON.stringify({ ok: true, result: { files: [{ path: "scope/probe.txt", kind: "file", size: 14, sha256: "probe" }] } });
  const receiptSignature = sign(null, Buffer.from(`POST\n${receiptPath}\nreceipt_nonce`), privateKey).toString("base64url");
  const receipt = await fetch(`${base}${receiptPath}`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-boxy-connector-version": connectorVersion, "x-connector-nonce": "receipt_nonce", "x-connector-signature": receiptSignature },
    body: receiptBody,
  });
  assert.equal(receipt.status, 200);
  const state = JSON.parse(readFileSync(statePath, "utf8"));
  assert.equal(state.browserUserAgent, `BoxyConnector/${connectorVersion}`);
  assert.equal(state.browserCookie, "boxy_connection=synthetic-session");
  assert.equal(state.receiptValid, true);
  assert.deepEqual(state.errors, []);
});
