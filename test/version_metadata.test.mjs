import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const cargoToml = readFileSync(new URL("../connector/Cargo.toml", import.meta.url), "utf8");
const cargoLock = readFileSync(new URL("../Cargo.lock", import.meta.url), "utf8");
const tauriConfig = JSON.parse(readFileSync(new URL("../connector/tauri.conf.json", import.meta.url), "utf8"));

function versionFrom(pattern, source, label) {
  const match = source.match(pattern);
  assert.ok(match, `Could not read ${label} version`);
  return match[1];
}

test("release tag, Cargo package, Cargo lock, and Tauri bundle versions agree", () => {
  const manifestVersion = versionFrom(/^name = "boxy"\r?\nversion = "([^"]+)"/m, cargoToml, "Cargo manifest");
  const lockVersion = versionFrom(/^\[\[package\]\]\r?\nname = "boxy"\r?\nversion = "([^"]+)"/m, cargoLock, "Cargo lock");
  const versions = [manifestVersion, lockVersion, tauriConfig.version];

  assert.ok(versions.every((version) => typeof version === "string"), "Every version source must be present");
  assert.equal(new Set(versions).size, 1, `Version mismatch: ${versions.join(", ")}`);
  if (process.env.RELEASE_TAG) {
    assert.equal(process.env.RELEASE_TAG, `v${manifestVersion}`, "Release tag must match the client bundle version");
  }
});
