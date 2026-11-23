import { describe, expect, it } from "vitest";
import { normalizeFieldCommit } from "./components/Controls";

describe("Field commit normalization (B-13)", () => {
  it("clears invalid numeric text back to empty without firing onCommit when value is already null", () => {
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
  });

  it("clamps out-of-range numeric input and skips onCommit when clamped result matches current value", () => {
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
        text: "-5",
        currentValue: 0,
        numeric: true,
        min: 0,
        max: 500,
        integer: true,
      }),
    ).toEqual({ value: 0, text: "0", changed: false });
  });

  it("skips onCommit on blur when value did not change, and fires onCommit when normalized value changes", () => {
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

    expect(
      normalizeFieldCommit({
        text: "   ",
        currentValue: 52,
        numeric: true,
      }),
    ).toEqual({ value: null, text: "", changed: true });

    expect(
      normalizeFieldCommit({
        text: "",
        currentValue: undefined,
        numeric: true,
      }),
    ).toEqual({ value: null, text: "", changed: false });
  });
});
