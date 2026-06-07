// 前端 ↔ Tauri 后端的调用层。所有数据都走 JSON 契约;不解析人类日志。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DeviceList,
  DoctorAudio,
  Platform,
  ProcessorManifest,
  RunEvent,
  ValidateResult,
} from "./types";

export function getPlatform(): Promise<Platform> {
  return invoke<Platform>("get_platform");
}

export function listDevices(): Promise<DeviceList> {
  return invoke<DeviceList>("list_devices");
}

export function listProcessors(): Promise<ProcessorManifest> {
  return invoke<ProcessorManifest>("list_processors");
}

export function doctorAudio(): Promise<DoctorAudio> {
  return invoke<DoctorAudio>("doctor_audio");
}

export function validateConfig(tomlText: string): Promise<ValidateResult> {
  return invoke<ValidateResult>("validate_config", { tomlText });
}

export function startRun(
  tomlText: string,
  statsIntervalMs = 80,
): Promise<void> {
  return invoke<void>("start_run", { tomlText, statsIntervalMs });
}

export function stopRun(): Promise<void> {
  return invoke<void>("stop_run");
}

// 订阅 run 的事件流(started + status 都走这个通道)。返回取消订阅函数。
export function onRunEvent(cb: (e: RunEvent) => void): Promise<UnlistenFn> {
  return listen<RunEvent>("echoless://status", (e) => cb(e.payload));
}
export function onRunExit(cb: () => void): Promise<UnlistenFn> {
  return listen("echoless://exit", () => cb());
}
export function onRunLog(cb: (line: string) => void): Promise<UnlistenFn> {
  return listen<string>("echoless://log", (e) => cb(e.payload));
}

// ---- 配置生成:把 UI 选择拼成后端 PipelineConfig(TOML) ----
export interface PipelineCfg {
  sample_rate: number;
  frame_ms: number;
  reference_channels: "mono" | "stereo";
}
export interface ConfigChoice {
  mic: string; // selector / stable_id / "default"
  output: string;
  reference: string; // "system" | "none" | "input:<stable_id>" | ...
  kind: string; // backend kind
  pipeline: PipelineCfg;
  params: Record<string, unknown>; // chain[0] 参数(不含 reference_channels)
}

function tomlString(v: string): string {
  return `"${v.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function tomlValue(v: unknown): string | null {
  if (v === null || v === undefined || v === "") return null;
  if (typeof v === "boolean") return v ? "true" : "false";
  if (typeof v === "number") return Number.isFinite(v) ? String(v) : null;
  return tomlString(String(v));
}

export function buildConfigToml(c: ConfigChoice): string {
  const lines = [
    `mic = ${tomlString(c.mic)}`,
    `reference = ${tomlString(c.reference)}`,
    `output = ${tomlString(c.output)}`,
    `sample_rate = ${c.pipeline.sample_rate}`,
    `frame_ms = ${c.pipeline.frame_ms}`,
    `reference_channels = ${tomlString(c.pipeline.reference_channels)}`,
    ``,
    `[[chain]]`,
    `kind = ${tomlString(c.kind)}`,
  ];
  for (const [k, raw] of Object.entries(c.params)) {
    if (k === "reference_channels") continue; // 顶层管线项,不重复
    const val = tomlValue(raw);
    if (val !== null) lines.push(`${k} = ${val}`);
  }
  return lines.join("\n") + "\n";
}
