import { describe, expect, it } from "vitest";
import type { AudioDevice, DeviceList, ReferenceSource } from "./types";
import {
  initialActiveSelection,
  parsePreferredDeviceSelection,
  pickDefaultInput,
  pickSafeReference,
  resolveDeviceSelection,
  selectUserInput,
  selectUserOutput,
  type PreferredDeviceSelection,
} from "./deviceSelection";

function dev(
  kind: "input" | "output",
  name: string,
  stable_id: string,
  opts: { is_default?: boolean; index?: number } = {},
): AudioDevice {
  const index = opts.index ?? 0;
  return {
    id: String(index),
    index,
    name,
    kind,
    is_default: Boolean(opts.is_default),
    selector: String(index),
    stable_id,
    default_sample_rate: 48000,
    channels: 2,
    sample_format: "f32",
    config_error: null,
  };
}

function ref(
  kind: ReferenceSource["kind"],
  label: string,
  stable_id: string,
  available = true,
): ReferenceSource {
  const id =
    kind === "system" || kind === "none" ? kind : `${kind}:${stable_id}`;
  return {
    id,
    label,
    kind,
    available,
    stable_id: id,
    selector: id,
  };
}

function list(
  inputs: AudioDevice[],
  outputs: AudioDevice[],
  reference_sources: ReferenceSource[],
): DeviceList {
  return { ok: true, inputs, outputs, reference_sources };
}

describe("persistent device selection (B-27)", () => {
  it("parses legacy v1 JSON without names and ignores default placeholders", () => {
    expect(
      parsePreferredDeviceSelection(
        JSON.stringify({
          input: "usb-mic-1",
          output: "cable-b-in",
          reference: "system",
        }),
      ),
    ).toEqual({
      input: "usb-mic-1",
      inputName: undefined,
      output: "cable-b-in",
      outputName: undefined,
      reference: "system",
      referenceLabel: undefined,
    });

    expect(
      parsePreferredDeviceSelection(
        JSON.stringify({ input: "default", output: "", reference: "none" }),
      ),
    ).toEqual({
      input: undefined,
      inputName: undefined,
      output: undefined,
      outputName: undefined,
      reference: "none",
      referenceLabel: undefined,
    });
  });

  it("does not overwrite preferred devices during temporary boot/unplug fallback and auto-reclaims on reconnect", () => {
    const preferred: PreferredDeviceSelection = {
      input: "usb-mic-id",
      inputName: "Shure MV7",
      output: "cable-b-in",
      outputName: "CABLE-B Input / VB-Audio Cable B",
      reference: "system",
      referenceLabel: "System audio",
    };

    // Boot refresh 1: USB mic is still enumerating, system loopback not ready yet
    const bootDevices = list(
      [dev("input", "Built-in Mic", "builtin-mic", { is_default: true })],
      [
        dev("output", "CABLE Input / VB-Audio Virtual Cable", "cable-in"),
        dev("output", "CABLE-B Input / VB-Audio Cable B", "cable-b-in"),
      ],
      [
        ref("system", "System audio", "system", false),
        ref("none", "No reference", "none", true),
      ],
    );

    const step1 = resolveDeviceSelection({
      devices: bootDevices,
      preferred,
      currentActive: initialActiveSelection(preferred),
      platform: "windows",
    });

    // Active temporarily falls back, but preferred stays intact
    expect(step1.active).toEqual({
      selInput: "builtin-mic",
      selOutput: "cable-b-in",
      reference: "none",
    });
    expect(step1.preferred).toEqual(preferred);
    expect(step1.preferredChanged).toBe(false);

    // Refresh 2: USB mic finishes enumerating and system reference comes online
    const readyDevices = list(
      [
        dev("input", "Built-in Mic", "builtin-mic", { is_default: true }),
        dev("input", "Shure MV7", "usb-mic-id", { index: 1 }),
      ],
      bootDevices.outputs,
      [
        ref("system", "System audio", "system", true),
        ref("none", "No reference", "none", true),
      ],
    );

    const step2 = resolveDeviceSelection({
      devices: readyDevices,
      preferred: step1.preferred,
      currentActive: step1.active,
      platform: "windows",
    });

    expect(step2.active).toEqual({
      selInput: "usb-mic-id",
      selOutput: "cable-b-in",
      reference: "system",
    });
  });

  it("does not reclaim an unplugged device after the user manually selects a different mic", () => {
    const initialPref: PreferredDeviceSelection = {
      input: "usb-mic-id",
      inputName: "Shure MV7",
      output: "cable-in",
      outputName: "CABLE Input",
      reference: "system",
    };
    const devicesWithoutUsb = list(
      [
        dev("input", "Built-in Mic", "builtin-mic", { is_default: true }),
        dev("input", "Headset Mic", "headset-mic", { index: 1 }),
      ],
      [dev("output", "CABLE Input", "cable-in")],
      [ref("system", "System audio", "system", true)],
    );

    const userPref = selectUserInput(initialPref, devicesWithoutUsb, "headset-mic");
    expect(userPref.input).toBe("headset-mic");
    expect(userPref.inputName).toBe("Headset Mic");

    const devicesWithUsbBack = list(
      [
        ...devicesWithoutUsb.inputs,
        dev("input", "Shure MV7", "usb-mic-id", { index: 2 }),
      ],
      devicesWithoutUsb.outputs,
      devicesWithoutUsb.reference_sources,
    );

    const resolved = resolveDeviceSelection({
      devices: devicesWithUsbBack,
      preferred: userPref,
      currentActive: {
        selInput: "headset-mic",
        selOutput: "cable-in",
        reference: "system",
      },
    });
    expect(resolved.active.selInput).toBe("headset-mic");
  });

  it("matches by unique device name when WASAPI/CoreAudio stable_id changes and refreshes the saved ID", () => {
    const preferred: PreferredDeviceSelection = {
      input: "{old-wasapi-guid}",
      inputName: "Blue Yeti USB",
      output: "{old-cable-guid}",
      outputName: "CABLE-B Input / VB-Audio Cable B",
      reference: "system",
      referenceLabel: "System audio",
    };

    const devices = list(
      [
        dev("input", "Built-in Mic", "builtin-mic", { is_default: true }),
        dev("input", "Blue Yeti USB", "{new-wasapi-guid}", { index: 1 }),
      ],
      [
        dev("output", "CABLE-B Input / VB-Audio Cable B", "{new-cable-guid}"),
      ],
      [ref("system", "System audio", "system", true)],
    );

    const resolved = resolveDeviceSelection({
      devices,
      preferred,
      currentActive: initialActiveSelection(preferred),
      platform: "windows",
    });

    expect(resolved.active.selInput).toBe("{new-wasapi-guid}");
    expect(resolved.active.selOutput).toBe("{new-cable-guid}");
    expect(resolved.preferred.input).toBe("{new-wasapi-guid}");
    expect(resolved.preferred.output).toBe("{new-cable-guid}");
    expect(resolved.preferredChanged).toBe(true);
  });

  it("never matches by fragile numeric selector index", () => {
    const preferred: PreferredDeviceSelection = {
      input: "0",
      inputName: "Old Mic",
    };
    const devices = list(
      [dev("input", "Different Mic", "diff-stable-id", { index: 0 })],
      [],
      [],
    );

    const resolved = resolveDeviceSelection({
      devices,
      preferred,
      currentActive: { selInput: "0", selOutput: "default", reference: "none" },
    });

    expect(resolved.active.selInput).toBe("diff-stable-id");
    expect(resolved.preferred.input).toBe("0");
    expect(resolved.preferredChanged).toBe(false);
  });

  it("pickDefaultInput skips virtual cables even when marked system default", () => {
    const inputs = [
      dev("input", "CABLE Output / VB-Audio Virtual Cable", "cable-out", {
        is_default: true,
        index: 0,
      }),
      dev("input", "BlackHole 2ch", "bh-in", { index: 1 }),
      dev("input", "USB Podcast Mic", "usb-mic", { index: 2 }),
    ];

    expect(pickDefaultInput(inputs, "windows")).toBe("usb-mic");
  });

  it("Linux pickSafeReference skips self-loop monitor and picks another monitor or none", () => {
    const devices = list(
      [
        dev("input", "Monitor of Echoless-Output", "mon-echo"),
        dev("input", "Monitor of Built-in Audio", "mon-spk", { index: 1 }),
      ],
      [
        dev("output", "Echoless-Output", "sink-echo"),
        dev("output", "Built-in Audio", "sink-spk", { index: 1 }),
      ],
      [
        ref("none", "No reference", "none", true),
        ref("input", "Monitor of Echoless-Output", "mon-echo", true),
        ref("input", "Monitor of Built-in Audio", "mon-spk", true),
      ],
    );

    expect(pickSafeReference(devices, "sink-echo")).toBe("input:mon-spk");

    const onlyOwnMonitor = list(
      [dev("input", "Monitor of Echoless-Output", "mon-echo")],
      [dev("output", "Echoless-Output", "sink-echo")],
      [
        ref("none", "No reference", "none", true),
        ref("input", "Monitor of Echoless-Output", "mon-echo", true),
      ],
    );
    expect(pickSafeReference(onlyOwnMonitor, "sink-echo")).toBe("none");
  });

  it("selectUserOutput resets reference to a safe source when switching output creates a self-loop", () => {
    const devices = list(
      [
        dev("input", "Shure MV7", "mic-1"),
        dev("input", "CABLE-B Output / VB-Audio Cable B", "in-b", { index: 1 }),
      ],
      [
        dev("output", "CABLE-A Input / VB-Audio Cable A", "out-a"),
        dev("output", "CABLE-B Input / VB-Audio Cable B", "out-b", { index: 1 }),
      ],
      [
        ref("system", "System audio", "system", true),
        ref("none", "No reference", "none", true),
        ref("input", "CABLE-B Output / VB-Audio Cable B", "in-b", true),
      ],
    );

    const res = selectUserOutput(
      {
        input: "mic-1",
        output: "out-a",
        reference: "input:in-b",
        referenceLabel: "CABLE-B Output / VB-Audio Cable B",
      },
      {
        selInput: "mic-1",
        selOutput: "out-a",
        reference: "input:in-b",
      },
      devices,
      "out-b",
    );

    expect(res.referenceReset).toBe(true);
    expect(res.active.selOutput).toBe("out-b");
    expect(res.active.reference).toBe("system");
    expect(res.preferred.output).toBe("out-b");
    expect(res.preferred.reference).toBe("system");
  });

  it("prefers standard paired VB-CABLE output over CABLE In 16 Ch regardless of enumeration order", () => {
    const devices = list(
      [
        dev("input", "麦克风 / USB Condenser Microphone", "mic-usb", {
          is_default: true,
        }),
        dev("input", "CABLE Output / VB-Audio Virtual Cable", "in-vb", {
          index: 1,
        }),
      ],
      [
        dev("output", "CABLE In 16 Ch / VB-Audio Virtual Cable", "out-vb-16ch"),
        dev("output", "扬声器 / VB-Audio Virtual Cable", "out-vb-std", {
          index: 1,
        }),
      ],
      [
        ref("system", "System audio", "system", true),
        ref("none", "No reference", "none", true),
      ],
    );

    const res = resolveDeviceSelection({
      devices,
      preferred: {},
      currentActive: { selInput: "default", selOutput: "default", reference: "system" },
      platform: "windows",
    });

    expect(res.active.selOutput).toBe("out-vb-std");
    expect(res.preferred.output).toBe("out-vb-std");
    expect(res.preferred.outputName).toBe("扬声器 / VB-Audio Virtual Cable");
  });
});
