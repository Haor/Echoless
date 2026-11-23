export type RunGeneration = {
  activeRunId: number | null;
  latestRunId: number;
};

export const INITIAL_RUN_GENERATION: RunGeneration = {
  activeRunId: null,
  latestRunId: 0,
};

export function observeRunStart(
  gen: RunGeneration,
  runId: number,
): RunGeneration {
  if (!Number.isSafeInteger(runId) || runId <= 0 || runId <= gen.latestRunId) {
    return gen;
  }
  return { activeRunId: runId, latestRunId: runId };
}

export function acceptRunEvent(
  gen: RunGeneration,
  event: { run_id: number; type: string },
): { generation: RunGeneration; accepted: boolean } {
  if (!Number.isSafeInteger(event.run_id) || event.run_id <= 0) {
    return { generation: gen, accepted: false };
  }
  if (event.type === "started" && event.run_id > gen.latestRunId) {
    return { generation: observeRunStart(gen, event.run_id), accepted: true };
  }
  return { generation: gen, accepted: event.run_id === gen.activeRunId };
}

export function acceptRunExit(
  gen: RunGeneration,
  event: { run_id: number },
): { generation: RunGeneration; accepted: boolean } {
  if (event.run_id !== gen.activeRunId) {
    return { generation: gen, accepted: false };
  }
  return { generation: { ...gen, activeRunId: null }, accepted: true };
}
