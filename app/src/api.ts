// 前端 ↔ Tauri 后端的调用层。所有数据都走 JSON 契约;不解析人类日志。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DeviceList,
  DoctorAudio,
  NvafxDoctor,
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

export function nvafxDoctor(runtimeDir?: string): Promise<NvafxDoctor> {
  return invoke<NvafxDoctor>("nvafx_doctor", { runtimeDir: runtimeDir ?? null });
}

// RTX AEC runtime 安装:解压 common + 按架构 model zip,回传安装后的 doctor 报告。
export function nvafxInstall(p: {
  commonZip: string;
  modelZip: string;
  runtimeDir?: string;
}): Promise<NvafxDoctor> {
  return invoke<NvafxDoctor>("nvafx_install", {
    commonZip: p.commonZip,
    modelZip: p.modelZip,
    runtimeDir: p.runtimeDir ?? null,
  });
}

// 从公共 GitHub release 下载 + 安装(后端按 GPU 架构自动选模型)。回传安装后 doctor。
export function nvafxDownloadInstall(p: {
  runtimeDir?: string;
}): Promise<NvafxDoctor> {
  return invoke<NvafxDoctor>("nvafx_download_install", {
    runtimeDir: p.runtimeDir ?? null,
  });
}

export function openUrl(url: string): Promise<void> {
  return invoke<void>("open_url", { url });
}

export function defaultDiagDir(): Promise<string> {
  return invoke<string>("default_diag_dir");
}

export function openPath(path: string): Promise<void> {
  return invoke<void>("open_path", { path });
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
export interface DiagnosticsCfg {
  record_dir: string;
  max_seconds: number | null;
}
export interface ConfigChoice {
  mic: string; // selector / stable_id / "default"
  output: string;
  reference: string; // "system" | "none" | "input:<stable_id>" | ...
  kind: string; // backend kind
  pipeline: PipelineCfg;
  params: Record<string, unknown>; // chain[0] 参数(不含 reference_channels)
  diagnostics?: DiagnosticsCfg | null; // 开启录制时写入 [diagnostics]
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
  ];
  if (c.diagnostics) {
    lines.push(`[diagnostics]`);
    lines.push(`record_dir = ${tomlString(c.diagnostics.record_dir)}`);
    if (c.diagnostics.max_seconds != null)
      lines.push(`max_seconds = ${c.diagnostics.max_seconds}`);
    lines.push(``);
  }
  lines.push(`[[chain]]`, `kind = ${tomlString(c.kind)}`);
  for (const [k, raw] of Object.entries(c.params)) {
    if (k === "reference_channels") continue; // 顶层管线项,不重复
    const val = tomlValue(raw);
    if (val !== null) lines.push(`${k} = ${val}`);
  }
  return lines.join("\n") + "\n";
}
