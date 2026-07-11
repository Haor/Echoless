import { describe, expect, it } from "vitest";

import apiSource from "./api.ts?raw";
import appSource from "./App.tsx?raw";
import setupSource from "./pages/RtxSetupPage.tsx?raw";

describe("indeterminate NVAFX download progress", () => {
  it("keeps nullable percent and falls back to stage or received bytes", () => {
    expect(apiSource).toContain("pct: number | null;");
    expect(setupSource).toContain("pct != null");
    expect(setupSource).toContain("recv != null && recv > 0");
    expect(setupSource).toContain("MiB");
  });

  it("does not model an extra HEAD or Content-Length request", () => {
    expect(appSource).not.toMatch(/\bHEAD\b|Content-Length/);
    expect(apiSource).not.toMatch(/\bHEAD\b|Content-Length/);
    expect(appSource).toContain("nvafxPct: null");
  });
});
