import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

const repositoryRoot = fileURLToPath(new URL("..", import.meta.url));
const fixture = join(repositoryRoot, "test", "fixtures", "tauri-webview-server.mjs");

test("local cloud fixture verifies protocol 3 signed poll and file receipt", async (t) => {
  const directory = mkdtempSync(join(tmpdir(), "boxy-tauri-fixture-test-"));
  const statePath = join(directory, "state.json");
  const portPath = join(directory, "port.txt");
  const child = spawn(process.execPath, [fixture, "--state-file", statePath, "--port-file", portPath, "--probe-select-service", "--include-local-image"], {
    stdio: "ignore",
  });
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
    if (child.exitCode !== null) throw new Error("fixture process exited during startup");
    await delay(50);
  }
  assert.ok(port > 0, "fixture should publish its loopback port");
  const base = `http://127.0.0.1:${port}`;
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const publicRaw = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const start = await fetch(`${base}/api/v2/connector/operations`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ protocolVersion: 3, devicePublicKey: publicRaw.toString("base64url") }),
  });
  const operation = await start.json();
  assert.equal(operation.protocolVersion, 3);
  const editor = await fetch(operation.browserUrl);
  assert.equal(editor.status, 200);
  assert.match(editor.headers.get("set-cookie"), /HttpOnly/);
  const smokeScript = await fetch(`${base}/smoke.js`);
  assert.equal(smokeScript.status, 200);
  const smokeSource = await smokeScript.text();
  assert.match(smokeSource, /select_service.*serverUrl/);
  assert.match(smokeSource, /__BOXY_LOCAL_IMAGE_BASE__/);
  assert.match(smokeSource, /outside\.png/);
  assert.equal((await fetch(`${base}/ready?ipc=absent&select=absent`, { headers: { cookie: "boxy-auth=fixture-auth" } })).status, 200);

  const pollPath = "/api/v2/connector/operations/packaging-smoke";
  const nonce = "fixture-nonce";
  const pollSignature = sign(null, Buffer.from(`GET\n${pollPath}\n${nonce}`), privateKey).toString("base64url");
  const poll = await fetch(`${base}${pollPath}`, {
    headers: { "x-connector-nonce": nonce, "x-connector-signature": pollSignature },
  });
  const pollBody = await poll.json();
  assert.equal(pollBody.directoryTasks[0].kind, "read_files");

  const content = Buffer.from("boxy-smoke-marker");
  const receiptPath = `${pollPath}/directory-tasks/packaging-smoke-read/complete`;
  const receiptBody = JSON.stringify({ files: [{
    path: "smoke-marker.txt",
    size: content.length,
    sha256: createHash("sha256").update(content).digest("base64url"),
    contentBase64: content.toString("base64url"),
  }] });
  const receiptNonce = "receipt-nonce";
  const receiptSignature = sign(null, Buffer.from(`POST\n${receiptPath}\n${receiptNonce}`), privateKey).toString("base64url");
  const receipt = await fetch(`${base}${receiptPath}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-connector-nonce": receiptNonce,
      "x-connector-signature": receiptSignature,
    },
    body: receiptBody,
  });
  assert.equal(receipt.status, 200);
  const state = JSON.parse(readFileSync(statePath, "utf8"));
  assert.equal(state.editorLoaded, true);
  assert.equal(state.readyFetch, true);
  assert.equal(state.cookieReturned, true);
  assert.equal(state.nativeIpcDenied, true);
  assert.equal(state.remoteSelectServiceDenied, true);
  assert.equal(state.operationStartCount, 1);
  assert.equal(state.signedPolls, 1);
  assert.equal(state.signedTaskReceipts, 1);
  assert.equal(state.readReceiptValid, true);
  assert.deepEqual(state.requestErrors, []);
});

test("local cloud fixture delivers open_folder then read_files and validates both receipts", async (t) => {
  const directory = mkdtempSync(join(tmpdir(), "boxy-open-folder-fixture-test-"));
  const statePath = join(directory, "state.json");
  const portPath = join(directory, "port.txt");
  const child = spawn(process.execPath, [fixture, "--state-file", statePath, "--port-file", portPath, "--include-open-folder"], {
    stdio: "ignore",
  });
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
    if (child.exitCode !== null) throw new Error("fixture process exited during startup");
    await delay(50);
  }
  assert.ok(port > 0, "fixture should publish its loopback port");
  const base = `http://127.0.0.1:${port}`;
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const publicRaw = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  await fetch(`${base}/api/v2/connector/operations`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ protocolVersion: 3, devicePublicKey: publicRaw.toString("base64url") }),
  });

  const pollPath = "/api/v2/connector/operations/packaging-smoke";
  async function poll(nonce) {
    const signature = sign(null, Buffer.from(`GET\n${pollPath}\n${nonce}`), privateKey).toString("base64url");
    const response = await fetch(`${base}${pollPath}`, {
      headers: { "x-connector-nonce": nonce, "x-connector-signature": signature },
    });
    assert.equal(response.status, 200);
    return response.json();
  }
  async function receipt(taskId, nonce, body) {
    const path = `${pollPath}/directory-tasks/${taskId}/complete`;
    const signature = sign(null, Buffer.from(`POST\n${path}\n${nonce}`), privateKey).toString("base64url");
    const response = await fetch(`${base}${path}`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-connector-nonce": nonce,
        "x-connector-signature": signature,
      },
      body: JSON.stringify(body),
    });
    assert.equal(response.status, 200);
  }

  assert.equal((await poll("open-poll")).directoryTasks[0].kind, "open_folder");
  await receipt("packaging-smoke-open-folder", "open-receipt", { ok: true });
  const nextPoll = await poll("read-poll");
  assert.equal(nextPoll.directoryTasks[0].kind, "read_files");
  const content = Buffer.from("boxy-smoke-marker");
  await receipt("packaging-smoke-read", "read-receipt", { files: [{
    path: "smoke-marker.txt",
    size: content.length,
    sha256: createHash("sha256").update(content).digest("base64url"),
    contentBase64: content.toString("base64url"),
  }] });

  const state = JSON.parse(readFileSync(statePath, "utf8"));
  assert.equal(state.signedPolls, 2);
  assert.equal(state.signedTaskReceipts, 2);
  assert.equal(state.openFolderReceiptValid, true);
  assert.equal(state.workerContinuedAfterOpenFolder, true);
  assert.equal(state.readReceiptValid, true);
  assert.deepEqual(state.requestErrors, []);
});
