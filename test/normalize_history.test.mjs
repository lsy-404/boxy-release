import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { normalizeHistory } from "../scripts/normalize-history.mjs";

function git(cwd, args, env = {}) {
  return execFileSync("git", args, {
    cwd,
    env: {
      ...process.env,
      GIT_CONFIG_GLOBAL: "",
      GIT_CONFIG_NOSYSTEM: "1",
      ...env,
    },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function setup() {
  const root = mkdtempSync(join(tmpdir(), "boxy-history-"));
  const origin = join(root, "origin.git");
  const cwd = join(root, "working");
  const emptyTemplate = join(root, "empty-template");
  mkdirSync(emptyTemplate);
  git(root, ["init", `--template=${emptyTemplate}`, "--bare", "--initial-branch=main", origin]);
  git(root, ["clone", "--no-local", "--depth=1", `--template=${emptyTemplate}`, origin, cwd]);
  git(cwd, ["config", "user.name", "Fixture Actor"]);
  git(cwd, ["config", "user.email", "fixture@example.invalid"]);
  git(cwd, ["config", "commit.gpgSign", "false"]);

  writeFileSync(join(cwd, "file.txt"), "first\n");
  git(cwd, ["add", "file.txt"]);
  git(cwd, ["commit", "-m", "first"], {
    GIT_AUTHOR_NAME: "Source Author",
    GIT_AUTHOR_EMAIL: "source-author@example.invalid",
    GIT_COMMITTER_NAME: "Source Committer",
    GIT_COMMITTER_EMAIL: "source-committer@example.invalid",
    GIT_AUTHOR_DATE: "2024-01-01T00:00:00Z",
    GIT_COMMITTER_DATE: "2024-01-01T00:00:00Z",
  });
  writeFileSync(join(cwd, "file.txt"), "complete tree\n");
  git(cwd, ["commit", "-am", "second"], {
    GIT_AUTHOR_NAME: "Source Author",
    GIT_AUTHOR_EMAIL: "source-author@example.invalid",
    GIT_COMMITTER_NAME: "Source Committer",
    GIT_COMMITTER_EMAIL: "source-committer@example.invalid",
    GIT_AUTHOR_DATE: "2024-01-02T00:00:00Z",
    GIT_COMMITTER_DATE: "2024-01-02T00:00:00Z",
  });
  const expectedSha = git(cwd, ["rev-parse", "HEAD"]);
  const tree = git(cwd, ["rev-parse", "HEAD^{tree}"]);
  git(cwd, ["tag", "v1"]);
  git(cwd, ["branch", "other"]);
  git(cwd, ["push", "origin", "main", "other", "refs/tags/v1"]);

  return {
    root,
    origin,
    cwd,
    expectedSha,
    tree,
    dispose: () => rmSync(root, { recursive: true, force: true }),
  };
}

function remoteMain(origin) {
  return git(origin, ["rev-parse", "refs/heads/main"]);
}

test("rewrites main to one unsigned epoch root commit with the same tree", (t) => {
  const fixture = setup();
  t.after(fixture.dispose);

  git(fixture.cwd, ["config", "commit.gpgSign", "true"]);
  git(fixture.cwd, ["config", "user.signingkey", "invalid-key-that-must-not-be-used"]);

  const ambientIdentity = {
    GIT_AUTHOR_NAME: process.env.GIT_AUTHOR_NAME,
    GIT_AUTHOR_EMAIL: process.env.GIT_AUTHOR_EMAIL,
    GIT_COMMITTER_NAME: process.env.GIT_COMMITTER_NAME,
    GIT_COMMITTER_EMAIL: process.env.GIT_COMMITTER_EMAIL,
  };
  Object.assign(process.env, {
    GIT_AUTHOR_NAME: "Ambient Actor",
    GIT_AUTHOR_EMAIL: "ambient-author@example.invalid",
    GIT_COMMITTER_NAME: "Ambient Actor",
    GIT_COMMITTER_EMAIL: "ambient-committer@example.invalid",
  });
  let result;
  try {
    result = normalizeHistory({ cwd: fixture.cwd, expectedSha: fixture.expectedSha });
  } finally {
    for (const [key, value] of Object.entries(ambientIdentity)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
  assert.equal(result.status, "updated");
  assert.equal(remoteMain(fixture.origin), result.sha);
  assert.equal(git(fixture.cwd, ["rev-list", "--count", result.sha]), "1");
  assert.equal(git(fixture.cwd, ["rev-parse", `${result.sha}^{tree}`]), fixture.tree);

  const raw = git(fixture.cwd, ["cat-file", "-p", result.sha]);
  assert.match(raw, /^tree [0-9a-f]+\nauthor Source Author <source-author@example\.invalid> 0 \+0000\ncommitter Source Committer <source-committer@example\.invalid> 0 \+0000\n\nUpdate Boxy$/);
  assert.doesNotMatch(raw, /^(?:parent |gpgsig )/m);
  assert.equal(git(fixture.cwd, ["show", "-s", "--format=%at %ct", result.sha]), "0 0");
  assert.equal(git(fixture.origin, ["rev-parse", "refs/heads/other"]), fixture.expectedSha);
  assert.equal(git(fixture.origin, ["rev-parse", "refs/tags/v1"]), fixture.expectedSha);
});

test("stale event leaves remote refs untouched", (t) => {
  const fixture = setup();
  t.after(fixture.dispose);
  const staleHead = fixture.expectedSha;
  writeFileSync(join(fixture.cwd, "later.txt"), "newer remote state\n");
  git(fixture.cwd, ["add", "later.txt"]);
  git(fixture.cwd, ["commit", "-m", "newer"]);
  const newerSha = git(fixture.cwd, ["rev-parse", "HEAD"]);
  git(fixture.cwd, ["push", "origin", "main"]);
  git(fixture.cwd, ["reset", "--hard", staleHead]);

  const result = normalizeHistory({ cwd: fixture.cwd, expectedSha: staleHead });
  assert.equal(result.status, "stale");
  assert.equal(result.remoteTip, newerSha);
  assert.equal(remoteMain(fixture.origin), newerSha);
  assert.equal(result.tree, fixture.tree);
});

test("repeated execution is idempotent and does not push again", (t) => {
  const fixture = setup();
  t.after(fixture.dispose);
  const first = normalizeHistory({ cwd: fixture.cwd, expectedSha: fixture.expectedSha });
  const hook = join(fixture.cwd, ".git", "hooks", "pre-push");
  mkdirSync(join(fixture.cwd, ".git", "hooks"), { recursive: true });
  git(fixture.cwd, ["config", "core.hooksPath", join(fixture.cwd, ".git", "hooks")]);
  writeFileSync(hook, "#!/bin/sh\nexit 1\n");
  chmodSync(hook, 0o755);

  const second = normalizeHistory({ cwd: fixture.cwd, expectedSha: fixture.expectedSha });
  assert.equal(second.status, "already-normalized");
  assert.equal(second.sha, first.sha);
  assert.equal(remoteMain(fixture.origin), first.sha);
  assert.equal(readFileSync(hook, "utf8"), "#!/bin/sh\nexit 1\n");
});

test("lease rejects a concurrent main update without overwriting it", (t) => {
  const fixture = setup();
  t.after(fixture.dispose);
  writeFileSync(join(fixture.cwd, "concurrent.txt"), "concurrent main\n");
  git(fixture.cwd, ["add", "concurrent.txt"]);
  git(fixture.cwd, ["commit", "-m", "concurrent"]);
  const concurrentSha = git(fixture.cwd, ["rev-parse", "HEAD"]);
  git(fixture.cwd, ["push", "origin", `${concurrentSha}:refs/heads/concurrent`]);
  git(fixture.cwd, ["reset", "--hard", fixture.expectedSha]);
  const hooks = join(fixture.root, "hooks");
  mkdirSync(hooks);
  const hook = join(hooks, "pre-push");
  const originPath = fixture.origin.replaceAll("\\", "/");
  writeFileSync(
    hook,
    [
      "#!/bin/sh",
      "unset GIT_DIR GIT_WORK_TREE",
      "git --git-dir='" + originPath + "' update-ref refs/heads/main " + concurrentSha,
      "",
    ].join("\n"),
  );
  chmodSync(hook, 0o755);
  git(fixture.cwd, ["config", "core.hooksPath", hooks]);

  assert.throws(
    () => normalizeHistory({ cwd: fixture.cwd, expectedSha: fixture.expectedSha }),
    /stale info|rejected|failed to push|fetch first/i,
  );
  assert.equal(remoteMain(fixture.origin), concurrentSha);
});

test("requires a full SHA matching HEAD", (t) => {
  const fixture = setup();
  t.after(fixture.dispose);
  assert.throws(() => normalizeHistory({ cwd: fixture.cwd }), /full Git commit SHA/);
  assert.throws(() => normalizeHistory({ cwd: fixture.cwd, expectedSha: "bad" }), /full Git commit SHA/);
  assert.throws(() => normalizeHistory({ cwd: fixture.cwd, expectedSha: "0".repeat(40) }), /HEAD does not match/);
  assert.equal(remoteMain(fixture.origin), fixture.expectedSha);
});

test("CLI reads GITHUB_SHA and reports the update", (t) => {
  const fixture = setup();
  t.after(fixture.dispose);

  const output = execFileSync(
    process.execPath,
    [fileURLToPath(new URL("../scripts/normalize-history.mjs", import.meta.url))],
    {
      cwd: fixture.cwd,
      env: {
        ...process.env,
        GITHUB_SHA: fixture.expectedSha,
        GIT_CONFIG_GLOBAL: "",
        GIT_CONFIG_NOSYSTEM: "1",
      },
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    },
  );

  const result = JSON.parse(output);
  assert.equal(result.status, "updated");
  assert.equal(remoteMain(fixture.origin), result.sha);
});
