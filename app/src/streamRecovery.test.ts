import { describe, expect, it } from "vitest";
import { INITIAL_STREAM_RECOVERY, nextStreamRecovery } from "./streamRecovery";

describe("stream recovery", () => {
  it("advances through bounded 750ms -> 1500ms -> 3000ms retries even across consecutive reconnect failures", () => {
    let state = INITIAL_STREAM_RECOVERY;
    const delays: Array<number | null> = [];
    const recoveringFlags: boolean[] = [];

    for (let attempt = 0; attempt < 4; attempt += 1) {
      const step = nextStreamRecovery(state, 1_000 + attempt * 100);
      state = step.state;
      delays.push(step.delayMs);
      recoveringFlags.push(state.recovering);
    }

    expect(delays).toEqual([750, 1_500, 3_000, null]);
    expect(recoveringFlags).toEqual([true, true, true, false]);
  });

  it("starts a fresh retry window after the run stayed healthy past the 30s window", () => {
    const exhausted = {
      attempts: 3,
      windowStartedAtMs: 1_000,
      recovering: false,
    };

    const recovered = nextStreamRecovery(exhausted, 31_001);
    expect(recovered.state.attempts).toBe(1);
    expect(recovered.state.recovering).toBe(true);
    expect(recovered.delayMs).toBe(750);
  });
});
