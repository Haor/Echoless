import { describe, expect, it } from "vitest";
import type { AudioDevice, DeviceList, DoctorAudio, ReferenceSource } from "./types";
import {
  isSelfLoopReference,
  isVirtualAudioInput,
  isVirtualAudioOutput,
  pairVirtualAppMic,
  resolveVirtualRoute,
} from "./virtualRoute";

function dev(
  kind: "input" | "output",
  name: string,
  stable_id: string,
  index = 0,
): AudioDevice {
  return {
    id: String(index),
    index,
    name,
    kind,
    is_default: false,
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

describe("virtual audio detection", () => {
  it("recognizes standard virtual audio drivers and rejects Virtual Desktop Audio", () => {
    expect(isVirtualAudioOutput("CABLE Input (VB-Audio Virtual Cable)")).toBe(true);
    expect(isVirtualAudioOutput("CABLE-B Input / VB-Audio Cable B")).toBe(true);
    expect(isVirtualAudioOutput("BlackHole 2ch")).toBe(true);
    expect(isVirtualAudioOutput("Echoless-Output", "linux")).toBe(true);
    expect(isVirtualAudioInput("Monitor of Echoless-Output", "linux")).toBe(true);

    expect(isVirtualAudioOutput("Virtual Desktop Audio")).toBe(false);
    expect(isVirtualAudioInput("Virtual Desktop Audio")).toBe(false);
    expect(isVirtualAudioOutput("Speakers / Realtek(R) Audio")).toBe(false);
  });
});

describe("three-tier deterministic virtual mic pairing", () => {
  it("pairs standard and multi VB-CABLE, Hi-Fi Cable, and VoiceMeeter via word-boundary Input -> Output", () => {
    const inputs = [
      dev("input", "CABLE Output / VB-Audio Virtual Cable", "in-base"),
      dev("input", "CABLE-A Output / VB-Audio Cable A", "in-a"),
      dev("input", "CABLE-B Output / VB-Audio Cable B", "in-b"),
      dev("input", "Hi-Fi Cable Output / VB-Audio Hi-Fi Cable", "in-hifi"),
      dev("input", "VoiceMeeter Output (VB-Audio VoiceMeeter VAIO)", "in-vm"),
    ];

    expect(
      pairVirtualAppMic(
        dev("output", "CABLE Input / VB-Audio Virtual Cable", "out-base"),
        inputs,
      )?.stable_id,
    ).toBe("in-base");
    expect(
      pairVirtualAppMic(
        dev("output", "CABLE-B Input / VB-Audio Cable B", "out-b"),
        inputs,
      )?.stable_id,
    ).toBe("in-b");
    expect(
      pairVirtualAppMic(
        dev("output", "Hi-Fi Cable Input / VB-Audio Hi-Fi Cable", "out-hifi"),
        inputs,
      )?.stable_id,
    ).toBe("in-hifi");
    expect(
      pairVirtualAppMic(
        dev("output", "VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)", "out-vm"),
        inputs,
      )?.stable_id,
    ).toBe("in-vm");
  });

  it("pairs same-name macOS/VAC devices and Linux monitor sources", () => {
    const inputs = [
      dev("input", "BlackHole 2ch", "bh2-in"),
      dev("input", "BlackHole 16ch", "bh16-in"),
      dev("input", "Line 1 (Virtual Audio Cable)", "vac1-in"),
      dev("input", "Line 2 (Virtual Audio Cable)", "vac2-in"),
      dev("input", "Monitor of Echoless-Output", "mon-echo"),
      dev("input", "echoless_out.monitor", "dot-mon"),
    ];

    expect(
      pairVirtualAppMic(dev("output", "BlackHole 16ch", "bh16-out"), inputs)
        ?.stable_id,
    ).toBe("bh16-in");
    expect(
      pairVirtualAppMic(
        dev("output", "Line 2 (Virtual Audio Cable)", "vac2-out"),
        inputs,
      )?.stable_id,
    ).toBe("vac2-in");
    expect(
      pairVirtualAppMic(dev("output", "Echoless-Output", "echo-out"), inputs)
        ?.stable_id,
    ).toBe("mon-echo");
    expect(
      pairVirtualAppMic(dev("output", "echoless_out", "echo-raw"), inputs)
        ?.stable_id,
    ).toBe("dot-mon");
  });

  it("falls back to Tier 2 unique adapter suffix for renamed Windows endpoints", () => {
    const outputs = [
      dev("output", "Stream Out / VB-Audio Cable B", "out-b"),
      dev("output", "CABLE Input / VB-Audio Virtual Cable", "out-base"),
    ];
    const inputs = [
      dev("input", "CABLE Output / VB-Audio Virtual Cable", "in-base"),
      dev("input", "Discord Mic / VB-Audio Cable B", "in-b-renamed"),
    ];

    expect(pairVirtualAppMic(outputs[0], inputs, outputs)?.stable_id).toBe(
      "in-b-renamed",
    );
  });

  it("never cross-pairs a disabled cable or shared-driver VAC line to another cable", () => {
    const vbOutputs = [
      dev("output", "CABLE Input / VB-Audio Virtual Cable", "out-base"),
      dev("output", "CABLE-B Input / VB-Audio Cable B", "out-b"),
    ];
    const vbInputs = [
      dev("input", "CABLE Output / VB-Audio Virtual Cable", "in-base"),
    ];
    expect(pairVirtualAppMic(vbOutputs[1], vbInputs, vbOutputs)).toBeNull();

    const vacOutputs = [
      dev("output", "Line 1 (Virtual Audio Cable)", "vac1-out"),
      dev("output", "Line 2 (Virtual Audio Cable)", "vac2-out"),
    ];
    const vacInputs = [dev("input", "Line 2 (Virtual Audio Cable)", "vac2-in")];
    expect(pairVirtualAppMic(vacOutputs[0], vacInputs, vacOutputs)).toBeNull();
  });

  it("never pairs physical speakers with a physical microphone sharing the same audio driver", () => {
    const outputs = [dev("output", "Speakers / Realtek(R) Audio", "spk")];
    const inputs = [dev("input", "Microphone / Realtek(R) Audio", "mic")];
    expect(pairVirtualAppMic(outputs[0], inputs, outputs)).toBeNull();
  });
});

describe("resolveVirtualRoute", () => {
  it("locks onto the selected virtual output and reports missing counterpart honestly", () => {
    const devices: Pick<DeviceList, "inputs" | "outputs"> = {
      outputs: [
        dev("output", "CABLE Input / VB-Audio Virtual Cable", "out-base"),
        dev("output", "CABLE-B Input / VB-Audio Cable B", "out-b"),
      ],
      inputs: [
        dev("input", "CABLE Output / VB-Audio Virtual Cable", "in-base"),
      ],
    };

    const brokenSelected = resolveVirtualRoute({
      selectedOutput: "out-b",
      devices,
      platform: "windows",
    });
    expect(brokenSelected).toMatchObject({
      ready: false,
      selectedIsVirtual: true,
      counterpartMissing: true,
      appMic: null,
    });
    expect(brokenSelected.output?.stable_id).toBe("out-b");

    const okSelected = resolveVirtualRoute({
      selectedOutput: "out-base",
      devices,
      platform: "windows",
    });
    expect(okSelected).toMatchObject({
      ready: true,
      selectedIsVirtual: true,
      counterpartMissing: false,
    });
    expect(okSelected.appMic?.stable_id).toBe("in-base");
  });

  it("picks an intact paired cable when the currently selected output is a physical speaker", () => {
    const devices: Pick<DeviceList, "inputs" | "outputs"> = {
      outputs: [
        dev("output", "Speakers / Realtek(R) Audio", "spk"),
        dev("output", "CABLE-A Input / VB-Audio Cable A", "out-a"),
        dev("output", "CABLE-B Input / VB-Audio Cable B", "out-b"),
      ],
      inputs: [
        // CABLE-A Output is disabled; CABLE-B Output is intact
        dev("input", "CABLE-B Output / VB-Audio Cable B", "in-b"),
      ],
    };

    const route = resolveVirtualRoute({
      selectedOutput: "spk",
      devices,
      platform: "windows",
    });
    expect(route.ready).toBe(true);
    expect(route.selectedIsVirtual).toBe(false);
    expect(route.output?.stable_id).toBe("out-b");
    expect(route.appMic?.stable_id).toBe("in-b");
  });

  it("falls back to doctor candidates when live devices are omitted (dev simulation)", () => {
    const doctor: DoctorAudio = {
      ok: true,
      platform: "windows",
      virtual_output_detected: true,
      candidate_outputs: [
        {
          index: 1,
          kind: "output",
          name: "CABLE Input (VB-Audio Virtual Cable)",
          selector: "1",
          stable_id: "cable-in",
        },
      ],
      candidate_inputs: [
        {
          index: 2,
          kind: "input",
          name: "CABLE Output (VB-Audio Virtual Cable)",
          selector: "2",
          stable_id: "cable-out",
        },
      ],
      recommended_driver: "vb-cable",
      install_status: "installed",
      needs_reboot: false,
      permission_state: "unknown",
      reference_sources: [],
    };

    const route = resolveVirtualRoute({
      selectedOutput: "unrelated-mac-id",
      devices: null,
      doctor,
      platform: "windows",
    });
    expect(route.ready).toBe(true);
    expect(route.output?.name).toBe("CABLE Input (VB-Audio Virtual Cable)");
    expect(route.appMic?.name).toBe("CABLE Output (VB-Audio Virtual Cable)");
  });
});

describe("isSelfLoopReference", () => {
  it("strips input:/output: prefixes and blocks paired Windows virtual input while allowing other cables", () => {
    const devices: Pick<DeviceList, "inputs" | "outputs"> = {
      outputs: [
        dev("output", "CABLE-A Input / VB-Audio Cable A", "{out-a}"),
        dev("output", "CABLE-B Input / VB-Audio Cable B", "{out-b}"),
      ],
      inputs: [
        dev("input", "CABLE-A Output / VB-Audio Cable A", "{in-a}"),
        dev("input", "CABLE-B Output / VB-Audio Cable B", "{in-b}"),
      ],
    };

    expect(
      isSelfLoopReference(
        ref("output", "CABLE-B Input / VB-Audio Cable B", "{out-b}"),
        "{out-b}",
        devices,
      ),
    ).toBe(true);
    expect(
      isSelfLoopReference(
        ref("input", "CABLE-B Output / VB-Audio Cable B", "{in-b}"),
        "{out-b}",
        devices,
      ),
    ).toBe(true);
    expect(
      isSelfLoopReference(
        ref("input", "CABLE-A Output / VB-Audio Cable A", "{in-a}"),
        "{out-b}",
        devices,
      ),
    ).toBe(false);
    expect(isSelfLoopReference(ref("system", "System audio", "system"), "{out-b}", devices)).toBe(false);
  });

  it("blocks Linux Monitor of Echoless-Output while allowing physical sink monitors", () => {
    const devices: Pick<DeviceList, "inputs" | "outputs"> = {
      outputs: [
        dev("output", "Echoless-Output", "sink-echo"),
        dev("output", "Built-in Audio Analog Stereo", "sink-spk"),
      ],
      inputs: [
        dev("input", "Monitor of Echoless-Output", "src-echo-mon"),
        dev("input", "Monitor of Built-in Audio Analog Stereo", "src-spk-mon"),
      ],
    };

    expect(
      isSelfLoopReference(
        ref("input", "Monitor of Echoless-Output", "src-echo-mon"),
        "sink-echo",
        devices,
      ),
    ).toBe(true);
    expect(
      isSelfLoopReference(
        ref("input", "Monitor of Built-in Audio Analog Stereo", "src-spk-mon"),
        "sink-echo",
        devices,
      ),
    ).toBe(false);
  });

  it("pairs localized VB-CABLE playback pins (扬声器 + CABLE In 16 Ch) with CABLE Output and blocks self-loops", () => {
    const devices: Pick<DeviceList, "inputs" | "outputs"> = {
      outputs: [
        dev("output", "扬声器 / ToDesk Virtual Audio", "{out-todesk}"),
        dev("output", "CABLE In 16 Ch / VB-Audio Virtual Cable", "{out-vb-16ch}"),
        dev("output", "扬声器 / VB-Audio Virtual Cable", "{out-vb-std}"),
        dev("output", "扬声器 / 2- XMOS USB Audio", "{out-xmos}"),
      ],
      inputs: [
        dev("input", "CABLE Output / VB-Audio Virtual Cable", "{in-vb}"),
      ],
    };

    expect(
      resolveVirtualRoute({
        selectedOutput: "{out-vb-16ch}",
        devices,
        platform: "windows",
      }),
    ).toMatchObject({
      ready: true,
      selectedIsVirtual: true,
      counterpartMissing: false,
      appMic: { stable_id: "{in-vb}" },
    });

    expect(
      resolveVirtualRoute({
        selectedOutput: "{out-vb-std}",
        devices,
        platform: "windows",
      }),
    ).toMatchObject({
      ready: true,
      selectedIsVirtual: true,
      counterpartMissing: false,
      appMic: { stable_id: "{in-vb}" },
    });

    // When a physical output is selected, prefer the standard endpoint over CABLE In 16 Ch.
    expect(
      resolveVirtualRoute({
        selectedOutput: "{out-xmos}",
        devices,
        platform: "windows",
      }),
    ).toMatchObject({
      ready: true,
      selectedIsVirtual: false,
      output: { stable_id: "{out-vb-std}" },
      appMic: { stable_id: "{in-vb}" },
    });

    // Both CABLE Output and the sibling VB-CABLE output pin must be flagged as self-loops.
    expect(
      isSelfLoopReference(
        ref("input", "CABLE Output / VB-Audio Virtual Cable", "{in-vb}"),
        "{out-vb-16ch}",
        devices,
      ),
    ).toBe(true);
    expect(
      isSelfLoopReference(
        ref("output", "扬声器 / VB-Audio Virtual Cable", "{out-vb-std}"),
        "{out-vb-16ch}",
        devices,
      ),
    ).toBe(true);
    expect(
      isSelfLoopReference(
        ref("output", "扬声器 / 2- XMOS USB Audio", "{out-xmos}"),
        "{out-vb-16ch}",
        devices,
      ),
    ).toBe(false);
  });

  it("falls back gently to a sole non-conflicting virtual input and distinguishes missing vs unmatched virtual inputs", () => {
    const soleFallback: Pick<DeviceList, "inputs" | "outputs"> = {
      outputs: [dev("output", "VoiceMeeter Virtual Send", "vm-out")],
      inputs: [dev("input", "VoiceMeeter Virtual Mic", "vm-in")],
    };
    expect(
      resolveVirtualRoute({
        selectedOutput: "vm-out",
        devices: soleFallback,
        platform: "windows",
      }),
    ).toMatchObject({
      ready: true,
      appMic: { stable_id: "vm-in" },
      hasVirtualInput: true,
    });

    const unmatchedButHasVirtualInput: Pick<DeviceList, "inputs" | "outputs"> = {
      outputs: [
        dev("output", "CABLE Input / VB-Audio Virtual Cable", "out-base"),
        dev("output", "CABLE-B Input / VB-Audio Cable B", "out-b"),
      ],
      inputs: [dev("input", "CABLE Output / VB-Audio Virtual Cable", "in-base")],
    };
    expect(
      resolveVirtualRoute({
        selectedOutput: "out-b",
        devices: unmatchedButHasVirtualInput,
        platform: "windows",
      }),
    ).toMatchObject({
      ready: false,
      counterpartMissing: true,
      hasVirtualInput: true,
    });

    const noVirtualInputAtAll: Pick<DeviceList, "inputs" | "outputs"> = {
      outputs: [dev("output", "CABLE Input / VB-Audio Virtual Cable", "out-base")],
      inputs: [dev("input", "USB Condenser Microphone", "mic-usb")],
    };
    expect(
      resolveVirtualRoute({
        selectedOutput: "out-base",
        devices: noVirtualInputAtAll,
        platform: "windows",
      }),
    ).toMatchObject({
      ready: false,
      counterpartMissing: true,
      hasVirtualInput: false,
    });
  });
});
