import { describe, expect, it } from "vitest";

import {
  acceptRunEvent,
  acceptRunExit,
  INITIAL_RUN_GENERATION,
  observeRunStart,
} from "./runGeneration";

describe("run generation isolation", () => {
  it("ignores delayed status and exit from the previous run", () => {
    let generation = observeRunStart(INITIAL_RUN_GENERATION, 1);
    generation = observeRunStart(generation, 2);

    const staleStatus = acceptRunEvent(generation, {
      type: "status",
      run_id: 1,
    });
    expect(staleStatus.accepted).toBe(false);
    expect(staleStatus.generation).toEqual(generation);

    const staleExit = acceptRunExit(generation, { run_id: 1 });
    expect(staleExit.accepted).toBe(false);
    expect(staleExit.generation).toEqual(generation);

    expect(
      acceptRunEvent(generation, { type: "status", run_id: 2 }).accepted,
    ).toBe(true);

    const currentExit = acceptRunExit(generation, { run_id: 2 });
    expect(currentExit.accepted).toBe(true);
    expect(currentExit.generation).toEqual({
      activeRunId: null,
      latestRunId: 2,
    });
    expect(acceptRunExit(currentExit.generation, { run_id: 2 }).accepted).toBe(
      false,
    );
    expect(observeRunStart(currentExit.generation, 2)).toEqual(
      currentExit.generation,
    );
  });

  it("lets a newer started event win before invoke resolves", () => {
    const first = observeRunStart(INITIAL_RUN_GENERATION, 1);
    const newer = acceptRunEvent(first, { type: "started", run_id: 2 });

    expect(newer.accepted).toBe(true);
    expect(newer.generation).toEqual({ activeRunId: 2, latestRunId: 2 });
    expect(observeRunStart(newer.generation, 1)).toEqual(newer.generation);
  });

  it("rejects invalid identifiers", () => {
    for (const run_id of [0, -1, Number.NaN, Number.MAX_SAFE_INTEGER + 1]) {
      expect(
        acceptRunEvent(INITIAL_RUN_GENERATION, { type: "started", run_id })
          .accepted,
      ).toBe(false);
    }
  });
});
