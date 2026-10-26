import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = fileURLToPath(new URL("..", import.meta.url));
const html = readFileSync(`${root}/connector/bootstrap/index.html`, "utf8");
const script = readFileSync(`${root}/connector/bootstrap/selection.js`, "utf8");

test("startup contains one blank, accessible URL field and submits on Enter", () => {
  assert.match(html, /<title><\/title>/);
  assert.equal((html.match(/<input\b/g) ?? []).length, 1);
  assert.match(html, /id="service-url"[^>]*type="text"/);
  assert.match(html, /id="service-url"[^>]*aria-label="Service address"/);
  assert.match(html, /id="service-url"[^>]*autofocus/);
  assert.doesNotMatch(html, /<h[1-6]\b|<button\b|<select\b|<textarea\b/);
  assert.doesNotMatch(html, /<p(?![^>]*id="error")/);
  assert.match(script, /service-form.*submit/s);
  assert.match(script, /invoke\('select_service'/);
  assert.doesNotMatch(script, /session|device|inventory|open_folder/i);
});

test("startup cannot infer a server from environment or command-line options", () => {
  const launch = readFileSync(`${root}/connector/src/launch.rs`, "utf8");
  assert.doesNotMatch(launch, /BOXY_API_URL|current_exe|inferred_service_url|session_path|FileDialog/);
});
