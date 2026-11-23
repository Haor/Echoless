const RECOVERY_DELAYS_MS = [750, 1_500, 3_000] as const;
const RECOVERY_WINDOW_MS = 30_000;

export type StreamRecoveryState = {
  attempts: number;
  windowStartedAtMs: number | null;
  recovering: boolean;
};

export const INITIAL_STREAM_RECOVERY: StreamRecoveryState = {
  attempts: 0,
  windowStartedAtMs: null,
  recovering: false,
};

export function nextStreamRecovery(
  state: StreamRecoveryState,
  nowMs: number,
): { state: StreamRecoveryState; delayMs: number | null } {
  const inCurrentWindow =
    state.windowStartedAtMs != null &&
    nowMs - state.windowStartedAtMs <= RECOVERY_WINDOW_MS;
  const attempts = inCurrentWindow ? state.attempts : 0;
  const windowStartedAtMs = inCurrentWindow ? state.windowStartedAtMs : nowMs;
  const delayMs = RECOVERY_DELAYS_MS[attempts] ?? null;

  return {
    state: {
      attempts: delayMs == null ? attempts : attempts + 1,
      windowStartedAtMs,
      recovering: delayMs != null,
    },
    delayMs,
  };
}
