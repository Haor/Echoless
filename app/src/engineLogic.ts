import type { PipelineCfg } from "./api";
import type { NoiseMode, NoiseSuppressionManifest } from "./types";

export function modelFileName(model: unknown): string | null {
  if (typeof model !== "string" || model.length === 0) return null;
  const parts = model.split(/[\\/]/);
  return parts[parts.length - 1] ?? null;
}

export function allowedNoiseModes(
  manifest: NoiseSuppressionManifest | null,
  kind: string,
  params: Record<string, unknown>,
): NoiseMode[] {
  if (!manifest) return ["off"];
  if (kind !== "localvqe") return manifest.engine_defaults[kind] ?? ["off"];

  const file = modelFileName(params.model);
  const model = manifest.localvqe_models.find((entry) => entry.file === file);
  return model?.allowed_modes ?? manifest.unknown_localvqe_allowed_modes;
}

export function normalizeNoiseMode(
  manifest: NoiseSuppressionManifest | null,
  kind: string,
  params: Record<string, unknown>,
  mode: NoiseMode,
): NoiseMode {
  return allowedNoiseModes(manifest, kind, params).includes(mode) ? mode : "off";
}

export function patchNoiseModeParam(
  current: Partial<Record<NoiseMode, Record<string, unknown>>>,
  mode: NoiseMode,
  key: string,
  value: unknown,
): Partial<Record<NoiseMode, Record<string, unknown>>> | null {
  if (mode === "off" || Object.is(current[mode]?.[key], value)) return null;
  return {
    ...current,
    [mode]: {
      ...(current[mode] ?? {}),
      [key]: value,
    },
  };
}

export function canChangePipeline(
  kind: string,
  patch: Partial<PipelineCfg>,
): boolean {
  if (kind !== "nvidia_afx_aec") return true;
  return !(
    "sample_rate" in patch ||
    "frame_ms" in patch ||
    "reference_channels" in patch
  );
}

export function pipelineForEngineKind(
  kind: string,
  pipeline: PipelineCfg,
): PipelineCfg {
  if (kind !== "nvidia_afx_aec") return pipeline;
  if (
    pipeline.sample_rate === 48_000 &&
    pipeline.frame_ms === 10 &&
    pipeline.reference_channels === "mono"
  ) {
    return pipeline;
  }
  return {
    ...pipeline,
    sample_rate: 48_000,
    frame_ms: 10,
    reference_channels: "mono",
  };
}

export function hotInitialDelayValue(value: unknown): number | null {
  if (value == null || value === "") return 0;
  const delayMs = Number(value);
  if (!Number.isFinite(delayMs)) return null;
  return Math.round(delayMs);
}

export function hotLocalvqeNoiseGateValue(next: Record<string, unknown>): {
  enabled: boolean;
  thresholdDbfs: number;
} | null {
  const threshold =
    next.noise_gate_threshold_dbfs == null ||
    next.noise_gate_threshold_dbfs === ""
      ? -45
      : Number(next.noise_gate_threshold_dbfs);
  if (!Number.isFinite(threshold)) return null;
  return { enabled: Boolean(next.noise_gate), thresholdDbfs: threshold };
}

export interface RunIntentGuard {
  /** Generation captured by an async restart transaction. */
  snapshot(): number;
  /** Replace the desired terminal state and invalidate older async work. */
  request(on: boolean): number;
  wantsRun(): boolean;
  /** Only the current generation may start a sidecar while ON is still desired. */
  allowsStart(generation: number): boolean;
}

export function createRunIntentGuard(initiallyOn: boolean): RunIntentGuard {
  let generation = 0;
  let desiredOn = initiallyOn;
  return {
    snapshot: () => generation,
    request(on) {
      generation += 1;
      desiredOn = on;
      return generation;
    },
    wantsRun: () => desiredOn,
    allowsStart: (captured) => desiredOn && captured === generation,
  };
}

export interface SerialQueue<T> {
  enqueue(delta: Partial<T>): void;
  settled(): Promise<void>;
}

/**
 * Serializes structural restart requests (device/reference/engine changes) so
 * overlapping stop->start cycles coalesce into a single final restart.
 */
export function createSerialQueue<T>(
  run: (merged: Partial<T>) => Promise<void>,
): SerialQueue<T> {
  let chain: Promise<void> = Promise.resolve();
  let queued: Partial<T> | null = null;
  return {
    enqueue(delta) {
      if (queued) {
        queued = { ...queued, ...delta };
        return;
      }
      queued = delta;
      chain = chain.then(async () => {
        const merged = queued;
        queued = null;
        if (merged) await run(merged);
      });
    },
    settled() {
      return chain;
    },
  };
}
