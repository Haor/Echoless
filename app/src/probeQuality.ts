import type { NearDelayProbeResult } from "./api";
import type { Platform } from "./types";

const PROBE_INIT_DELAY_MS = 8;

export interface ProbeAutofill {
  nearDelayMs: number | null;
  initialDelayMs: number | null;
}

export function probeAutofill(
  result: NearDelayProbeResult,
  platform: Platform,
  kind: string,
): ProbeAutofill {
  if (result.quality !== "valid") {
    return { nearDelayMs: null, initialDelayMs: null };
  }

  const nearDelayMs =
    platform === "macos" ? result.recommended_near_delay_ms : null;
  if (kind !== "aec3") return { nearDelayMs, initialDelayMs: null };

  if (platform === "macos") {
    return {
      nearDelayMs,
      initialDelayMs: nearDelayMs == null ? null : PROBE_INIT_DELAY_MS,
    };
  }

  const measured = result.event_lag_mean_ms;
  const initialDelayMs = measured == null ? null : Math.round(measured);
  return {
    nearDelayMs,
    initialDelayMs:
      initialDelayMs != null && initialDelayMs >= 1 ? initialDelayMs : null,
  };
}
