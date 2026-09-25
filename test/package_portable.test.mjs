import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
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

  const executablePath = join(fixtureRoot, "dist", "boxy-windows-x64.exe");
  if (!existsSync(executablePath)) throw new Error("Windows executable was not packaged");
  if (readFileSync(executablePath, "utf8") !== "fixture binary") throw new Error("Windows executable contents changed");
  if (existsSync(join(fixtureRoot, "dist", "boxy-windows-x64.zip"))) throw new Error("Windows package should not be a ZIP");
} finally {
  rmSync(fixtureRoot, { recursive: true, force: true });
}
