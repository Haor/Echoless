export type RunGeneration = {
  activeRunId: number | null;
  latestRunId: number;
};

export const INITIAL_RUN_GENERATION: RunGeneration = {
  activeRunId: null,
  latestRunId: 0,
};

type RunIdentity = {
  run_id: number;
};

type RunEventIdentity = RunIdentity & {
  type: string;
};

export type RunGenerationDecision = {
  generation: RunGeneration;
  accepted: boolean;
};

function validRunId(runId: number): boolean {
  return Number.isSafeInteger(runId) && runId > 0;
}

export function observeRunStart(
  generation: RunGeneration,
  runId: number,
): RunGeneration {
  if (!validRunId(runId) || runId <= generation.latestRunId) {
    return generation;
  }
  return { activeRunId: runId, latestRunId: runId };
}

export function acceptRunEvent(
  generation: RunGeneration,
  event: RunEventIdentity,
): RunGenerationDecision {
  if (!validRunId(event.run_id)) {
    return { generation, accepted: false };
  }

  if (event.type === "started" && event.run_id > generation.latestRunId) {
    const next = observeRunStart(generation, event.run_id);
    return { generation: next, accepted: true };
  }

  return {
    generation,
    accepted: event.run_id === generation.activeRunId,
  };
}

export function acceptRunExit(
  generation: RunGeneration,
  event: RunIdentity,
): RunGenerationDecision {
  if (!validRunId(event.run_id) || event.run_id !== generation.activeRunId) {
    return { generation, accepted: false };
  }
  return {
    generation: { ...generation, activeRunId: null },
    accepted: true,
  };
}
