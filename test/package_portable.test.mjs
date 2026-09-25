import { execFileSync, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, mkdirSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
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
  const runtimePath = join(fixtureRoot, "runtime");
  mkdirSync(runtimePath);
  writeFileSync(binaryPath, "fixture binary");
  writeFileSync(join(runtimePath, "msedgewebview2.exe"), "fixture runtime");
  writeFileSync(join(runtimePath, "msedge.dll"), "fixture library");

  execFileSync(process.execPath, [join(fixtureRoot, "scripts", "package-portable.mjs"), "x86_64-pc-windows-msvc", "--binary", binaryPath, "--webview-runtime", runtimePath], {
    cwd: fixtureRoot,
    stdio: "inherit",
  });

  const archivePath = join(fixtureRoot, "dist", "boxy-windows-x64.zip");
  if (!existsSync(archivePath)) throw new Error("Windows archive was not created");
  const entries = execFileSync("tar", ["-tf", archivePath], { encoding: "utf8" });
  for (const entry of ["boxy-windows-x64/boxy.exe", "boxy-windows-x64/WebView2Runtime/msedgewebview2.exe", "boxy-windows-x64/WebView2Runtime/msedge.dll"]) {
    if (!entries.includes(entry)) throw new Error(`Archive is missing ${entry}`);
  }

  unlinkSync(join(runtimePath, "msedge.dll"));
  const incompleteRuntime = spawnSync(process.execPath, [join(fixtureRoot, "scripts", "package-portable.mjs"), "x86_64-pc-windows-msvc", "--binary", binaryPath, "--webview-runtime", runtimePath], {
    cwd: fixtureRoot,
    encoding: "utf8",
  });
  if (incompleteRuntime.status === 0 || !incompleteRuntime.stderr.includes("msedge.dll")) {
    throw new Error("Incomplete WebView2 Runtime was accepted");
  }
} finally {
  rmSync(fixtureRoot, { recursive: true, force: true });
}
