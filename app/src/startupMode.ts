export type StartupMode = "unknown" | "manual" | "autostart";

export function shouldRevealWindow(
  booted: boolean,
  mode: StartupMode,
): boolean {
  return booted && mode === "manual";
}

export function shouldRevealAutostartFailure(input: {
  mode: StartupMode;
  attempted: boolean;
  running: boolean;
  err: string | null;
}): boolean {
  return (
    input.mode === "autostart" &&
    input.attempted &&
    !input.running &&
    Boolean(input.err)
  );
}

export function shouldAttemptAutoStart(input: {
  mode: StartupMode;
  dataReady: boolean;
  cleanupReady: boolean;
  attempted: boolean;
  running: boolean;
}): boolean {
  return (
    input.mode === "autostart" &&
    input.dataReady &&
    input.cleanupReady &&
    !input.attempted &&
    !input.running
  );
}

export function startupDataReady(
  coreReady: boolean,
  engineKind: string,
  nvafxChecked: boolean,
): boolean {
  return coreReady && (engineKind !== "nvidia_afx_aec" || nvafxChecked);
}
