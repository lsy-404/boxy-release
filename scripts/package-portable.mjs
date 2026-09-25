import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputDir = resolve(repositoryRoot, "dist");

let targetTriple;
let binaryOverride;
const args = process.argv.slice(2);
for (let index = 0; index < args.length; index += 1) {
  const arg = args[index];
  if (arg === "--binary") {
    binaryOverride = args[index + 1];
    index += 1;
    if (!binaryOverride) {
      fail("--binary requires a path.");
    }
  } else if (arg.startsWith("--")) {
    fail(`Unknown option: ${arg}`);
  } else if (!targetTriple) {
    targetTriple = arg;
  } else {
    fail(`Unexpected argument: ${arg}`);
  }
}

if (!targetTriple) {
  fail("Usage: node scripts/package-portable.mjs <target-triple> [--binary <path>]");
}

const isWindows = targetTriple.includes("windows");
const isMac = targetTriple.includes("apple-darwin");
if (!isWindows && !isMac) {
  fail(`Unsupported target triple: ${targetTriple}`);
}

const binaryName = isWindows ? "boxy.exe" : "boxy";
const defaultBinary = resolve(repositoryRoot, "target", targetTriple, "release", binaryName);
const binaryPath = binaryOverride
  ? resolve(process.cwd(), binaryOverride)
  : existsSync(defaultBinary)
    ? defaultBinary
    : resolve(repositoryRoot, "target", "release", binaryName);

if (!existsSync(binaryPath) || !statSync(binaryPath).isFile()) {
  fail(`Boxy binary not found: ${binaryPath}`);
}

function main() {
  if (isWindows) {
    packageWindows();
  } else {
    packageMac();
  }
}

function packageMac() {
  const appDir = join(outputDir, "Boxy.app");
  const contentsDir = join(appDir, "Contents");
  const macosDir = join(contentsDir, "MacOS");
  const resourcesDir = join(contentsDir, "Resources");
  const version = readCargoVersion();

  rmSync(appDir, { recursive: true, force: true });
  mkdirSync(macosDir, { recursive: true });
  mkdirSync(resourcesDir, { recursive: true });

  const appBinary = join(macosDir, "boxy");
  copyFileSync(binaryPath, appBinary);
  chmodSync(appBinary, 0o755);
  writeFileSync(join(contentsDir, "Info.plist"), infoPlist(version));

  writeFileSync(join(resourcesDir, "boxy-pen.svg"), readFileSync(resolve(repositoryRoot, "assets/boxy-pen.svg")));

  try {
    execFileSync("codesign", ["--force", "--deep", "--sign", "-", appDir], { stdio: "ignore" });
  } catch {
    console.warn("Ad-hoc codesign skipped or failed; continuing.");
  }

  const arch = targetTriple.startsWith("aarch64") ? "arm64" : "x64";
  const zipPath = join(outputDir, `boxy-macos-${arch}.zip`);
  rmSync(zipPath, { force: true });
  try {
    execFileSync("ditto", ["-c", "-k", "--sequesterRsrc", "--keepParent", "Boxy.app", zipPath], {
      cwd: outputDir,
      stdio: "ignore",
    });
  } catch {
    execFileSync("zip", ["-r", "-q", basename(zipPath), "Boxy.app"], { cwd: outputDir, stdio: "ignore" });
  }

  report([appDir, zipPath]);
}

function packageWindows() {
  const bundleName = "boxy-windows-x64";
  const bundleDir = join(outputDir, bundleName);
  const zipPath = join(outputDir, `${bundleName}.zip`);
  mkdirSync(outputDir, { recursive: true });
  rmSync(bundleDir, { recursive: true, force: true });
  rmSync(zipPath, { force: true });
  mkdirSync(bundleDir, { recursive: true });
  const exePath = join(bundleDir, "boxy.exe");
  copyFileSync(binaryPath, exePath);
  chmodSync(exePath, 0o755);
  createWindowsZip(bundleName, zipPath);

  report([bundleDir, zipPath]);
}

function createWindowsZip(bundleName, zipPath) {
  try {
    execFileSync("tar", ["-a", "-c", "-f", zipPath, "-C", outputDir, bundleName], { stdio: "ignore" });
  } catch {
    execFileSync(
      "powershell",
      ["-NoProfile", "-Command", "Compress-Archive -LiteralPath $args[0] -DestinationPath $args[1] -Force", join(outputDir, bundleName), zipPath],
      { stdio: "ignore" },
    );
  }
  if (!existsSync(zipPath) || !statSync(zipPath).isFile()) {
    fail("Windows bundle archive was not created.");
  }
}

function infoPlist(version) {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
\t<key>CFBundleName</key>
\t<string>Boxy</string>
\t<key>CFBundleDisplayName</key>
\t<string>Boxy</string>
\t<key>CFBundleExecutable</key>
\t<string>boxy</string>
\t<key>CFBundleIdentifier</key>
\t<string>dev.voidcarve.boxy</string>
\t<key>CFBundleVersion</key>
\t<string>${version}</string>
\t<key>CFBundleShortVersionString</key>
\t<string>${version}</string>
\t<key>CFBundlePackageType</key>
\t<string>APPL</string>
\t<key>LSMinimumSystemVersion</key>
\t<string>11.0</string>
\t<key>NSHighResolutionCapable</key>
\t<true/>
</dict>
</plist>
`;
}

function readCargoVersion() {
  const cargoToml = readFileSync(resolve(repositoryRoot, "connector", "Cargo.toml"), "utf8");
  const match = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    fail("Could not read version from connector/Cargo.toml.");
  }
  return match[1];
}

function report(artifacts) {
  for (const artifact of artifacts) {
    console.log(`${artifact} (${sizeOf(artifact)} bytes)`);
  }
}

function sizeOf(path) {
  const stats = statSync(path);
  if (!stats.isDirectory()) {
    return stats.size;
  }
  return readdirSize(path);
}

function readdirSize(path) {
  let total = 0;
  for (const name of readdirSync(path)) {
    const entry = join(path, name);
    total += statSync(entry).isDirectory() ? readdirSize(entry) : statSync(entry).size;
  }
  return total;
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

main();
