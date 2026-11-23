import type {
  DeviceList,
  DoctorAudio,
  Platform,
  ReferenceSource,
} from "./types";

export interface NamedDevice {
  name: string;
  stable_id?: string;
}

export interface ResolvedVirtualRoute {
  output: NamedDevice | null;
  appMic: NamedDevice | null;
  ready: boolean;
  selectedIsVirtual: boolean;
  counterpartMissing: boolean;
  hasVirtualInput: boolean;
}

const VIRTUAL_DESKTOP_RE = /\bvirtual\s+desktop\b/i;
const VIRTUAL_AUDIO_RE =
  /vb-?audio|vb-?cable|vbcable|\bcable(?:-[a-z0-9]+)?\s+(?:input|output)\b|hi-?fi\s+cable|voicemeeter|blackhole|soundflower|virtual\s+audio\s+cable|echoless|null/i;
const LINUX_VIRTUAL_OUTPUT_RE = /echoless|null/i;
const LINUX_MONITOR_RE = /monitor/i;
const WORD_INPUT_RE = /\binput\b/i;
const CABLE_LINE_TOKEN_RE = /\b(?:cable-[a-z0-9]+|line\s+\d+)\b/i;
const MULTICHANNEL_VIRTUAL_PIN_RE = /\b(?:in\s+)?(?:16|32|64)\s*ch\b/i;

function normalizeName(name: string): string {
  return name.trim().replace(/\s+/g, " ").toLowerCase();
}

export function isMultichannelVirtualPin(name: string): boolean {
  return MULTICHANNEL_VIRTUAL_PIN_RE.test(name);
}

export function stripReferencePrefix(value: string): string {
  return value.replace(/^(?:input|output):/, "");
}

export function isVirtualAudioOutput(name: string, platform?: Platform): boolean {
  if (VIRTUAL_DESKTOP_RE.test(name)) return false;
  if (platform === "linux") return LINUX_VIRTUAL_OUTPUT_RE.test(name);
  return VIRTUAL_AUDIO_RE.test(name);
}

export function isVirtualAudioInput(name: string, platform?: Platform): boolean {
  if (VIRTUAL_DESKTOP_RE.test(name)) return false;
  if (platform === "linux") return LINUX_MONITOR_RE.test(name);
  return VIRTUAL_AUDIO_RE.test(name);
}

/**
 * Extracts a normalized driver/adapter suffix from Windows device names:
 * - WASAPI cpal format: "CABLE-B Input / VB-Audio Cable B" -> "vb-audio cable b"
 * - Parenthesized driver: "CABLE Input (VB-Audio Virtual Cable)" -> "vb-audio virtual cable"
 */
export function extractAdapterSuffix(name: string): string | null {
  const slashIdx = name.lastIndexOf(" / ");
  if (slashIdx !== -1) {
    const tail = normalizeName(name.slice(slashIdx + 3));
    if (tail.length > 0) return tail;
  }
  const parenMatch = name.match(/\(([^()]+)\)\s*$/);
  if (parenMatch) {
    const tail = normalizeName(parenMatch[1]);
    if (tail.length > 0) return tail;
  }
  return null;
}

function cableLineToken(name: string): string | null {
  const match = name.match(CABLE_LINE_TOKEN_RE);
  return match ? normalizeName(match[0]) : null;
}

/**
 * Three-tier deterministic pairing from an output endpoint to its virtual app mic input:
 * 1. Exact deterministic name match:
 *    - word-boundary `\binput\b` -> `output` (CABLE Input, CABLE-A/B/C/D Input, Hi-Fi Cable Input, VoiceMeeter Input)
 *    - Linux `Monitor of <name>` / `<name>.monitor`
 *    - exact same-name match (BlackHole 2ch/16ch, VB-Cable, VAC Line 1/2)
 * 2. Unique adapter suffix match (` / <driver>` or `(<driver>)`) for virtual devices when no other output shares that suffix.
 * 3. Missing/unpaired -> `null` (never cross-pairs a different cable).
 */
export function pairVirtualAppMic<T extends NamedDevice>(
  output: NamedDevice,
  inputs: readonly T[],
  allOutputs?: readonly NamedDevice[],
): T | null {
  const outNorm = normalizeName(output.name);

  // Tier 1a: word-boundary Input -> Output
  if (WORD_INPUT_RE.test(output.name)) {
    const expected = normalizeName(output.name.replace(WORD_INPUT_RE, "output"));
    const match = inputs.find((inp) => normalizeName(inp.name) === expected);
    if (match) return match;
  }

  // Tier 1b: Linux PipeWire / PulseAudio monitor source
  const monitorOf = `monitor of ${outNorm}`;
  const dotMonitor = `${outNorm}.monitor`;
  const monitorMatch = inputs.find((inp) => {
    const inpNorm = normalizeName(inp.name);
    return inpNorm === monitorOf || inpNorm === dotMonitor;
  });
  if (monitorMatch) return monitorMatch;

  // Tier 1c: exact same-name virtual loopback (BlackHole, VB-Cable macOS, VAC Line N)
  if (!WORD_INPUT_RE.test(output.name)) {
    const sameName = inputs.find((inp) => normalizeName(inp.name) === outNorm);
    if (sameName) return sameName;
  }

  // Tier 2: adapter/driver suffix match for virtual outputs (handles renamed or
  // localized endpoints such as "扬声器 / VB-Audio Virtual Cable" and companion
  // multichannel pins such as "CABLE In 16 Ch / VB-Audio Virtual Cable" without
  // hardcoding driver whitelists). Only rejects inputs with a conflicting line
  // token (e.g. VAC Line 1 vs Line 2) or owned by another explicit-token output.
  if (!isVirtualAudioOutput(output.name)) return null;
  const suffix = extractAdapterSuffix(output.name);
  const outToken = cableLineToken(output.name);

  if (suffix) {
    const siblingTokens = new Set(
      (allOutputs ?? [])
        .filter(
          (out) =>
            normalizeName(out.name) !== outNorm &&
            extractAdapterSuffix(out.name) === suffix,
        )
        .map((out) => cableLineToken(out.name))
        .filter((tok): tok is string => tok != null),
    );
    const suffixMatches = inputs.filter((inp) => {
      if (extractAdapterSuffix(inp.name) !== suffix) return false;
      const inpToken = cableLineToken(inp.name);
      if (outToken && inpToken && outToken !== inpToken) return false;
      if (!outToken && inpToken && siblingTokens.has(inpToken)) return false;
      return true;
    });

    if (suffixMatches.length === 1) return suffixMatches[0];
  }

  // Tier 3: gentle fallback when the machine has a single virtual mic input
  // and neither its adapter suffix nor line token contradicts the output.
  const virtualInputs = inputs.filter((inp) => isVirtualAudioInput(inp.name));
  if (virtualInputs.length === 1) {
    const sole = virtualInputs[0];
    const soleSuffix = extractAdapterSuffix(sole.name);
    const soleToken = cableLineToken(sole.name);
    const suffixConflict = Boolean(suffix && soleSuffix && suffix !== soleSuffix);
    const tokenConflict = Boolean(outToken && soleToken && outToken !== soleToken);
    if (!suffixConflict && !tokenConflict) return sole;
  }

  return null;
}

export function resolveVirtualRoute(params: {
  selectedOutput?: string | null;
  devices?: Pick<DeviceList, "inputs" | "outputs"> | null;
  doctor?: DoctorAudio | null;
  platform?: Platform;
}): ResolvedVirtualRoute {
  const { selectedOutput, devices, doctor, platform } = params;
  const hasLiveDevices =
    Boolean(devices) &&
    ((devices?.outputs.length ?? 0) > 0 || (devices?.inputs.length ?? 0) > 0);
  const activeOutputs: readonly NamedDevice[] = hasLiveDevices
    ? devices!.outputs
    : (doctor?.candidate_outputs ?? []);
  const activeInputs: readonly NamedDevice[] = hasLiveDevices
    ? devices!.inputs
    : (doctor?.candidate_inputs ?? []);

  const doctorOutIds = new Set(
    (doctor?.candidate_outputs ?? []).map((c) => c.stable_id),
  );
  const doctorInIds = new Set(
    (doctor?.candidate_inputs ?? []).map((c) => c.stable_id),
  );
  const isVirtOut = (dev: NamedDevice) =>
    isVirtualAudioOutput(dev.name, platform) ||
    (dev.stable_id != null && doctorOutIds.has(dev.stable_id));
  const isVirtIn = (dev: NamedDevice) =>
    isVirtualAudioInput(dev.name, platform) ||
    (dev.stable_id != null && doctorInIds.has(dev.stable_id));
  const hasVirtualInput = activeInputs.some(isVirtIn);

  const selOutDev =
    selectedOutput && selectedOutput !== "default"
      ? activeOutputs.find((d) => d.stable_id === selectedOutput) ?? null
      : null;

  if (selOutDev && isVirtOut(selOutDev)) {
    const paired = pairVirtualAppMic(selOutDev, activeInputs, activeOutputs);
    if (paired) {
      return {
        output: selOutDev,
        appMic: paired,
        ready: true,
        selectedIsVirtual: true,
        counterpartMissing: false,
        hasVirtualInput: true,
      };
    }
    return {
      output: selOutDev,
      appMic: null,
      ready: false,
      selectedIsVirtual: true,
      counterpartMissing: true,
      hasVirtualInput,
    };
  }

  // Current output is physical (or unselected): recommend a genuinely paired virtual cable,
  // preferring standard 2ch/8ch endpoints over multichannel companion pins (e.g. CABLE In 16 Ch).
  const virtOutputs = activeOutputs.filter(isVirtOut);
  const preferredVirtOutputs = [
    ...virtOutputs.filter((d) => !isMultichannelVirtualPin(d.name)),
    ...virtOutputs.filter((d) => isMultichannelVirtualPin(d.name)),
  ];
  for (const candidate of preferredVirtOutputs) {
    const paired = pairVirtualAppMic(candidate, activeInputs, activeOutputs);
    if (paired) {
      return {
        output: candidate,
        appMic: paired,
        ready: true,
        selectedIsVirtual: false,
        counterpartMissing: false,
        hasVirtualInput: true,
      };
    }
  }

  const fallbackOut = preferredVirtOutputs[0] ?? null;
  return {
    output: fallbackOut,
    appMic: null,
    ready: false,
    selectedIsVirtual: false,
    counterpartMissing: fallbackOut != null,
    hasVirtualInput,
  };
}

export function isSelfLoopReference(
  ref: ReferenceSource,
  selectedOutput: string,
  devices?: Pick<DeviceList, "inputs" | "outputs"> | null,
): boolean {
  if (ref.kind !== "input" && ref.kind !== "output") return false;
  if (!selectedOutput || selectedOutput === "default") return false;

  const bareOutput = stripReferencePrefix(selectedOutput);
  const bareRefStable = stripReferencePrefix(ref.stable_id);
  const bareRefSelector = stripReferencePrefix(ref.selector ?? ref.id);
  if (bareRefStable === bareOutput || bareRefSelector === bareOutput) {
    return true;
  }

  const outputs = devices?.outputs ?? [];
  const inputs = devices?.inputs ?? [];
  const outDev = outputs.find((d) => d.stable_id === bareOutput);
  if (!outDev) return false;

  if (normalizeName(ref.label) === normalizeName(outDev.name)) return true;

  if (ref.kind === "output") {
    const outPaired = pairVirtualAppMic(outDev, inputs, outputs);
    const refPaired = pairVirtualAppMic(
      { name: ref.label, stable_id: bareRefStable },
      inputs,
      outputs,
    );
    if (
      outPaired &&
      refPaired &&
      (outPaired.stable_id === refPaired.stable_id ||
        normalizeName(outPaired.name) === normalizeName(refPaired.name))
    ) {
      return true;
    }
  }

  if (ref.kind === "input") {
    const paired = pairVirtualAppMic(outDev, inputs, outputs);
    if (
      paired &&
      (bareRefStable === paired.stable_id ||
        bareRefSelector === paired.stable_id ||
        normalizeName(ref.label) === normalizeName(paired.name))
    ) {
      return true;
    }
    const directPaired = pairVirtualAppMic(
      outDev,
      [{ name: ref.label, stable_id: bareRefStable }],
      outputs,
    );
    if (directPaired != null) return true;
  }

  return false;
}
