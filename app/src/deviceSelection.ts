import type { AudioDevice, DeviceList, Platform, ReferenceSource } from "./types";
import {
  isMultichannelVirtualPin,
  isSelfLoopReference,
  isVirtualAudioInput,
  isVirtualAudioOutput,
  pairVirtualAppMic,
} from "./virtualRoute";

export const DEVICE_SELECTION_KEY = "echoless.deviceSelection.v1";

export interface PreferredDeviceSelection {
  input?: string;
  inputName?: string;
  output?: string;
  outputName?: string;
  reference?: string;
  referenceLabel?: string;
}

export interface ActiveDeviceSelection {
  selInput: string;
  selOutput: string;
  reference: string;
}

function nonEmptyString(v: unknown): string | undefined {
  return typeof v === "string" && v.length > 0 ? v : undefined;
}

function nonDefaultId(v: unknown): string | undefined {
  const s = nonEmptyString(v);
  return s && s !== "default" ? s : undefined;
}

export function parsePreferredDeviceSelection(
  raw: string | null,
): PreferredDeviceSelection {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const p = parsed as Record<string, unknown>;
    return {
      input: nonDefaultId(p.input),
      inputName: nonEmptyString(p.inputName),
      output: nonDefaultId(p.output),
      outputName: nonEmptyString(p.outputName),
      reference: nonEmptyString(p.reference),
      referenceLabel: nonEmptyString(p.referenceLabel),
    };
  } catch {
    return {};
  }
}

export function readPreferredDeviceSelection(): PreferredDeviceSelection {
  try {
    return parsePreferredDeviceSelection(
      localStorage.getItem(DEVICE_SELECTION_KEY),
    );
  } catch {
    return {};
  }
}

export function writePreferredDeviceSelection(
  preferred: PreferredDeviceSelection,
): void {
  try {
    localStorage.setItem(DEVICE_SELECTION_KEY, JSON.stringify(preferred));
  } catch {
    // Best-effort preference cache; failing to persist must not block audio.
  }
}

export function initialActiveSelection(
  preferred: PreferredDeviceSelection,
): ActiveDeviceSelection {
  return {
    selInput: preferred.input ?? "default",
    selOutput: preferred.output ?? "default",
    reference: preferred.reference ?? "system",
  };
}

export function pickDefaultInput(
  inputs: readonly AudioDevice[],
  platform?: Platform,
): string {
  const realDefault = inputs.find(
    (d) => d.is_default && !isVirtualAudioInput(d.name, platform),
  );
  if (realDefault) return realDefault.stable_id;
  const firstReal = inputs.find((d) => !isVirtualAudioInput(d.name, platform));
  if (firstReal) return firstReal.stable_id;
  return (
    inputs.find((d) => d.is_default)?.stable_id ??
    inputs[0]?.stable_id ??
    "default"
  );
}

export function pickDefaultOutput(
  outputs: readonly AudioDevice[],
  platform?: Platform,
  inputs?: readonly AudioDevice[],
): string {
  const virtOutputs = outputs.filter((d) =>
    isVirtualAudioOutput(d.name, platform),
  );
  if (virtOutputs.length > 0) {
    const ordered = [
      ...virtOutputs.filter((d) => !isMultichannelVirtualPin(d.name)),
      ...virtOutputs.filter((d) => isMultichannelVirtualPin(d.name)),
    ];
    if (inputs && inputs.length > 0) {
      const paired = ordered.find(
        (d) => pairVirtualAppMic(d, inputs, outputs) != null,
      );
      if (paired) return paired.stable_id;
    }
    return ordered[0].stable_id;
  }
  return (
    outputs.find((d) => d.is_default)?.stable_id ??
    outputs[0]?.stable_id ??
    "default"
  );
}

function refValue(r: ReferenceSource): string {
  return r.selector ?? r.id;
}

export function pickSafeReference(
  devices: DeviceList,
  activeOutput: string,
): string {
  const sys = devices.reference_sources.find((r) => r.id === "system");
  if (sys?.available) return "system";
  const monitor = devices.reference_sources.find(
    (r) =>
      r.available &&
      r.kind === "input" &&
      /monitor/i.test(r.label) &&
      !isSelfLoopReference(r, activeOutput, devices),
  );
  if (monitor) return refValue(monitor);
  return "none";
}

function resolveAudioSlot(
  list: readonly AudioDevice[],
  preferredId: string | undefined,
  preferredName: string | undefined,
  currentActiveId: string,
  pickDefault: (devs: readonly AudioDevice[]) => string,
): {
  active: string;
  nextId: string | undefined;
  nextName: string | undefined;
  changed: boolean;
} {
  if (preferredId) {
    // Tier 1: exact stable_id match
    const byId = list.find((d) => d.stable_id === preferredId);
    if (byId) {
      return {
        active: byId.stable_id,
        nextId: byId.stable_id,
        nextName: byId.name,
        changed: preferredName !== byId.name,
      };
    }
    // Tier 2: unique exact name match (refreshes changed WASAPI/CoreAudio stable_id)
    if (preferredName) {
      const byName = list.filter((d) => d.name === preferredName);
      if (byName.length === 1) {
        return {
          active: byName[0].stable_id,
          nextId: byName[0].stable_id,
          nextName: byName[0].name,
          changed: true,
        };
      }
    }
    // Temporary offline fallback: keep preferred untouched so device auto-reclaims on reconnect
    const keepCurrent =
      currentActiveId !== "" &&
      currentActiveId !== "default" &&
      list.some((d) => d.stable_id === currentActiveId);
    return {
      active: keepCurrent ? currentActiveId : pickDefault(list),
      nextId: preferredId,
      nextName: preferredName,
      changed: false,
    };
  }

  // First-ever initialization (no saved preference yet)
  const picked = pickDefault(list);
  const dev = list.find((d) => d.stable_id === picked);
  if (dev) {
    return {
      active: dev.stable_id,
      nextId: dev.stable_id,
      nextName: dev.name,
      changed: true,
    };
  }
  return {
    active: picked,
    nextId: undefined,
    nextName: undefined,
    changed: false,
  };
}

export function resolveDeviceSelection(params: {
  devices: DeviceList;
  preferred: PreferredDeviceSelection;
  currentActive: ActiveDeviceSelection;
  platform?: Platform;
}): {
  active: ActiveDeviceSelection;
  preferred: PreferredDeviceSelection;
  preferredChanged: boolean;
} {
  const { devices, preferred, currentActive, platform } = params;

  const outRes = resolveAudioSlot(
    devices.outputs,
    preferred.output,
    preferred.outputName,
    currentActive.selOutput,
    (outs) => pickDefaultOutput(outs, platform, devices.inputs),
  );
  const inRes = resolveAudioSlot(
    devices.inputs,
    preferred.input,
    preferred.inputName,
    currentActive.selInput,
    (ins) => pickDefaultInput(ins, platform),
  );

  const activeOutput = outRes.active;
  const isUsableRef = (r: ReferenceSource) =>
    r.available && !isSelfLoopReference(r, activeOutput, devices);

  let activeRef: string;
  let nextPrefRef = preferred.reference;
  let nextPrefRefLabel = preferred.referenceLabel;
  let refChanged = false;

  if (preferred.reference) {
    const byVal = devices.reference_sources.find(
      (r) => isUsableRef(r) && refValue(r) === preferred.reference,
    );
    if (byVal) {
      activeRef = refValue(byVal);
      if (preferred.referenceLabel !== byVal.label) {
        nextPrefRefLabel = byVal.label;
        refChanged = true;
      }
    } else if (
      preferred.reference !== "system" &&
      preferred.reference !== "none" &&
      preferred.referenceLabel
    ) {
      const byLabel = devices.reference_sources.filter(
        (r) => isUsableRef(r) && r.label === preferred.referenceLabel,
      );
      if (byLabel.length === 1) {
        activeRef = refValue(byLabel[0]);
        nextPrefRef = activeRef;
        nextPrefRefLabel = byLabel[0].label;
        refChanged = true;
      } else {
        const keepCur = devices.reference_sources.some(
          (r) => isUsableRef(r) && refValue(r) === currentActive.reference,
        );
        activeRef = keepCur
          ? currentActive.reference
          : pickSafeReference(devices, activeOutput);
      }
    } else {
      const keepCur = devices.reference_sources.some(
        (r) => isUsableRef(r) && refValue(r) === currentActive.reference,
      );
      activeRef = keepCur
        ? currentActive.reference
        : pickSafeReference(devices, activeOutput);
    }
  } else if (devices.reference_sources.length > 0) {
    activeRef = pickSafeReference(devices, activeOutput);
    const matched = devices.reference_sources.find(
      (r) => refValue(r) === activeRef,
    );
    nextPrefRef = activeRef;
    nextPrefRefLabel = matched?.label;
    refChanged = true;
  } else {
    activeRef = currentActive.reference || "system";
  }

  return {
    active: {
      selInput: inRes.active,
      selOutput: outRes.active,
      reference: activeRef,
    },
    preferred: {
      input: inRes.nextId,
      inputName: inRes.nextName,
      output: outRes.nextId,
      outputName: outRes.nextName,
      reference: nextPrefRef,
      referenceLabel: nextPrefRefLabel,
    },
    preferredChanged: inRes.changed || outRes.changed || refChanged,
  };
}

export function selectUserInput(
  preferred: PreferredDeviceSelection,
  devices: DeviceList,
  inputId: string,
): PreferredDeviceSelection {
  const dev = devices.inputs.find((d) => d.stable_id === inputId);
  return {
    ...preferred,
    input: inputId,
    inputName: dev?.name ?? preferred.inputName,
  };
}

export function selectUserReference(
  preferred: PreferredDeviceSelection,
  devices: DeviceList,
  refVal: string,
): PreferredDeviceSelection {
  const src = devices.reference_sources.find((r) => refValue(r) === refVal);
  return {
    ...preferred,
    reference: refVal,
    referenceLabel: src?.label ?? preferred.referenceLabel,
  };
}

export function selectUserOutput(
  preferred: PreferredDeviceSelection,
  currentActive: ActiveDeviceSelection,
  devices: DeviceList,
  outputId: string,
): {
  preferred: PreferredDeviceSelection;
  active: ActiveDeviceSelection;
  referenceReset: boolean;
} {
  const outDev = devices.outputs.find((d) => d.stable_id === outputId);
  const nextPreferred: PreferredDeviceSelection = {
    ...preferred,
    output: outputId,
    outputName: outDev?.name ?? preferred.outputName,
  };

  const curRefSrc = devices.reference_sources.find(
    (r) => refValue(r) === currentActive.reference,
  );
  const loops =
    curRefSrc != null && isSelfLoopReference(curRefSrc, outputId, devices);

  if (loops) {
    const safeRef = pickSafeReference(devices, outputId);
    const safeSrc = devices.reference_sources.find(
      (r) => refValue(r) === safeRef,
    );
    if (nextPreferred.reference === currentActive.reference) {
      nextPreferred.reference = safeRef;
      nextPreferred.referenceLabel = safeSrc?.label;
    }
    return {
      preferred: nextPreferred,
      active: {
        ...currentActive,
        selOutput: outputId,
        reference: safeRef,
      },
      referenceReset: true,
    };
  }

  return {
    preferred: nextPreferred,
    active: {
      ...currentActive,
      selOutput: outputId,
    },
    referenceReset: false,
  };
}
