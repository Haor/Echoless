import { describe, expect, it, vi } from "vitest";
import { canChangePipeline, pipelineForEngineKind } from "./engineLogic";

const FIXED_PIPELINE_PATCHES = [
  { sample_rate: 16_000 },
  { frame_ms: 20 },
  { reference_channels: "stereo" as const },
];

describe("NVAFX Advanced pipeline lock", () => {
  it.each(FIXED_PIPELINE_PATCHES)(
    "blocks NVAFX fixed pipeline patch %o before side effects",
    (patch) => {
      const effect = vi.fn();

      if (canChangePipeline("nvidia_afx_aec", patch)) effect();

      expect(effect).not.toHaveBeenCalled();
    },
  );

  it.each(["aec3", "localvqe"])(
    "keeps all pipeline options editable for %s",
    (kind) => {
      for (const patch of FIXED_PIPELINE_PATCHES) {
        expect(canChangePipeline(kind, patch)).toBe(true);
      }
    },
  );

  it("keeps NVAFX near-delay tuning outside the three-option lock", () => {
    expect(canChangePipeline("nvidia_afx_aec", { near_delay_ms: 25 })).toBe(
      true,
    );
  });

  it("normalizes a non-default pipeline when entering NVAFX", () => {
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
  });

  it.each(["aec3", "localvqe"])(
    "does not normalize the pipeline when entering %s",
    (kind) => {
      const pipeline = {
        sample_rate: 16_000,
        frame_ms: 20,
        reference_channels: "stereo" as const,
      };

      expect(pipelineForEngineKind(kind, pipeline)).toBe(pipeline);
    },
  );
});
