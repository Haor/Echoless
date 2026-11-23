import { describe, expect, it } from "vitest";
import {
  shouldAttemptAutoStart,
  shouldRevealAutostartFailure,
  shouldRevealWindow,
  startupDataReady,
} from "./startupMode";

describe("startup launch policy", () => {
  it("shows manual launches only after boot and keeps autostart hidden", () => {
    expect(shouldRevealWindow(false, "manual")).toBe(false);
    expect(shouldRevealWindow(true, "unknown")).toBe(false);
    expect(shouldRevealWindow(true, "manual")).toBe(true);
    expect(shouldRevealWindow(true, "autostart")).toBe(false);
  });

  it("attempts autostart once after cleanup and configuration are ready", () => {
    const ready = {
      mode: "autostart" as const,
      dataReady: true,
      cleanupReady: true,
      attempted: false,
      running: false,
    };

    expect(shouldAttemptAutoStart(ready)).toBe(true);
    expect(shouldAttemptAutoStart({ ...ready, mode: "manual" })).toBe(false);
    expect(shouldAttemptAutoStart({ ...ready, dataReady: false })).toBe(false);
    expect(shouldAttemptAutoStart({ ...ready, cleanupReady: false })).toBe(false);
    expect(shouldAttemptAutoStart({ ...ready, attempted: true })).toBe(false);
    expect(shouldAttemptAutoStart({ ...ready, running: true })).toBe(false);
  });

  it("waits for NVAFX doctor only when NVAFX is the selected engine", () => {
    expect(startupDataReady(true, "aec3", false)).toBe(true);
    expect(startupDataReady(true, "localvqe", false)).toBe(true);
    expect(startupDataReady(true, "nvidia_afx_aec", false)).toBe(false);
    expect(startupDataReady(true, "nvidia_afx_aec", true)).toBe(true);
    expect(startupDataReady(false, "aec3", true)).toBe(false);
  });

  it("reveals the window immediately when autostart fails before or after started", () => {
    expect(
      shouldRevealAutostartFailure({
        mode: "autostart",
        attempted: true,
        running: false,
        err: "WASAPI device open failed",
      }),
    ).toBe(true);
    expect(
      shouldRevealAutostartFailure({
        mode: "autostart",
        attempted: true,
        running: true,
        err: "transient warning",
      }),
    ).toBe(false);
    expect(
      shouldRevealAutostartFailure({
        mode: "autostart",
        attempted: false,
        running: false,
        err: "startup error",
      }),
    ).toBe(false);
    expect(
      shouldRevealAutostartFailure({
        mode: "manual",
        attempted: true,
        running: false,
        err: "error",
      }),
    ).toBe(false);
  });
});
