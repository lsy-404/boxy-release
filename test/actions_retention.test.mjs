import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { retainLatestWorkflowRuns } from "../scripts/actions-retention.mjs";

const producerNames = ["Boxy Release Build", "Boxy verification", "Normalize main history"];
const cleanupName = "Actions history retention";

function makeRun(id, workflowId, runNumber, overrides = {}) {
  return { id, workflow_id: workflowId, run_number: runNumber, run_attempt: 1, status: "completed", created_at: `2026-10-${String(runNumber).padStart(2, "0")}T00:00:00Z`, ...overrides };
}

function harness(runPages, overrides = {}) {
  const calls = [];
  const workflows = [
    { id: 11, name: producerNames[0] },
    { id: 12, name: producerNames[1] },
    { id: 13, name: producerNames[2] },
    { id: 14, name: cleanupName },
    { id: 15, name: "Other workflow" },
  ];
  const api = {
    listWorkflows: async () => { calls.push(["workflows"]); return workflows; },
    listWorkflowRuns: async (id) => { calls.push(["runs", id]); return runPages[id] ?? []; },
    deleteWorkflowRun: async (id) => { calls.push(["delete", id]); await overrides.deleteWorkflowRun?.(id); },
  };
  return { api, calls };
}

function runRetention(api, cleanupRunId = 140) {
  return retainLatestWorkflowRuns({ api, producerWorkflowNames: producerNames, cleanupWorkflowName: cleanupName, cleanupRunId });
}

test("keeps the newest completed run per workflow and retains the current cleanup run", async () => {
  const { api, calls } = harness({
    11: [makeRun(112, 11, 2), makeRun(111, 11, 1)],
    12: [makeRun(121, 12, 1)],
    13: [],
    14: [makeRun(141, 14, 3), makeRun(140, 14, 2), makeRun(139, 14, 1)],
    15: [makeRun(151, 15, 1), makeRun(150, 15, 0)],
  });

  const result = await runRetention(api);

  assert.deepEqual(result, { skipped: null, activeRuns: [], deletedRunIds: [111, 139, 150] });
  assert.deepEqual(calls.slice(0, 6), [["workflows"], ["runs", 11], ["runs", 12], ["runs", 13], ["runs", 14], ["runs", 15]]);
  assert.deepEqual(calls.slice(6), [["delete", 111], ["delete", 139], ["delete", 150]]);
});

test("skips the entire pass if any producer run is not completed", async () => {
  const { api, calls } = harness({
    11: [makeRun(111, 11, 1)],
    12: [makeRun(121, 12, 1, { status: "waiting" })],
    13: [makeRun(131, 13, 1)],
    14: [makeRun(141, 14, 2), makeRun(140, 14, 1)],
    15: [makeRun(151, 15, 2), makeRun(150, 15, 1)],
  });

  const result = await runRetention(api);

  assert.deepEqual(result.skipped, "producer_workflows_active");
  assert.deepEqual(result.activeRuns, [{ workflow: producerNames[1], runId: 121, status: "waiting" }]);
  assert.ok(calls.every(([kind]) => kind !== "delete"));
});

test("collects every workflow inventory before deletion", async () => {
  const { api, calls } = harness({
    11: [makeRun(111, 11, 1), makeRun(112, 11, 2)],
    12: [makeRun(121, 12, 1)],
    13: [makeRun(131, 13, 1)],
    14: [],
    15: [],
  });

  await runRetention(api);

  const firstDelete = calls.findIndex(([kind]) => kind === "delete");
  assert.equal(firstDelete, 6);
  assert.deepEqual(calls.slice(1, firstDelete), [["runs", 11], ["runs", 12], ["runs", 13], ["runs", 14], ["runs", 15]]);
});

test("a run created after inventory cannot be deleted by this pass", async () => {
  const pages = {
    11: [makeRun(111, 11, 1), makeRun(112, 11, 2)],
    12: [makeRun(121, 12, 1)],
    13: [makeRun(131, 13, 1)],
    14: [],
    15: [],
  };
  const { api, calls } = harness(pages);
  api.deleteWorkflowRun = async (id) => {
    calls.push(["delete", id]);
    if (id === 111) pages[11].push(makeRun(113, 11, 3));
  };

  await runRetention(api);

  assert.deepEqual(calls.filter(([kind]) => kind === "delete"), [["delete", 111]]);
  assert.ok(pages[11].some(({ id }) => id === 113));
  assert.ok(!calls.some(([, id]) => id === 113));
});

test("fails closed when a workflow inventory is unavailable", async () => {
  const { api, calls } = harness({});
  api.listWorkflowRuns = async (id) => {
    if (id === 13) throw new Error("inventory unavailable");
    return [];
  };

  await assert.rejects(runRetention(api), /inventory unavailable/);
  assert.ok(calls.every(([kind]) => kind !== "delete"));
});

test("a queued cleanup run does not block retention", async () => {
  const { api } = harness({
    11: [makeRun(111, 11, 1)],
    12: [makeRun(121, 12, 1)],
    13: [makeRun(131, 13, 1)],
    14: [makeRun(141, 14, 2, { status: "queued" }), makeRun(140, 14, 1)],
    15: [],
  });

  const result = await runRetention(api, 141);

  assert.equal(result.skipped, null);
  assert.deepEqual(result.deletedRunIds, [140]);
});

test("workflow serializes without cancelling and only listens to producer completions", () => {
  const workflow = readFileSync(new URL("../.github/workflows/cleanup-actions.yml", import.meta.url), "utf8");

  assert.match(workflow, /workflow_run:\s*\n\s+workflows:[\s\S]*?- Boxy Release Build[\s\S]*?- Boxy verification[\s\S]*?- Normalize main history\s*\n\s+types: \[completed\]/);
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /actions: write/);
  assert.match(workflow, /contents: read/);
  assert.match(workflow, /group: actions-history-retention\s*\n\s+queue: max/);
  assert.doesNotMatch(workflow, /cancel-in-progress:\s*true/);
  assert.match(workflow, /ref: \$\{\{ github\.event\.repository\.default_branch \}\}/);
  assert.match(workflow, /actions\/github-script@v8/);
  assert.match(workflow, /github\.paginate\(github\.rest\.actions\.listRepoWorkflows/);
  assert.match(workflow, /github\.paginate\(github\.rest\.actions\.listWorkflowRuns/);
  const workflowRunTrigger = workflow.split("  workflow_run:")[1].split("  workflow_dispatch:")[0];
  assert.doesNotMatch(workflowRunTrigger, /Actions history retention/);
});
