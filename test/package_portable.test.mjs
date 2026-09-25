import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = join(fileURLToPath(new URL("..", import.meta.url)));
const fixtureRoot = mkdtempSync(join(tmpdir(), "boxy-package-"));

try {
  mkdirSync(join(fixtureRoot, "scripts"), { recursive: true });
  mkdirSync(join(fixtureRoot, "connector"), { recursive: true });
  cpSync(join(repositoryRoot, "scripts", "package-portable.mjs"), join(fixtureRoot, "scripts", "package-portable.mjs"));
  writeFileSync(join(fixtureRoot, "connector", "Cargo.toml"), '[package]\nversion = "2.0.0"\n');

  const binaryPath = join(fixtureRoot, "boxy.exe");
  writeFileSync(binaryPath, "fixture binary");

  execFileSync(process.execPath, [join(fixtureRoot, "scripts", "package-portable.mjs"), "x86_64-pc-windows-msvc", "--binary", binaryPath], {
    cwd: fixtureRoot,
    stdio: "inherit",
  });

  const archivePath = join(fixtureRoot, "dist", "boxy-windows-x64.zip");
  if (!existsSync(archivePath)) throw new Error("Windows archive was not created");
  const entries = execFileSync("tar", ["-tf", archivePath], { encoding: "utf8" });
  if (!entries.includes("boxy-windows-x64/boxy.exe")) throw new Error("Archive is missing boxy.exe");
  if (entries.includes("WebView2Runtime/")) throw new Error("Windows archive unexpectedly includes the on-demand runtime");
} finally {
  rmSync(fixtureRoot, { recursive: true, force: true });
}
