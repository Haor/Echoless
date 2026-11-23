import { describe, it, expect, vi, afterEach } from "vitest";
import {
  dash,
  getRunStatusKind,
  publishRuntimeStatus,
  resetRuntimeLive,
  statusToLive,
  subscribeRunStatusKind,
  syncRunStatusInputs,
} from "./runtimeTelemetry";
import type { RuntimeStatus } from "./types";

// 回归:黑屏根因是 undefined 遥测值流进 .toFixed() 卸载整个 React 树。
// 两道防线各自锁死 —— 源头不产 undefined(statusToLive),消费端挡住任何非有限值(dash)。

describe("dash", () => {
  it("挡住 null / undefined / NaN / Infinity(全部显示 —)", () => {
    expect(dash(null)).toBe("—");
    expect(dash(undefined)).toBe("—");
    expect(dash(NaN)).toBe("—");
    expect(dash(Infinity)).toBe("—");
    expect(dash(-Infinity)).toBe("—");
  });

  it("正常数值走 toFixed", () => {
    expect(dash(-20.456)).toBe("-20.5");
    expect(dash(12, 0)).toBe("12");
    expect(dash(0)).toBe("0.0");
  });

  it("undefined 不再抛错(旧 `=== null` 守卫的黑屏根因)", () => {
    expect(() => dash(undefined)).not.toThrow();
  });
});

describe("statusToLive", () => {
  it("缺字段的 status 落成 null,绝不放 undefined 流出", () => {
    // 后端某帧只发部分字段(其余 key 缺失 → 前端读到 undefined)。
    const partial = { frames: 7 } as unknown as RuntimeStatus;
    const live = statusToLive(partial);
    expect(live.mic).toBeNull();
    expect(live.ref).toBeNull();
    expect(live.out).toBeNull();
    expect(live.lat).toBeNull();
    // 关键不变量:四个数值口全都不是 undefined(否则 dash 会崩)。
    for (const v of [live.mic, live.ref, live.out, live.lat]) {
      expect(v).not.toBe(undefined);
    }
  });

  it("完整 status 透传数值", () => {
    const full = {
      mic_dbfs: -20,
      ref_dbfs: -30,
      out_dbfs: -40,
      estimated_user_latency_ms: 12,
      frames: 5,
      diverged: false,
      runtime_errors: 0,
    } as unknown as RuntimeStatus;
    const live = statusToLive(full);
    expect(live.mic).toBe(-20);
    expect(live.ref).toBe(-30);
    expect(live.out).toBe(-40);
    expect(live.lat).toBe(12);
    expect(live.healthy).toBe(true);
  });

  it("端到端:缺字段 status → statusToLive → dash 不崩", () => {
    const live = statusToLive({ frames: 1 } as unknown as RuntimeStatus);
    expect(() => dash(live.mic)).not.toThrow();
    expect(dash(live.mic)).toBe("—");
  });
});

describe("statusKind subscription isolation (P-02)", () => {
  afterEach(() => {
    vi.useRealTimers();
    resetRuntimeLive(0);
    syncRunStatusInputs(
      { powerOn: false, refSel: "none", dev: false, bypassed: false },
      0,
    );
  });

  function makeStatus(patch: Partial<RuntimeStatus> = {}): RuntimeStatus {
    return {
      type: "status",
      elapsed_s: 1,
      frames: 1,
      sample_rate: 48000,
      frame_ms: 10,
      backend: "aec3",
      mic_dbfs: -20,
      ref_dbfs: -18,
      out_dbfs: -24,
      mic_q_samples: 0,
      ref_q_samples: 0,
      out_q_samples: 0,
      input_queue_latency_ms: 0,
      output_queue_latency_ms: 0,
      algorithmic_latency_ms: 10,
      estimated_user_latency_ms: 14,
      aec_estimated_delay_ms: 0,
      mic_input_drops: 0,
      ref_input_drops: 0,
      input_drops: 0,
      ref_underruns: 0,
      output_underruns: 0,
      output_overruns: 0,
      stale_drops: 0,
      node_process_time_ms: 1,
      runtime_errors: 0,
      diverged: false,
      ...patch,
    };
  }

  it("does not notify statusKind subscribers on steady-state 12.5Hz telemetry packets", () => {
    vi.useFakeTimers();
    syncRunStatusInputs(
      { powerOn: true, refSel: "system", dev: false, bypassed: false },
      1000,
    );
    expect(getRunStatusKind()).toBe("live");

    const listener = vi.fn();
    const unsub = subscribeRunStatusKind(listener);
    try {
      for (let i = 1; i <= 50; i++) {
        publishRuntimeStatus(makeStatus({ frames: i, mic_dbfs: -30 + (i % 5) }), 1000 + i * 80);
      }
      expect(listener).not.toHaveBeenCalled();
      expect(getRunStatusKind()).toBe("live");
    } finally {
      unsub();
    }
  });

  it("debounces degradation to warn/noref and notifies immediately on recovery or bypass", () => {
    vi.useFakeTimers();
    syncRunStatusInputs(
      { powerOn: true, refSel: "system", dev: false, bypassed: false },
      1000,
    );
    const listener = vi.fn();
    const unsub = subscribeRunStatusKind(listener);
    try {
      publishRuntimeStatus(makeStatus({ frames: 1, diverged: true }), 1080);
      expect(getRunStatusKind()).toBe("live");
      expect(listener).toHaveBeenCalledTimes(0);

      vi.advanceTimersByTime(2500);
      expect(getRunStatusKind()).toBe("warn");
      expect(listener).toHaveBeenCalledTimes(1);

      publishRuntimeStatus(makeStatus({ frames: 2, diverged: false }), 3600);
      expect(getRunStatusKind()).toBe("live");
      expect(listener).toHaveBeenCalledTimes(2);

      // Sustained reference silence (> 3000ms) + status hold (2500ms) -> noref
      publishRuntimeStatus(makeStatus({ frames: 3, ref_dbfs: -120 }), 7000);
      expect(getRunStatusKind()).toBe("live");
      vi.advanceTimersByTime(2500);
      expect(getRunStatusKind()).toBe("noref");
      expect(listener).toHaveBeenCalledTimes(3);

      // Bypass toggles immediately
      syncRunStatusInputs(
        { powerOn: true, refSel: "system", dev: false, bypassed: true },
        9600,
      );
      expect(getRunStatusKind()).toBe("bypass");
      expect(listener).toHaveBeenCalledTimes(4);
    } finally {
      unsub();
    }
  });
});
