import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { retainLatestRelease } from "../scripts/release-retention.mjs";

const assets = [
  { name: "boxy-windows-x64.exe", state: "uploaded", size: 10 },
  { name: "boxy-macos-universal.zip", state: "uploaded", size: 20 },
];

function release(tag, date, overrides = {}) {
  return {
    id: tag,
    tag_name: tag,
    draft: false,
    published_at: date,
    created_at: date,
    assets: [],
    ...overrides,
  };
}

function setup(current, releases) {
  const deleted = [];
  const api = {
    getReleaseByTag: async () => current,
    listReleases: async () => releases,
    deleteRelease: async (id) => deleted.push(id),
  };
  return { api, deleted };
}

test("deletes older releases after retaining the complete current release", async () => {
  const current = release("v2.3.0", "2026-10-22T12:00:00Z", { assets });
  const { api, deleted } = setup(current, [
    current,
    release("v2.2.3", "2026-10-21T12:00:00Z"),
    release("v2.2.2", "2026-10-20T12:00:00Z"),
  ]);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(deleted, ["v2.2.3", "v2.2.2"]);
  assert.deepEqual(result.deleted, ["v2.2.3", "v2.2.2"]);
});

test("older reruns leave newer releases untouched", async () => {
  const current = release("v2.2.3", "2026-10-21T12:00:00Z", { assets });
  const { api, deleted } = setup(current, [
    release("v2.3.0", "2026-10-22T12:00:00Z"),
    current,
    release("v2.2.2", "2026-10-20T12:00:00Z"),
  ]);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(deleted, ["v2.2.2"]);
  assert.deepEqual(result.deleted, ["v2.2.2"]);
});

test("fails closed if either platform asset is missing or incomplete", async () => {
  for (const incompleteAssets of [
    [assets[0]],
    [assets[0], { ...assets[1], size: 0 }],
    [assets[0], { ...assets[1], state: "new" }],
  ]) {
    const current = release("v2.3.0", "2026-10-22T12:00:00Z", { assets: incompleteAssets });
    const { api, deleted } = setup(current, [current, release("v2.2.3", "2026-10-21T12:00:00Z")]);
    await assert.rejects(retainLatestRelease({ tag: current.tag_name, api }), /missing a complete/);
    assert.deepEqual(deleted, []);
  }
});

test("fails closed for a draft or mismatched current release", async () => {
  const current = release("v2.3.0", "2026-10-22T12:00:00Z", { assets, draft: true });
  const { api, deleted } = setup(current, [current, release("v2.2.3", "2026-10-21T12:00:00Z")]);
  await assert.rejects(retainLatestRelease({ tag: current.tag_name, api }), /unpublished/);
  assert.deepEqual(deleted, []);

  const mismatched = setup({ ...current, tag_name: "v2.3.1", draft: false }, [current]);
  await assert.rejects(retainLatestRelease({ tag: "v2.3.0", api: mismatched.api }), /does not match/);
  assert.deepEqual(mismatched.deleted, []);
});

test("keeps releases with the same publication timestamp", async () => {
  const current = release("v2.3.0", "2026-10-22T12:00:00Z", { assets });
  const { api, deleted } = setup(current, [
    current,
    release("v2.2.9", "2026-10-22T12:00:00Z"),
    release("v2.2.8", "2026-10-21T12:00:00Z"),
  ]);

  await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(deleted, ["v2.2.8"]);
});

test("deletes older prereleases and drafts without touching newer releases", async () => {
  const current = release("v2.3.0", "2026-10-22T12:00:00Z", { assets });
  const { api, deleted } = setup(current, [
    current,
    release("v2.4.0-rc.1", "2026-10-23T12:00:00Z", { prerelease: true }),
    release("v2.2.4-rc.1", "2026-10-21T12:00:00Z", { prerelease: true }),
    release("v2.3.1", null, { draft: true, created_at: "2026-10-23T12:00:00Z" }),
    release("v2.2.9", null, { draft: true, created_at: "2026-10-21T12:00:00Z" }),
  ]);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(deleted, ["v2.2.4-rc.1", "v2.2.9"]);
  assert.deepEqual(result.deleted, ["v2.2.4-rc.1", "v2.2.9"]);
});

test("workflow gates retention on both uploads and paginates releases", () => {
  const workflow = readFileSync(new URL("../.github/workflows/manual-build.yml", import.meta.url), "utf8");
  const retentionJob = workflow.split("  retain-latest-release:")[1];

  assert.ok(retentionJob, "retention job should be configured");
  assert.match(retentionJob, /needs: \[windows-boxy, macos-boxy\]/);
  assert.match(retentionJob, /github\.paginate\(github\.rest\.repos\.listReleases/);
  assert.doesNotMatch(retentionJob, /always\(\)/);
});
