import { useEffect, useSyncExternalStore } from "react";
import type { RuntimeStatus } from "./types";

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

export interface Health {
  input_drops: number;
  ref_underruns: number;
  output_underruns: number;
  mic_stale_drops: number;
  ref_stale_drops: number;
  stale_drops: number;
  runtime_errors: number;
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
  healthSnapshot = {
    input_drops: s.input_drops ?? 0,
    ref_underruns: s.ref_underruns ?? 0,
    output_underruns: s.output_underruns ?? 0,
    mic_stale_drops: s.mic_stale_drops ?? 0,
    ref_stale_drops: s.ref_stale_drops ?? 0,
    stale_drops: s.stale_drops ?? 0,
    runtime_errors: s.runtime_errors ?? 0,
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

export function useRuntimeHealth(): Health {
  return useSyncExternalStore(subscribe, () => healthSnapshot, () => ZERO_HEALTH);
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
