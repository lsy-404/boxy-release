import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const SHA_PATTERN = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/i;
const ROOT_DATE = {
  GIT_AUTHOR_DATE: "@0 +0000",
  GIT_COMMITTER_DATE: "@0 +0000",
};

function git(cwd, args, env = {}) {
  return execFileSync("git", args, {
    cwd,
    env: { ...process.env, ...env },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

export function normalizeHistory({ cwd = process.cwd(), expectedSha } = {}) {
  if (typeof expectedSha !== "string" || !SHA_PATTERN.test(expectedSha)) {
    throw new Error("expectedSha must be a full Git commit SHA");
  }

  const head = git(cwd, ["rev-parse", "--verify", "HEAD"]);
  if (head.toLowerCase() !== expectedSha.toLowerCase()) {
    throw new Error("HEAD does not match expectedSha");
  }

  const [authorName, authorEmail, committerName, committerEmail] = git(cwd, [
    "show",
    "-s",
    "--format=%an%x00%ae%x00%cn%x00%ce",
    expectedSha,
  ]).split("\0");
  const tree = git(cwd, ["rev-parse", "--verify", `${expectedSha}^{tree}`]);
  const normalizedSha = git(
    cwd,
    ["-c", "commit.gpgSign=false", "commit-tree", tree, "--no-gpg-sign", "-m", "Update Boxy"],
    {
      GIT_AUTHOR_NAME: authorName,
      GIT_AUTHOR_EMAIL: authorEmail,
      GIT_COMMITTER_NAME: committerName,
      GIT_COMMITTER_EMAIL: committerEmail,
      ...ROOT_DATE,
    },
  );
  const remoteTip = git(cwd, ["ls-remote", "--refs", "origin", "refs/heads/main"])
    .split("\t", 1)[0]
    .toLowerCase();
  const expected = expectedSha.toLowerCase();
  if (remoteTip === normalizedSha) {
    return { status: "already-normalized", sha: normalizedSha, tree };
  }
  if (remoteTip !== expected) {
    return { status: "stale", sha: normalizedSha, tree, remoteTip: remoteTip || null };
  }

  git(cwd, [
    "push",
    `--force-with-lease=refs/heads/main:${expected}`,
    "origin",
    `${normalizedSha}:refs/heads/main`,
  ]);
  return { status: "updated", sha: normalizedSha, tree };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const result = normalizeHistory({ expectedSha: process.env.GITHUB_SHA });
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
