function compareRuns(left, right) {
  const runNumberOrder = Number(right.run_number) - Number(left.run_number);
  if (runNumberOrder !== 0) return runNumberOrder;
  const attemptOrder = Number(right.run_attempt ?? 1) - Number(left.run_attempt ?? 1);
  if (attemptOrder !== 0) return attemptOrder;
  const dateOrder = String(right.created_at ?? "").localeCompare(String(left.created_at ?? ""));
  if (dateOrder !== 0) return dateOrder;
  return BigInt(right.id) > BigInt(left.id) ? 1 : BigInt(right.id) < BigInt(left.id) ? -1 : 0;
}

export async function retainLatestWorkflowRuns({ api, producerWorkflowNames, cleanupWorkflowName, cleanupRunId }) {
  if (!api || !Array.isArray(producerWorkflowNames) || !cleanupWorkflowName || cleanupRunId == null) {
    throw new TypeError("Workflow retention requires an API, producer names, cleanup workflow name, and current run ID.");
  }

  const workflows = await api.listWorkflows();
  const workflowByName = new Map(workflows.map((workflow) => [workflow.name, workflow]));
  const requiredNames = [...new Set([...producerWorkflowNames, cleanupWorkflowName])];
  for (const name of requiredNames) {
    if (!workflowByName.has(name)) throw new Error(`Required workflow was not found: ${name}`);
  }
  const selected = workflows.map((workflow) => {
    if (!workflow.id) throw new Error(`Workflow has no ID: ${workflow.name}`);
    return { ...workflow, isProducer: producerWorkflowNames.includes(workflow.name) };
  });

  const inventories = await Promise.all(selected.map(async (workflow) => ({
    workflow,
    runs: await api.listWorkflowRuns(workflow.id),
  })));

  const busy = inventories.flatMap(({ workflow, runs }) => workflow.isProducer
    ? runs.filter((run) => run.status !== "completed").map((run) => ({ workflow: workflow.name, runId: run.id, status: run.status }))
    : []);
  if (busy.length) return { skipped: "producer_workflows_active", activeRuns: busy, deletedRunIds: [] };

  const deletionCandidates = [];
  for (const { workflow, runs } of inventories) {
    const ordered = [...runs].sort(compareRuns);
    const newestId = ordered[0]?.id;
    for (const run of ordered) {
      if (run.status !== "completed" || run.id === newestId || String(run.id) === String(cleanupRunId)) continue;
      deletionCandidates.push(run.id);
    }
  }

  const deletedRunIds = [];
  for (const runId of deletionCandidates) {
    await api.deleteWorkflowRun(runId);
    deletedRunIds.push(runId);
  }
  return { skipped: null, activeRuns: [], deletedRunIds };
}
