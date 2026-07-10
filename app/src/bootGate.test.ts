import { describe, expect, it, vi } from "vitest";
import { settleBootGate } from "./bootGate";

function deferred() {
  let resolve!: () => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<void>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

describe("settleBootGate", () => {
  it("lifts exactly once when fonts become ready", async () => {
    const fonts = deferred();
    const timeout = deferred();
    const lift = vi.fn();
    const settled = settleBootGate(fonts.promise, timeout.promise, lift);

    fonts.resolve();
    await settled;
    timeout.resolve();
    await timeout.promise;
    await Promise.resolve();

    expect(lift).toHaveBeenCalledTimes(1);
  });

  it("consumes a fonts-ready rejection and still lifts exactly once", async () => {
    const fonts = deferred();
    const timeout = deferred();
    const lift = vi.fn();
    const settled = settleBootGate(fonts.promise, timeout.promise, lift);

    fonts.reject(new Error("font load failed"));
    await expect(settled).resolves.toBeUndefined();
    timeout.resolve();
    await timeout.promise;
    await Promise.resolve();

    expect(lift).toHaveBeenCalledTimes(1);
  });

  it("consumes a late fonts rejection after the timeout already lifted", async () => {
    const fonts = deferred();
    const timeout = deferred();
    const lift = vi.fn();
    const settled = settleBootGate(fonts.promise, timeout.promise, lift);

    timeout.resolve();
    await settled;
    fonts.reject(new Error("late font load failure"));
    await Promise.resolve();

    expect(lift).toHaveBeenCalledTimes(1);
  });
});
