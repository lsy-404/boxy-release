import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, createReadStream, createWriteStream, existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pipeline } from "node:stream/promises";
import { execFileSync } from "node:child_process";

const version = "1.75.2";
const assets = {
  "aarch64-apple-darwin": ["osx-arm64", "610ab7d48615135dfc60b763858b11a426a0d3d1160c1bb76fb0ae6011b02edd", "rclone"],
  "x86_64-apple-darwin": ["osx-amd64", "574e1f31cf1c464e5fb0325cb66fcbdeaf1efa1802bd9b17f13c256fd74b6561", "rclone"],
  "x86_64-pc-windows-msvc": ["windows-amd64", "8a9db2bad1dc79a65b3348fd857069598e2aab7cf4d118b77b1df23055aff818", "rclone.exe"],
};

const target = process.argv[2];
const asset = assets[target];
if (!asset) fail(`Unsupported target: ${target ?? "(missing)"}`);

const [platform, expectedHash, executableName] = asset;
const archiveName = `rclone-v${version}-${platform}.zip`;
const outputDir = resolve("target", "rclone", target);
const outputPath = join(outputDir, executableName);
mkdirSync(outputDir, { recursive: true });

{
  rmSync(outputPath, { force: true });
  const temporaryDir = mkdtempSync(join(tmpdir(), "boxy-rclone-"));
  const archivePath = join(temporaryDir, archiveName);
  try {
    const response = await fetch(`https://github.com/rclone/rclone/releases/download/v${version}/${archiveName}`);
    if (!response.ok || !response.body) fail(`Could not download ${archiveName}: HTTP ${response.status}`);
    await pipeline(response.body, createWriteStream(archivePath, { flags: "wx" }));
    const hash = await sha256File(archivePath);
    if (hash !== expectedHash) fail(`Checksum mismatch for ${archiveName}`);
    execFileSync("tar", ["-xf", archivePath, "-C", temporaryDir]);
    const extracted = join(temporaryDir, `rclone-v${version}-${platform}`, executableName);
    if (!existsSync(extracted)) fail(`Archive does not contain ${executableName}`);
    copyFileSync(extracted, outputPath, 1);
    if (executableName === "rclone") chmodSync(outputPath, 0o755);
  } finally {
    rmSync(temporaryDir, { recursive: true, force: true });
  }
}

const reported = execFileSync(outputPath, ["--version"], { encoding: "utf8" });
if (!new RegExp(`^rclone v${version}(?:\\s|$)`).test(reported)) fail(`Unexpected rclone version: ${reported.split("\n")[0]}`);
console.log(outputPath);

function fail(message) {
  throw new Error(message);
}

async function sha256File(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}
