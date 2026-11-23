import { useEffect, useSyncExternalStore } from "react";
import type { RuntimeStatus } from "./types";

// Telemetry number formatting: null / undefined / NaN / Infinity render as "—".
export const dash = (v: number | null | undefined, d = 1): string =>
  v == null || !Number.isFinite(v) ? "—" : v.toFixed(d);

// Lost audio in pipeline sample frames -> "12 ms". Unknown rate -> raw frames.
export function formatLostAudio(frames: number, sampleRate: number | null): string {
  if (!sampleRate) return String(frames);
  const ms = (frames / sampleRate) * 1000;
  if (frames > 0 && ms < 1) return "<1 ms";
  return `${Math.round(ms)} ms`;
}

// Seconds -> "m:ss" or "h:mm:ss".
export function formatRunClock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

export type HealthVerdict = "idle" | "waiting" | "ok" | "issue";

export function healthVerdict(health: Health, running: boolean): HealthVerdict {
  if (!running) return "idle";
  if (health.elapsed_s == null) return "waiting";
  return health.last_issue_s == null && !health.diverged ? "ok" : "issue";
}

export type RunStatusKind = "live" | "noref" | "warn" | "bypass" | "stopped";

export interface RunStatusInputs {
  powerOn: boolean;
  refSel: string;
  dev: boolean;
  bypassed: boolean;
}

export interface Live {
  mic: number | null;
  ref: number | null;
  out: number | null;
  lat: number | null;
  healthy: boolean;
  seq: number;
}

// Drop/underrun counters are run totals in pipeline sample frames.
export interface Health {
  input_drops: number;
  ref_underruns: number;
  output_underruns: number;
  mic_stale_drops: number;
  ref_stale_drops: number;
  stale_drops: number;
  runtime_errors: number;
  // Backend run clock and pipeline rate from the latest status line;
  // null until the first status line of a run arrives.
  elapsed_s: number | null;
  sample_rate: number | null;
  // Run clock (elapsed_s) of the latest status interval with an audible
  // problem: mic/output dropouts, runtime errors or divergence.
  last_issue_s: number | null;
  diverged: boolean;
  // 时钟漂移:输出时钟相对麦克风时钟的偏差百分比 + 后端告警位(带滞回)。
  clock_skew_pct: number | null;
  clock_skew_warning: boolean;
  session_dir: string | null;
  backend_error: string | null;
  recording: boolean;
  rec_elapsed_s: number;
  rec_drops: number;
}

const ZERO_LIVE: Live = {
  mic: null,
  ref: null,
  out: null,
  lat: null,
  healthy: true,
  seq: 0,
};

const ZERO_HEALTH: Health = {
  input_drops: 0,
  ref_underruns: 0,
  output_underruns: 0,
  mic_stale_drops: 0,
  ref_stale_drops: 0,
  stale_drops: 0,
  runtime_errors: 0,
  elapsed_s: null,
  sample_rate: null,
  last_issue_s: null,
  diverged: false,
  clock_skew_pct: null,
  clock_skew_warning: false,
  session_dir: null,
  backend_error: null,
  recording: false,
  rec_elapsed_s: 0,
  rec_drops: 0,
};

const STATUS_HOLD_MS = 2500;
const REF_SILENT_HOLD_MS = 3000;

let liveSnapshot = ZERO_LIVE;
let healthSnapshot = ZERO_HEALTH;
let statusInputs: RunStatusInputs = {
  powerOn: false,
  refSel: "none",
  dev: false,
  bypassed: false,
};
let lastRefLoudAt = 0;
let shownStatusKind: RunStatusKind = "stopped";
let pendingStatus: {
  kind: RunStatusKind;
  timer: ReturnType<typeof setTimeout>;
} | null = null;

const listeners = new Set<() => void>();
const statusListeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

function emitStatus() {
  for (const listener of statusListeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function subscribeRunStatusKind(listener: () => void) {
  statusListeners.add(listener);
  return () => statusListeners.delete(listener);
}

export function getRunStatusKind(): RunStatusKind {
  return shownStatusKind;
}

function clearPendingStatus() {
  if (!pendingStatus) return;
  clearTimeout(pendingStatus.timer);
  pendingStatus = null;
}

function computeRawStatus(nowMs: number): RunStatusKind {
  if (liveSnapshot.ref !== null && liveSnapshot.ref > -100) {
    lastRefLoudAt = nowMs;
  }
  const refSilent =
    liveSnapshot.ref !== null &&
    liveSnapshot.ref <= -100 &&
    nowMs - lastRefLoudAt > REF_SILENT_HOLD_MS;
  const hasReference =
    statusInputs.refSel !== "none" && (statusInputs.dev || !refSilent);

  return !statusInputs.powerOn
    ? "stopped"
    : statusInputs.bypassed
      ? "bypass"
      : !liveSnapshot.healthy
        ? "warn"
        : !hasReference
          ? "noref"
          : "live";
}

function reconcileRunStatus(nowMs: number) {
  const raw = computeRawStatus(nowMs);
  if (raw === shownStatusKind) {
    clearPendingStatus();
    return;
  }
  if (
    raw === "stopped" ||
    shownStatusKind === "stopped" ||
    raw === "bypass" ||
    shownStatusKind === "bypass" ||
    raw === "live"
  ) {
    clearPendingStatus();
    shownStatusKind = raw;
    emitStatus();
    return;
  }
  if (pendingStatus?.kind === raw) return;
  clearPendingStatus();
  const target = raw;
  const timer = setTimeout(() => {
    pendingStatus = null;
    if (shownStatusKind !== target) {
      shownStatusKind = target;
      emitStatus();
    }
  }, STATUS_HOLD_MS);
  pendingStatus = { kind: target, timer };
}

export function syncRunStatusInputs(
  next: RunStatusInputs,
  nowMs = performance.now(),
): RunStatusKind {
  if (!statusInputs.powerOn && next.powerOn) {
    lastRefLoudAt = nowMs;
  }
  statusInputs = next;
  reconcileRunStatus(nowMs);
  return shownStatusKind;
}

export function resetRuntimeHealth() {
  healthSnapshot = ZERO_HEALTH;
  emit();
}

// 停机/重启时清空实时读数(dBFS / 延迟),否则最后一帧数值残留在界面上。
export function resetRuntimeLive(nowMs = performance.now()) {
  liveSnapshot = ZERO_LIVE;
  emit();
  reconcileRunStatus(nowMs);
}

export function setDiagnosticsSessionDir(sessionDir: string | null) {
  healthSnapshot = { ...healthSnapshot, session_dir: sessionDir };
  emit();
}

// status → Live 的纯映射(可单测的回归 seam)。?? null 兜底:后端某帧缺这些
// 字段时裸取会得到 undefined(非 null),流进 dash 的 toFixed 会抛错卸载整树。
export function statusToLive(s: RuntimeStatus): Live {
  return {
    mic: s.mic_dbfs ?? null,
    ref: s.ref_dbfs ?? null,
    out: s.out_dbfs ?? null,
    lat: s.estimated_user_latency_ms ?? null,
    healthy: !s.diverged && s.runtime_errors === 0 && !s.last_backend_error,
    seq: s.frames,
  };
}

export function publishRuntimeStatus(
  s: RuntimeStatus,
  nowMs = performance.now(),
) {
  liveSnapshot = statusToLive(s);
  // Per-interval counters reset on every status line; the *_total fields are
  // run totals. Older CLIs only send the per-interval values.
  const micStale = s.mic_stale_drops_total ?? s.mic_stale_drops ?? 0;
  const refStale = s.ref_stale_drops_total ?? s.ref_stale_drops ?? 0;
  const runtimeErrors = s.runtime_errors ?? 0;
  const elapsed = Number.isFinite(s.elapsed_s) ? s.elapsed_s : null;
  const audibleIssue =
    (s.input_drops ?? 0) > 0 ||
    (s.mic_stale_drops ?? 0) > 0 ||
    (s.output_underruns ?? 0) > 0 ||
    runtimeErrors > healthSnapshot.runtime_errors ||
    Boolean(s.diverged);
  healthSnapshot = {
    input_drops: s.input_drops_total ?? s.input_drops ?? 0,
    ref_underruns: s.ref_underruns_total ?? s.ref_underruns ?? 0,
    output_underruns: s.output_underruns_total ?? s.output_underruns ?? 0,
    mic_stale_drops: micStale,
    ref_stale_drops: refStale,
    stale_drops: micStale + refStale,
    runtime_errors: runtimeErrors,
    elapsed_s: elapsed,
    sample_rate:
      Number.isFinite(s.sample_rate) && s.sample_rate > 0 ? s.sample_rate : null,
    last_issue_s: audibleIssue ? (elapsed ?? 0) : healthSnapshot.last_issue_s,
    diverged: Boolean(s.diverged),
    clock_skew_pct: Number.isFinite(s.output_skew_pct as number)
      ? (s.output_skew_pct as number)
      : null,
    clock_skew_warning: Boolean(s.clock_skew_warning),
    session_dir: s.diagnostics_session_dir ?? healthSnapshot.session_dir,
    backend_error: s.last_backend_error ?? null,
    recording: Boolean(s.recording),
    rec_elapsed_s: s.diagnostics_elapsed_s ?? 0,
    rec_drops: s.diagnostics_drops ?? 0,
  };
  emit();
  reconcileRunStatus(nowMs);
}

export function useRuntimeLive(): Live {
  return useSyncExternalStore(subscribe, () => liveSnapshot, () => ZERO_LIVE);
}

export function getRuntimeHealth(): Health {
  return healthSnapshot;
}

export function useRuntimeHealth(): Health {
  return useSyncExternalStore(subscribe, getRuntimeHealth, () => ZERO_HEALTH);
}

export function useRunStatusKind(
  powerOn: boolean,
  refSel: string,
  dev: boolean,
  bypassed = false,
): RunStatusKind {
  useEffect(() => {
    syncRunStatusInputs({ powerOn, refSel, dev, bypassed });
  }, [powerOn, refSel, dev, bypassed]);
  return useSyncExternalStore(
    subscribeRunStatusKind,
    getRunStatusKind,
    () => "stopped",
  );
}
