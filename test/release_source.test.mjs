import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

test("release source tree excludes internal records and root README", () => {
  const tracked = execFileSync("git", ["ls-files", "--cached", "--", "agents/"], {
    cwd: repositoryRoot,
    encoding: "utf8",
  })
    .split(/\r?\n/)
    .filter(Boolean);
  const trackedReadme = execFileSync("git", ["ls-files", "--cached", "--", "README.md"], {
    cwd: repositoryRoot,
    encoding: "utf8",
  })
    .split(/\r?\n/)
    .filter(Boolean);

  assert.deepEqual(tracked, [], `tracked agent records would be public: ${tracked.join(", ")}`);
  assert.deepEqual(trackedReadme, [], `the root README would be public: ${trackedReadme.join(", ")}`);
});

test("Git source archive excludes a tracked agent directory through export-ignore", () => {
  const fixture = mkdtempSync(join(tmpdir(), "boxy-release-source-"));
  try {
    mkdirSync(join(fixture, "agents"));
    writeFileSync(
      join(fixture, ".gitattributes"),
      readFileSync(join(repositoryRoot, ".gitattributes")),
    );
    writeFileSync(join(fixture, "agents", "private.md"), "internal\n");
    writeFileSync(join(fixture, "public.txt"), "public\n");
    execFileSync("git", ["init", "--quiet"], { cwd: fixture });
    const emptyIgnore = join(fixture, ".git", "empty-ignore");
    writeFileSync(emptyIgnore, "");
    execFileSync("git", ["config", "core.excludesFile", emptyIgnore], { cwd: fixture });
    execFileSync("git", ["config", "user.name", "Release Test"], { cwd: fixture });
    execFileSync("git", ["config", "user.email", "release-test@example.invalid"], { cwd: fixture });
    execFileSync("git", ["add", ".gitattributes", "agents/private.md", "public.txt"], { cwd: fixture });
    execFileSync("git", ["commit", "--quiet", "-m", "fixture"], { cwd: fixture });

    const archive = execFileSync("git", ["archive", "--format=tar", "HEAD"], { cwd: fixture });
    const entries = execFileSync("tar", ["-tf", "-"], {
      cwd: fixture,
      encoding: "utf8",
      input: archive,
    })
      .split(/\r?\n/)
      .filter(Boolean);
    const agentEntries = entries.filter((path) => /(?:^|\/)agents(?:\/|$)/.test(path));

    assert.ok(entries.includes("public.txt"), "the archive should retain ordinary source files");
    assert.deepEqual(agentEntries, [], `source archive contains agent records: ${agentEntries.join(", ")}`);
  } finally {
    rmSync(fixture, { recursive: true, force: true });
  }
});
