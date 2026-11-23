import { describe, it, expect } from "vitest";
import {
  allowedNoiseModes,
  canChangePipeline,
  createRunIntentGuard,
  createSerialQueue,
  hotInitialDelayValue,
  hotLocalvqeNoiseGateValue,
  modelFileName,
  normalizeNoiseMode,
  patchNoiseModeParam,
  pipelineForEngineKind,
} from "./engineLogic";
import { normalizeFieldCommit } from "./components/Controls";
import type { NoiseSuppressionManifest } from "./types";

const NOISE_MANIFEST: NoiseSuppressionManifest = {
  modes: [
    { id: "webrtc", processor_kind: "webrtc_ns" },
    { id: "rnnoise", processor_kind: "rnnoise" },
    { id: "off", processor_kind: null },
  ],
  engine_defaults: {
    aec3: ["webrtc", "rnnoise", "off"],
    nvidia_afx_aec: ["webrtc", "rnnoise", "off"],
  },
  localvqe_models: [
    {
      file: "localvqe-v1.2-1.3M-f32.gguf",
      version: "v1.2",
      capability: "built_in_ns",
      allowed_modes: ["off"],
    },
    {
      file: "localvqe-v1.3-4.8M-f32.gguf",
      version: "v1.3",
      capability: "built_in_ns",
      allowed_modes: ["off"],
    },
    {
      file: "localvqe-v1.4-aec-200K-f32.gguf",
      version: "v1.4",
      capability: "pure_aec",
      allowed_modes: ["webrtc", "rnnoise", "off"],
    },
  ],
  unknown_localvqe_allowed_modes: ["off"],
};

describe("shared noise suppression compatibility", () => {
  it("uses the manifest for all supported engines and LocalVQE models", () => {
    expect(allowedNoiseModes(NOISE_MANIFEST, "aec3", {})).toEqual([
      "webrtc",
      "rnnoise",
      "off",
    ]);
    expect(
      allowedNoiseModes(NOISE_MANIFEST, "localvqe", {
        model: "C:\\models\\localvqe-v1.3-4.8M-f32.gguf",
      }),
    ).toEqual(["off"]);
    expect(
      allowedNoiseModes(NOISE_MANIFEST, "localvqe", {
        model: "/models/localvqe-v1.4-aec-200K-f32.gguf",
      }),
    ).toEqual(["webrtc", "rnnoise", "off"]);
  });

  it("forces built-in and unknown LocalVQE models to OFF", () => {
    expect(
      normalizeNoiseMode(
        NOISE_MANIFEST,
        "localvqe",
        { model: "localvqe-v1.2-1.3M-f32.gguf" },
        "rnnoise",
      ),
    ).toBe("off");
    expect(
      normalizeNoiseMode(
        NOISE_MANIFEST,
        "localvqe",
        { model: "custom.gguf" },
        "webrtc",
      ),
    ).toBe("off");
  });

  it("extracts model filenames across Windows and POSIX paths", () => {
    expect(modelFileName("C:\\models\\model.gguf")).toBe("model.gguf");
    expect(modelFileName("/models/model.gguf")).toBe("model.gguf");
    expect(modelFileName(undefined)).toBeNull();
  });

  it("persists WebRTC strength independently and ignores repeat selections", () => {
    const current = {
      webrtc: { level: "low" },
      rnnoise: {},
    } satisfies Partial<Record<"webrtc" | "rnnoise", Record<string, unknown>>>;
    const next = patchNoiseModeParam(current, "webrtc", "level", "high");

    expect(next).toEqual({
      webrtc: { level: "high" },
      rnnoise: {},
    });
    expect(patchNoiseModeParam(next!, "webrtc", "level", "high")).toBeNull();
    expect(patchNoiseModeParam(next!, "off", "level", "low")).toBeNull();
  });
});

describe("NVAFX pipeline constraints", () => {
  it("blocks NVAFX fixed pipeline changes while keeping near_delay_ms editable", () => {
    expect(canChangePipeline("nvidia_afx_aec", { sample_rate: 16_000 })).toBe(false);
    expect(canChangePipeline("nvidia_afx_aec", { frame_ms: 20 })).toBe(false);
    expect(
      canChangePipeline("nvidia_afx_aec", { reference_channels: "stereo" }),
    ).toBe(false);
    expect(canChangePipeline("nvidia_afx_aec", { near_delay_ms: 25 })).toBe(true);
    expect(canChangePipeline("aec3", { sample_rate: 16_000 })).toBe(true);
    expect(canChangePipeline("localvqe", { frame_ms: 20 })).toBe(true);
  });

  it("normalizes non-default pipeline values only when entering NVAFX", () => {
    expect(
      pipelineForEngineKind("nvidia_afx_aec", {
        sample_rate: 16_000,
        frame_ms: 20,
        reference_channels: "stereo",
        near_delay_ms: 31,
        output_level: 72,
      }),
    ).toEqual({
      sample_rate: 48_000,
      frame_ms: 10,
      reference_channels: "mono",
      near_delay_ms: 31,
      output_level: 72,
    });

    const custom = {
      sample_rate: 16_000,
      frame_ms: 20,
      reference_channels: "stereo" as const,
    };
    expect(pipelineForEngineKind("aec3", custom)).toBe(custom);
    expect(pipelineForEngineKind("localvqe", custom)).toBe(custom);
  });
});

describe("hot control value normalization", () => {
  it("normalizes initial_delay_ms (empty -> 0, finite -> rounded, invalid -> null)", () => {
    expect(hotInitialDelayValue("")).toBe(0);
    expect(hotInitialDelayValue(null)).toBe(0);
    expect(hotInitialDelayValue(undefined)).toBe(0);
    expect(hotInitialDelayValue("12.4")).toBe(12);
    expect(hotInitialDelayValue(12.6)).toBe(13);
    expect(hotInitialDelayValue("abc")).toBeNull();
    expect(hotInitialDelayValue(NaN)).toBeNull();
    expect(hotInitialDelayValue(Infinity)).toBeNull();
  });

  it("normalizes LocalVQE noise gate values", () => {
    expect(hotLocalvqeNoiseGateValue({ noise_gate: true })).toEqual({
      enabled: true,
      thresholdDbfs: -45,
    });
    expect(
      hotLocalvqeNoiseGateValue({
        noise_gate: 1,
        noise_gate_threshold_dbfs: "-60",
      }),
    ).toEqual({ enabled: true, thresholdDbfs: -60 });
    expect(
      hotLocalvqeNoiseGateValue({ noise_gate_threshold_dbfs: "nope" }),
    ).toBeNull();
  });
});

describe("Field commit normalization", () => {
  it("clears invalid text and clamps out-of-range input without firing redundant commits", () => {
    expect(
      normalizeFieldCommit({
        text: "abc",
        currentValue: null,
        numeric: true,
        min: 0,
        max: 500,
        integer: true,
      }),
    ).toEqual({ value: null, text: "", changed: false });

    expect(
      normalizeFieldCommit({
        text: "999",
        currentValue: 500,
        numeric: true,
        min: 0,
        max: 500,
        integer: true,
      }),
    ).toEqual({ value: 500, text: "500", changed: false });

    expect(
      normalizeFieldCommit({
        text: " 52 ",
        currentValue: 52,
        numeric: true,
        min: 10,
      }),
    ).toEqual({ value: 52, text: "52", changed: false });

    expect(
      normalizeFieldCommit({
        text: "60.4",
        currentValue: 52,
        numeric: true,
        min: 10,
        integer: true,
      }),
    ).toEqual({ value: 60, text: "60", changed: true });
  });
});

describe("run intent guard", () => {
  it("invalidates an in-flight restart as soon as stop becomes the terminal intent", () => {
    const guard = createRunIntentGuard(true);
    const applyIntent = guard.snapshot();

    expect(guard.allowsStart(applyIntent)).toBe(true);
    guard.request(false);

    expect(guard.wantsRun()).toBe(false);
    expect(guard.allowsStart(applyIntent)).toBe(false);
  });

  it("requires a fresh generation before a later explicit start", () => {
    const guard = createRunIntentGuard(true);
    const staleApply = guard.snapshot();
    guard.request(false);
    const explicitStart = guard.request(true);

    expect(guard.allowsStart(staleApply)).toBe(false);
    expect(guard.allowsStart(explicitStart)).toBe(true);
  });

  it("lets a newer configuration restart invalidate an older one", () => {
    const guard = createRunIntentGuard(true);
    const firstRestart = guard.request(true);
    const latestRestart = guard.request(true);

    expect(guard.allowsStart(firstRestart)).toBe(false);
    expect(guard.allowsStart(latestRestart)).toBe(true);
    expect(guard.wantsRun()).toBe(true);
  });
});

describe("createSerialQueue", () => {
  it("serializes overlapping enqueues and coalesces queued deltas", async () => {
    const events: string[] = [];
    let active = 0;
    const q = createSerialQueue<{ v: number }>(async (m) => {
      active += 1;
      expect(active).toBe(1);
      events.push(`start:${m.v}`);
      await new Promise((r) => setTimeout(r, 5));
      events.push(`end:${m.v}`);
      active -= 1;
    });

    q.enqueue({ v: 1 });
    await Promise.resolve();
    q.enqueue({ v: 2 });
    q.enqueue({ v: 3 });
    await q.settled();
    await q.settled();

    expect(events).toEqual(["start:1", "end:1", "start:3", "end:3"]);
  });

  it("coalesces synchronous bursts into a single final execution", async () => {
    const runs: number[] = [];
    const q = createSerialQueue<{ v: number }>(async (m) => {
      runs.push(m.v!);
    });
    q.enqueue({ v: 1 });
    q.enqueue({ v: 2 });
    q.enqueue({ v: 3 });
    await q.settled();
    expect(runs).toEqual([3]);
  });

  it("merges distinct fields while queued with latest-field-wins", async () => {
    const runs: Array<Record<string, unknown>> = [];
    const q = createSerialQueue<{ a?: number; b?: number }>(async (m) => {
      runs.push(m);
      await new Promise((r) => setTimeout(r, 1));
    });
    q.enqueue({ a: 1 });
    await Promise.resolve();
    q.enqueue({ a: 2 });
    q.enqueue({ b: 9 });
    await q.settled();
    await q.settled();
    expect(runs).toEqual([{ a: 1 }, { a: 2, b: 9 }]);
  });
});
