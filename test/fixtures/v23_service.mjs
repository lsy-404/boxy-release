import assert from "node:assert/strict";
import { createPublicKey, verify } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";

function option(name) {
  const index = process.argv.indexOf(name);
  if (index < 0 || !process.argv[index + 1]) throw new Error(`missing ${name}`);
  return process.argv[index + 1];
}

const statePath = option("--state-file");
const portPath = option("--port-file");
const root = option("--root");
const manifest = readFileSync(new URL("../../connector/Cargo.toml", import.meta.url), "utf8");
const connectorVersion = /^version\s*=\s*"([^"]+)"/m.exec(manifest)?.[1];
if (!connectorVersion) throw new Error("connector package version is missing");
const state = {
  errors: [],
  hello: null,
  browserLoaded: false,
  browserCookie: null,
  polls: [],
  taskSentAfterReady: false,
  receipt: null,
  receiptValid: false,
  closed: false,
};
let publicKey;
let pollCount = 0;
let taskSent = false;
let connectionCount = 0;

function save() {
  writeFileSync(statePath, JSON.stringify(state));
}

function body(request) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = 0;
    request.on("data", (chunk) => {
      size += chunk.length;
      if (size > 4 * 1024 * 1024) reject(new Error("request body too large"));
      else chunks.push(chunk);
    });
    request.on("end", () => {
      try { resolve(JSON.parse(Buffer.concat(chunks).toString("utf8"))); }
      catch (error) { reject(error); }
    });
    request.on("error", reject);
  });
}

function send(response, value, status = 200, contentType = "application/json") {
  response.writeHead(status, { "content-type": contentType, "cache-control": "no-store" });
  response.end(contentType === "application/json" ? JSON.stringify(value) : value);
}

function signed(request) {
  assert.ok(publicKey, "signed request before anonymous hello");
  const nonce = request.headers["x-connector-nonce"];
  const signature = request.headers["x-connector-signature"];
  assert.equal(typeof nonce, "string");
  assert.equal(typeof signature, "string");
  return verify(
    null,
    Buffer.from(`${request.method}\n${request.url}\n${nonce}`),
    publicKey,
    Buffer.from(signature, "base64url"),
  );
}

const server = createServer(async (request, response) => {
  try {
    if (request.url.startsWith("/api/")) {
      assert.equal(request.headers["x-boxy-connector-version"], connectorVersion);
    }
    if (request.method === "GET" && request.url === "/") {
      const expectedCookie = "boxy_connection=synthetic-session";
      assert.equal(request.headers.cookie, expectedCookie, "browser cookie must arrive on the first root request");
      state.browserLoaded = true;
      state.browserCookie = request.headers.cookie;
      state.browserUserAgent = request.headers["user-agent"] ?? null;
      save();
      send(response, "<!doctype html><html><head><title>Remote Service</title></head><body>Ready</body></html>", 200, "text/html");
      return;
    }
    if (request.method === "POST" && request.url === "/api/connect") {
      connectionCount += 1;
      assert.equal(connectionCount, 1, "only one connection should be started");
      const hello = await body(request);
      assert.deepEqual(Object.keys(hello).sort(), ["capabilities", "platform", "protocolVersion", "publicKey"].sort());
      assert.equal(hello.protocolVersion, 4);
      assert.ok(["windows", "macos"].includes(hello.platform));
      assert.ok(Array.isArray(hello.capabilities));
      const rawKey = Buffer.from(hello.publicKey, "base64url");
      assert.equal(rawKey.length, 32);
      publicKey = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), rawKey]), format: "der", type: "spki" });
      state.hello = hello;
      save();
      const port = server.address().port;
      send(response, {
        protocolVersion: 4,
        connectionId: "smoke_connection",
        browserUrl: `http://127.0.0.1:${port}/`,
        browserCookies: [{ name: "boxy_connection", value: "synthetic-session" }],
        expiresAt: Date.now() + 120_000,
      });
      return;
    }
    if (request.method === "GET" && request.url === "/api/connections/smoke_connection") {
      assert.ok(signed(request), "invalid signed poll");
      pollCount += 1;
      if (pollCount === 1) {
        state.polls.push({ state: "awaiting_auth", taskCount: 0 });
        save();
        send(response, { protocolVersion: 4, state: "awaiting_auth", tasks: [] });
      } else if (!taskSent) {
        taskSent = true;
        state.taskSentAfterReady = true;
        state.polls.push({ state: "ready", taskCount: 1 });
        save();
        send(response, {
          protocolVersion: 4,
          state: "ready",
          tasks: [{ id: "scope_list", kind: "fs.list", params: { root, paths: ["."] } }],
        });
      } else {
        state.polls.push({ state: "ready", taskCount: 0 });
        save();
        send(response, { protocolVersion: 4, state: "ready", tasks: [] });
      }
      return;
    }
    if (request.method === "POST" && request.url === "/api/connections/smoke_connection/tasks/scope_list/receipt") {
      assert.ok(signed(request), "invalid signed task receipt");
      state.receipt = await body(request);
      const files = state.receipt.result?.files;
      state.receiptValid = state.receipt.ok === true && taskSent && state.polls.some((poll) => poll.state === "awaiting_auth")
        && files?.some((file) => file.path === "scope/probe.txt" && file.kind === "file" && !file.error);
      assert.ok(state.receiptValid, "scoped file listing receipt was not valid");
      save();
      send(response, { accepted: true });
      return;
    }
    if (request.method === "POST" && request.url === "/api/connections/smoke_connection/close") {
      assert.ok(signed(request), "invalid signed close request");
      state.closed = true;
      save();
      send(response, { closed: true });
      return;
    }
    send(response, { error: "not_found" }, 404);
  } catch (error) {
    state.errors.push(String(error?.message ?? error));
    save();
    send(response, { error: "fixture_rejected_request" }, 400);
  }
});

server.listen(0, "127.0.0.1", () => {
  writeFileSync(portPath, String(server.address().port));
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.close(() => process.exit(0)));
}
