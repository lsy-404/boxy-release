import { createPublicKey, verify } from "node:crypto";
import { writeFileSync } from "node:fs";
import { createServer } from "node:http";

const options = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  options.set(process.argv[index], process.argv[index + 1]);
}
const statePath = options.get("--state-file");
const portPath = options.get("--port-file");
const enableExport = process.argv.includes("--enable-export");
const probeSelectService = process.argv.includes("--probe-select-service");
if (!statePath || !portPath) throw new Error("--state-file and --port-file are required");

const state = {
  editorLoaded: false,
  readyFetch: false,
  cookieReturned: false,
  nativeIpcDenied: false,
  remoteSelectServiceDenied: false,
  operationStartCount: 0,
  exportStarted: false,
  startProtocolVersion: null,
  signedPolls: 0,
  signedSnapshots: 0,
  signedTaskReceipts: 0,
  readReceiptValid: false,
  requestErrors: [],
};
let publicKey;
let taskSent = false;

function saveState() {
  writeFileSync(statePath, JSON.stringify(state));
}

function send(response, status, body, contentType = "application/json", extraHeaders = {}) {
  const bytes = Buffer.from(body);
  response.writeHead(status, {
    "Content-Type": contentType,
    "Content-Length": bytes.length,
    "Cache-Control": "no-store",
    Connection: "close",
    ...extraHeaders,
  });
  response.end(bytes);
}

async function readJson(request) {
  const chunks = [];
  let length = 0;
  for await (const chunk of request) {
    length += chunk.length;
    if (length > 4 * 1024 * 1024) throw new Error("request_too_large");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

function verifyRequest(request, method, path) {
  const nonce = request.headers["x-connector-nonce"];
  const encodedSignature = request.headers["x-connector-signature"];
  if (!publicKey || typeof nonce !== "string" || typeof encodedSignature !== "string") {
    throw new Error("missing_signature");
  }
  const signature = Buffer.from(encodedSignature, "base64url");
  const signed = Buffer.from(`${method}\n${path}\n${nonce}`, "utf8");
  if (!verify(null, signed, publicKey, signature)) throw new Error("invalid_signature");
}

const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, "http://127.0.0.1");
    if (request.method === "POST" && url.pathname === "/api/v2/connector/operations") {
      const body = await readJson(request);
      state.operationStartCount += 1;
      if (state.operationStartCount > 1) {
        saveState();
        return send(response, 409, JSON.stringify({ error: "operation_already_started" }));
      }
      state.startProtocolVersion = body.protocolVersion;
      if (body.protocolVersion !== 3) throw new Error("wrong_protocol_version");
      const rawPublicKey = Buffer.from(body.devicePublicKey ?? "", "base64url");
      if (rawPublicKey.length !== 32) throw new Error("invalid_device_key");
      publicKey = createPublicKey({
        key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), rawPublicKey]),
        format: "der",
        type: "spki",
      });
      saveState();
      return send(response, 200, JSON.stringify({
        protocolVersion: 3,
        operationId: "packaging-smoke",
        browserUrl: `http://127.0.0.1:${server.address().port}/d_${"a".repeat(32)}/?authorization=${"b".repeat(43)}`,
      }));
    }

    if (request.method === "GET" && url.pathname === `/d_${"a".repeat(32)}/`) {
      state.editorLoaded = true;
      saveState();
      return send(response, 200, '<!doctype html><html><head><meta charset="utf-8"><title>Boxy fixture</title><script src="/smoke.js"></script></head><body></body></html>', "text/html; charset=utf-8", {
        "Set-Cookie": "boxy-auth=fixture-auth; HttpOnly; SameSite=Lax; Path=/",
        "Content-Security-Policy": "default-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'",
      });
    }
    if (request.method === "GET" && url.pathname === "/smoke.js") {
      const exportScript = enableExport
        ? 'await fetch("/export-started");const blob=new Blob(["boxy-export-fixture\\n"],{type:"text/plain"});const link=document.createElement("a");link.href=URL.createObjectURL(blob);link.download="boxy-export-fixture.txt";link.click();'
        : "";
      const serviceProbe = probeSelectService
        ? 'let service="absent";if(typeof call==="function"){try{service=await Promise.race([call("select_service",{serverUrl:location.origin}).then(()=>"allowed",()=>"denied"),new Promise(r=>setTimeout(()=>r("timeout"),1000))])}catch{service="denied"}}'
        : 'const service="not_probed";';
      const script = `async function probe(){let ipc="absent";const call=window.__TAURI_INTERNALS__?.invoke;if(typeof call==="function"){try{ipc=await Promise.race([call("plugin:window|set_title",{label:"editor",title:"ipc-probe"}).then(()=>"allowed",()=>"denied"),new Promise(r=>setTimeout(()=>r("timeout"),1000))])}catch{ipc="denied"}}${serviceProbe}const response=await fetch(\`/ready?ipc=\${ipc}&select=\${service}\`);document.body.dataset.ready=response.ok?"yes":"error";if(response.ok){${exportScript}}}probe().catch(()=>document.body.dataset.ready="error")`;
      return send(response, 200, script, "text/javascript; charset=utf-8");
    }
    if (request.method === "GET" && url.pathname === "/export-started" && enableExport) {
      state.exportStarted = true;
      saveState();
      return send(response, 200, "started", "text/plain; charset=utf-8");
    }
    if (request.method === "GET" && url.pathname === "/ready") {
      state.cookieReturned = request.headers.cookie === "boxy-auth=fixture-auth";
      const ipcProbe = url.searchParams.get("ipc") ?? "absent";
      state.nativeIpcDenied = ipcProbe === "absent" || ipcProbe === "denied";
      const serviceProbe = url.searchParams.get("select") ?? "not_probed";
      state.remoteSelectServiceDenied = serviceProbe === "absent" || serviceProbe === "denied";
      state.readyFetch = state.cookieReturned && state.nativeIpcDenied
        && (!probeSelectService || state.remoteSelectServiceDenied);
      saveState();
      const allowed = state.cookieReturned && state.nativeIpcDenied;
      return send(response, allowed ? 200 : 403, allowed ? "ready" : "cookie_or_ipc_failed", "text/plain; charset=utf-8");
    }

    const pollPath = "/api/v2/connector/operations/packaging-smoke";
    if (request.method === "POST" && url.pathname === `${pollPath}/session-snapshot`) {
      const body = await readJson(request);
      verifyRequest(request, "POST", `${pollPath}/session-snapshot`);
      state.signedSnapshots += 1;
      saveState();
      return send(response, 200, JSON.stringify({ state: "accepted", sourceSha256: body.sourceSha256 }));
    }
    if (request.method === "GET" && url.pathname === pollPath) {
      verifyRequest(request, "GET", pollPath);
      state.signedPolls += 1;
      const directoryTasks = taskSent ? [] : [{
        id: "packaging-smoke-read",
        kind: "read_files",
        paths: ["smoke-marker.txt"],
        maxFileBytes: 64,
        maxTotalBytes: 64,
      }];
      taskSent = true;
      saveState();
      return send(response, 200, JSON.stringify({ protocolVersion: 3, state: "waiting", directoryTasks }));
    }
    if (request.method === "POST" && url.pathname === `${pollPath}/directory-tasks/packaging-smoke-read/complete`) {
      const body = await readJson(request);
      verifyRequest(request, "POST", `${pollPath}/directory-tasks/packaging-smoke-read/complete`);
      state.signedTaskReceipts += 1;
      const file = body.files?.[0];
      state.readReceiptValid = file?.path === "smoke-marker.txt"
        && file?.contentBase64 === Buffer.from("boxy-smoke-marker").toString("base64url")
        && typeof file.sha256 === "string"
        && file.size === Buffer.byteLength("boxy-smoke-marker");
      saveState();
      return send(response, 200, "{}");
    }

    return send(response, 404, JSON.stringify({ error: "not_found" }));
  } catch (error) {
    state.requestErrors.push(String(error?.message ?? "fixture_error"));
    saveState();
    if (!response.headersSent) send(response, 400, JSON.stringify({ error: "fixture_rejected" }));
    else response.destroy();
  }
});

server.listen(0, "127.0.0.1", () => {
  writeFileSync(portPath, String(server.address().port));
  saveState();
});

function stop() {
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(1), 3000).unref();
}
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
