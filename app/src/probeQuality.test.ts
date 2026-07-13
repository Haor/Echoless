import { describe, expect, it } from "vitest";
import type { NearDelayProbeResult } from "./api";
import { probeAutofill } from "./probeQuality";

function result(
  patch: Partial<NearDelayProbeResult> = {},
): NearDelayProbeResult {
  return {
    session_dir: "/tmp/probe",
    session_retained: false,
    ref_dbfs: -6,
    mic_dbfs: -12,
    global_lag_ms: -20,
    global_corr: 0.9,
    quality: "valid",
    quality_reasons: [],
    event_count: 12,
    event_detected: 12,
    event_lag_mean_ms: -20,
    event_lag_stddev_ms: 0,
    event_lag_drift_ms: 0,
    recommended_near_delay_ms: 30,
    per_beep_lags: [],
    warnings: [],
    ...patch,
  };
}

describe("authoritative delay probe quality", () => {
  it.each(["invalid", "uncertain"] as const)(
    "never fills delay fields for %s measurements",
    (quality) => {
      const measured = result({
        quality,
        quality_reasons: ["insufficient_valid_lags"],
      });

      expect(probeAutofill(measured, "macos", "aec3")).toEqual({
        nearDelayMs: null,
        initialDelayMs: null,
      });
      expect(probeAutofill(measured, "windows", "aec3")).toEqual({
        nearDelayMs: null,
        initialDelayMs: null,
      });
    },
  );

  it("fills macOS only when the valid recommendation is present", () => {
    expect(probeAutofill(result(), "macos", "aec3")).toEqual({
      nearDelayMs: 30,
      initialDelayMs: 8,
    });
    expect(
      probeAutofill(
        result({ recommended_near_delay_ms: null }),
        "macos",
        "aec3",
      ),
    ).toEqual({ nearDelayMs: null, initialDelayMs: null });
  });

  it("fills Windows AEC3 init from a valid event measurement only", () => {
    expect(
      probeAutofill(
        result({ event_lag_mean_ms: 18.6 }),
        "windows",
        "aec3",
      ),
    ).toEqual({ nearDelayMs: null, initialDelayMs: 19 });
    expect(
      probeAutofill(
        result({ event_lag_mean_ms: null }),
        "windows",
        "aec3",
      ),
    ).toEqual({ nearDelayMs: null, initialDelayMs: null });
  });

  it("does not re-derive quality from diagnostic warnings", () => {
    expect(
      probeAutofill(
        result({ warnings: ["diagnostic only"] }),
        "macos",
        "aec3",
      ),
    ).toEqual({ nearDelayMs: 30, initialDelayMs: 8 });
  });

  it("does not write an AEC3-only init hint for other engines", () => {
    expect(probeAutofill(result(), "macos", "localvqe")).toEqual({
      nearDelayMs: 30,
      initialDelayMs: null,
    });
  });

});
