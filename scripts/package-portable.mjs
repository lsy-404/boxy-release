import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateRawSync } from "node:zlib";

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

  const icon = readRecoveredIcon();
  if (icon) {
    writeFileSync(join(resourcesDir, "boxy-pen.svg"), icon);
  }

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
  const exePath = join(outputDir, "boxy-windows-x64.exe");
  const zipPath = join(outputDir, "boxy-windows-x64.zip");
  mkdirSync(outputDir, { recursive: true });
  copyFileSync(binaryPath, exePath);
  chmodSync(exePath, 0o755);

  rmSync(zipPath, { force: true });
  try {
    execFileSync("zip", ["-j", "-q", basename(zipPath), basename(exePath)], { cwd: outputDir, stdio: "ignore" });
  } catch {
    writeFileSync(zipPath, makeZip([{ name: basename(exePath), data: readFileSync(exePath) }]));
  }

  report([exePath, zipPath]);
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

function readRecoveredIcon() {
  try {
    return execFileSync("git", ["show", "7285e27:src-tauri/icons/boxy-pen.svg"], {
      cwd: repositoryRoot,
      maxBuffer: 16 * 1024 * 1024,
    });
  } catch {
    console.warn("Recovered Boxy icon unavailable; skipping resource.");
    return null;
  }
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

function makeZip(entries) {
  const chunks = [];
  const central = [];
  let offset = 0;
  const { time, date } = dosDateTime(new Date());

  for (const entry of entries) {
    const name = Buffer.from(entry.name, "utf8");
    const crc = crc32(entry.data);
    const compressed = deflateRawSync(entry.data);

    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0, 6);
    local.writeUInt16LE(8, 8);
    local.writeUInt16LE(time, 10);
    local.writeUInt16LE(date, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(compressed.length, 18);
    local.writeUInt32LE(entry.data.length, 22);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt16LE(0, 28);

    chunks.push(local, name, compressed);

    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(20, 6);
    header.writeUInt16LE(0, 8);
    header.writeUInt16LE(8, 10);
    header.writeUInt16LE(time, 12);
    header.writeUInt16LE(date, 14);
    header.writeUInt32LE(crc, 16);
    header.writeUInt32LE(compressed.length, 20);
    header.writeUInt32LE(entry.data.length, 24);
    header.writeUInt16LE(name.length, 28);
    header.writeUInt16LE(0, 30);
    header.writeUInt16LE(0, 32);
    header.writeUInt16LE(0, 34);
    header.writeUInt16LE(0, 36);
    header.writeUInt32LE(0, 38);
    header.writeUInt32LE(offset, 42);
    central.push(header, name);

    offset += local.length + name.length + compressed.length;
  }

  const centralBuffer = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(0, 4);
  end.writeUInt16LE(0, 6);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralBuffer.length, 12);
  end.writeUInt32LE(offset, 16);
  end.writeUInt16LE(0, 20);

  return Buffer.concat([...chunks, centralBuffer, end]);
}

function dosDateTime(date) {
  const time = (date.getHours() << 11) | (date.getMinutes() << 5) | (date.getSeconds() >> 1);
  const day = ((Math.max(date.getFullYear(), 1980) - 1980) << 9) | ((date.getMonth() + 1) << 5) | date.getDate();
  return { time: time & 0xffff, date: day & 0xffff };
}

function crc32(buffer) {
  const table = crcTable();
  let value = 0xffffffff;
  for (let index = 0; index < buffer.length; index += 1) {
    value = table[(value ^ buffer[index]) & 0xff] ^ (value >>> 8);
  }
  return (value ^ 0xffffffff) >>> 0;
}

let cachedCrcTable;

function crcTable() {
  if (cachedCrcTable) {
    return cachedCrcTable;
  }
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let value = n;
    for (let bit = 0; bit < 8; bit += 1) {
      value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
    }
    table[n] = value >>> 0;
  }
  cachedCrcTable = table;
  return table;
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

main();
