import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { retainLatestRelease } from "../scripts/release-retention.mjs";

function release(tag, date = "2026-10-22T12:00:00Z", overrides = {}) {
  return { id: tag, tag_name: tag, draft: false, published_at: date, created_at: date, assets: [], ...overrides };
}

function setup(current, releases, tags = releases.map(({ tag_name }) => ({ name: tag_name })), handlers = {}) {
  const calls = [];
  const api = {
    getReleaseByTag: async () => current,
    listReleases: async () => releases,
    listTags: async () => tags,
    deleteRelease: async (id) => { calls.push(["release", id]); await handlers.deleteRelease?.(id); },
    deleteTag: async (tag) => { calls.push(["tag", tag]); await handlers.deleteTag?.(tag); },
  };
  return { api, calls };
}

test("retains semantic newest release and removes older releases and orphan version tags", async () => {
  const current = release("v2.3.0", "2026-10-22T12:00:00Z");
  const releases = [current, release("v2.2.9", "2026-10-25T12:00:00Z"), release("v1.9.9")];
  const { api, calls } = setup(current, releases, [
    { name: "v2.3.0" }, { name: "v2.2.9" }, { name: "v1.9.9" }, { name: "v1.8.0" },
    { name: "v2.4.0-rc.1" }, { name: "release-candidate" }, { name: "v3.0.0" },
  ]);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(result, { deleted: ["v2.2.9", "v1.9.9"], deletedTags: ["v2.2.9", "v1.9.9", "v1.8.0"] });
  assert.deepEqual(calls, [
    ["release", "v2.2.9"], ["release", "v1.9.9"],
    ["tag", "v2.2.9"], ["tag", "v1.9.9"], ["tag", "v1.8.0"],
  ]);
});

test("compares numeric components and prereleases semantically", async () => {
  const current = release("v1.2.0.rc.2", "2026-10-20T00:00:00Z");
  const releases = [current,
    release("v1.2.0-rc.10", "2026-10-19T00:00:00Z"),
    release("v1.2.0-rc.1", "2026-10-25T00:00:00Z"),
    release("v1.1.99", "2026-10-25T00:00:00Z"),
    release("v1.2.0", "2026-10-19T00:00:00Z"),
    release("v1.10.0", "2026-10-19T00:00:00Z"),
  ];
  const { api, calls } = setup(current, releases);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(result.deleted, ["v1.2.0-rc.1", "v1.1.99"]);
  assert.deepEqual(calls.filter(([kind]) => kind === "release").map(([, tag]) => tag), result.deleted);
});

test("an older rerun leaves newer releases and tags untouched", async () => {
  const current = release("v2.2.3", "2026-10-21T12:00:00Z");
  const releases = [release("v2.3.0", "2026-10-20T00:00:00Z"), current, release("v2.2.2")];
  const { api, calls } = setup(current, releases);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(result.deleted, ["v2.2.2"]);
  assert.deepEqual(result.deletedTags, ["v2.2.2"]);
  assert.deepEqual(calls, [["release", "v2.2.2"], ["tag", "v2.2.2"]]);
});

test("deletes an older release with the same publication time and keeps unknown tags", async () => {
  const date = "2026-10-22T12:00:00Z";
  const current = release("v2.3.0", date);
  const { api, calls } = setup(current, [current, release("v2.2.9", date), release("nightly", date)], [
    { name: "v2.3.0" }, { name: "v2.2.9" }, { name: "stable" }, { name: "release-candidate" },
  ]);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(result, { deleted: ["v2.2.9"], deletedTags: ["v2.2.9"] });
  assert.deepEqual(calls, [["release", "v2.2.9"], ["tag", "v2.2.9"]]);
});

test("removes orphaned old version tags when there are no old releases", async () => {
  const current = release("v2.3.0", undefined);
  const { api, calls } = setup(current, [current], [
    { name: "v2.3.0" }, { name: "v2.2.9" }, { name: "v1.0.0" }, { name: "v3.0.0" }, { name: "stable" },
  ]);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(result, { deleted: [], deletedTags: ["v2.2.9", "v1.0.0"] });
  assert.deepEqual(calls, [["tag", "v2.2.9"], ["tag", "v1.0.0"]]);
});

test("deletes older releases before the current release has any build assets", async () => {
  const current = release("v2.3.0", undefined, { assets: [] });
  const { api, calls } = setup(current, [current, release("v2.2.3")]);

  const result = await retainLatestRelease({ tag: current.tag_name, api });

  assert.deepEqual(result.deleted, ["v2.2.3"]);
  assert.deepEqual(calls, [["release", "v2.2.3"], ["tag", "v2.2.3"]]);
});

test("fails closed for an unpublished or mismatched current release", async () => {
  const draft = release("v2.3.0", null, { draft: true });
  const draftSetup = setup(draft, [draft, release("v2.2.3")]);
  await assert.rejects(retainLatestRelease({ tag: draft.tag_name, api: draftSetup.api }), /unpublished/);
  assert.deepEqual(draftSetup.calls, []);

  const mismatch = setup(release("v2.3.1"), [release("v2.3.0")]);
  await assert.rejects(retainLatestRelease({ tag: "v2.3.0", api: mismatch.api }), /does not match/);
  assert.deepEqual(mismatch.calls, []);
});

test("does not delete a release tag if release deletion fails", async () => {
  const current = release("v2.3.0");
  const { api, calls } = setup(current, [current, release("v2.2.3")], undefined, {
    deleteRelease: async () => { throw new Error("release deletion failed"); },
  });

  await assert.rejects(retainLatestRelease({ tag: current.tag_name, api }), /release deletion failed/);
  assert.deepEqual(calls, [["release", "v2.2.3"]]);
});

test("rejects tag deletion failures and accepts an already missing tag", async () => {
  const current = release("v2.3.0");
  const failing = setup(current, [current, release("v2.2.3")], undefined, {
    deleteTag: async () => { throw new Error("tag deletion failed"); },
  });
  await assert.rejects(retainLatestRelease({ tag: current.tag_name, api: failing.api }), /tag deletion failed/);
  assert.deepEqual(failing.calls, [["release", "v2.2.3"], ["tag", "v2.2.3"]]);

  const missing = setup(current, [current, release("v2.2.3")], undefined, {
    deleteTag: async () => { const error = new Error("missing"); error.status = 404; throw error; },
  });
  const result = await retainLatestRelease({ tag: current.tag_name, api: missing.api });
  assert.deepEqual(result.deletedTags, []);
});

test("does not delete anything if release or tag inventory fails", async () => {
  const current = release("v2.3.0");
  const releasesFailure = setup(current, [current, release("v2.2.3")]);
  releasesFailure.api.listReleases = async () => { throw new Error("release inventory failed"); };
  await assert.rejects(retainLatestRelease({ tag: current.tag_name, api: releasesFailure.api }), /release inventory failed/);
  assert.deepEqual(releasesFailure.calls, []);

  const tagsFailure = setup(current, [current, release("v2.2.3")]);
  tagsFailure.api.listTags = async () => { throw new Error("tag inventory failed"); };
  await assert.rejects(retainLatestRelease({ tag: current.tag_name, api: tagsFailure.api }), /tag inventory failed/);
  assert.deepEqual(tagsFailure.calls, []);
});

test("workflow cleans old releases before either platform build and gates both builds on preparation", () => {
  const workflow = readFileSync(new URL("../.github/workflows/manual-build.yml", import.meta.url), "utf8");
  const prepareJob = workflow.split("  prepare-release:")[1].split("  windows-boxy:")[0];
  const windowsJob = workflow.split("  windows-boxy:")[1].split("  macos-boxy:")[0];
  const macosJob = workflow.split("  macos-boxy:")[1];

  assert.ok(prepareJob, "release preparation should be configured");
  const sourceCheck = prepareJob.indexOf("node --test test/release_source.test.mjs");
  const retentionTest = prepareJob.indexOf("node --test test/release_retention.test.mjs");
  const verifyRelease = prepareJob.indexOf("- name: Verify existing release");
  const cleanup = prepareJob.indexOf("- name: Delete older releases and tags before build");
  assert.ok(sourceCheck >= 0 && sourceCheck < verifyRelease, "source boundary must be checked before release verification");
  assert.ok(retentionTest >= 0 && retentionTest < cleanup, "retention tests must pass before cleanup");
  assert.ok(verifyRelease >= 0 && verifyRelease < cleanup, "the current release target must be verified before cleanup");
  assert.match(prepareJob.slice(cleanup), /github\.paginate\(github\.rest\.repos\.listReleases/);
  assert.match(prepareJob.slice(cleanup), /github\.paginate\(github\.rest\.repos\.listTags/);
  assert.match(prepareJob.slice(cleanup), /github\.rest\.git\.deleteRef/);
  assert.match(windowsJob, /needs: prepare-release/);
  assert.match(macosJob, /needs: prepare-release/);
  assert.doesNotMatch(workflow, /retain-latest-release/);
});
