# Frontend Agent Handoff

本文档给负责 Echoless GUI/Tauri 的前端 agent 使用。后端/整体改造计划见 `docs/frontend/FRONTEND_ADAPTATION_PLAN.md`。前端实现时必须保留现有 CLI 能力,不要把 GUI 做成唯一入口。

## 必读文件

- `README.md`
- `configs/example.toml`
- `docs/frontend/FRONTEND_ADAPTATION_PLAN.md`
- `docs/localvqe_inference.md`
- `docs/research/rtx_aec_runtime_distribution.md`
- `docs/research/sonora_aec3_internal_map.md`

## 当前产品判断

首版 GUI 默认 backend 是 `sonora_aec3`。推荐默认配置:

```toml
mic = "default"
reference = "system"
output = "default"
sample_rate = 48000
frame_ms = 10
reference_channels = "mono"

[[chain]]
kind = "sonora_aec3"
ns = false
agc = false
```

理由:

- AEC3 内部处理域是 48k / 10ms。
- Mac 本机试听中,48k 比 44.1k 更稳。
- 音质保真优先,默认不开 NS / AGC。
- LocalVQE 和 RTX AEC 都是 standalone 可选 backend,不是默认后级。

## 前端首版范围

### 必须做

- 设备选择:
  - mic
  - reference
  - output
- Backend 选择:
  - AEC3
  - LocalVQE experimental
  - RTX AEC Windows only,doctor 通过才启用
  - Passthrough diagnostic
- AEC3 基础参数:
  - reference channels: mono / stereo
  - NS: off / low / moderate / high / veryhigh
  - AGC: advanced only,默认 off
- 启动/停止实时处理。
- 显示实时电平:
  - mic dBFS
  - reference dBFS
  - output dBFS
- 显示状态:
  - estimated user latency
  - AEC estimated delay
  - input drops
  - stale drops
  - ref underruns
  - output underruns
  - runtime errors
- 诊断录制:
  - 30s
  - 45s
  - custom seconds
  - 显示 diagnostics session 路径
- 保存/加载配置。

### 不要在首版做

- 不要默认启用 `sonora_aec3 + localvqe` 级联。
- 不要默认启用 RTX AEC。
- 不要在普通界面展示所有 AEC3 内部 suppressor 参数。
- 不要用 stdout 人类文本作为长期数据源。
- 不要把 CLI 命令删除或重命名。

## Product Capability Map for UI/UX

Echoless 是一个本地实时 reference-based AEC 工具。GUI 的核心价值不是展示算法细节,而是让用户稳定完成这条链路:

```text
microphone + system/reference audio -> selected backend -> virtual microphone/output device
```

### 当前可设计成产品能力

| 能力 | 当前后端状态 | 前端建议 |
|---|---|---|
| Realtime AEC3 | 可用,主路径 | 首页默认启动项,标记 Recommended |
| Mic / reference / output routing | CLI 可选设备,JSON 可列设备 | Devices 页做三段式链路选择 |
| Reference mono / stereo | 可用 | Processing 页基础控制,默认 mono |
| Noise suppression | AEC3 内置 NS 可调 | 普通模式可放 off/low/moderate/high,默认 off |
| AGC | AEC3 内置 AGC 可开关 | 高级模式,默认 off |
| Runtime status | JSONL 可输出 | 首页实时状态、meter、badge、延迟 |
| User latency estimate | JSONL 已输出 | 首页显示 Estimated app latency |
| AEC alignment delay | JSONL 已输出 | 诊断/高级信息,不要当成用户延迟 |
| Diagnostics recording | 可写 mic/ref/out WAV + stats.csv + metadata | Diagnostics 页一键录制与 session 列表 |
| LocalVQE | 可 standalone,实验 | Processing 页 Experimental backend |
| RTX AEC | Windows-only,doctor/install/offline/realtime 已有 | Windows 且 doctor ok 才可选 |
| Passthrough | 可用 | 诊断模式,用于排查设备链路 |
| Offline processing | CLI 可用 | 首版可不做主 UI,可留高级入口 |
| Config validate | JSON 可用 | 保存/启动前调用,展示结构化错误 |

### 不应表现成已完成产品能力

| 能力 | 当前状态 | 前端处理 |
|---|---|---|
| 原生虚拟麦驱动 | 明确不做 | 提示用户选择 VB-Cable / BlackHole / Virtual Desktop Mic 等外部设备 |
| 原生 WASAPI/CoreAudio HAL | stub,实时 MVP 走 cpal | 不在 UI 承诺 native HAL;仅作为未来 I/O 优化 |
| 参数热更新 | 首版不支持 | 运行中变更设备/backend/采样率时提示需要重启 |
| 自动延迟校准向导 | 未实现独立向导 | 只显示 runtime status 中的估算值 |
| 自动安装 VB-Cable/BlackHole | 未实现 | 只做说明/链接占位,不要静默安装 |
| AEC3 + LocalVQE 默认级联 | 不推荐 | 不作为默认流程 |
| macOS RTX AEC | 不可用 | 禁用或隐藏 RTX backend |

## Primary User Flows

### 1. First Run Setup

目标: 用户第一次打开应用时能建立完整音频链路。

1. 调用 `echoless devices --json`。
2. 让用户选择 mic、reference、output。
3. 默认 backend 为 AEC3。
4. 默认配置为 48k / 10ms / mono reference / NS off / AGC off。
5. 调用 `echoless config validate --config ... --json`。
6. 允许 Start。

空态:

- 没有输入设备: 禁用 Start,显示刷新入口。
- 没有输出设备: 禁用 Start,提示选择/安装虚拟输出设备。
- reference 为 `none`: 允许启动,但状态应显示 No reference,告诉用户 AEC 不会真正消回声。

### 2. Daily Use

目标: 用户打开应用后用最少操作启动。

- 首页显示 Start / Stop。
- 首页显示当前 mic/reference/output 摘要。
- 首页显示 mic/ref/out 三个电平。
- 首页显示 Estimated app latency。
- 只有异常时突出显示 drops/runtime errors/high latency。
- 保留 Diagnostics quick action。

### 3. Tuning

目标: 用户音质或消回声效果不满意时能调整少量有效参数。

- 普通模式只暴露 reference mono/stereo、NS off/low/moderate/high。
- 高级模式再暴露 AGC、tail_ms、delay_num_filters、linear_stable_echo_path。
- 参数变更如果影响 runtime,提示需要重启后生效。
- 不要把所有 AEC3 内部 suppressor 参数铺出来。

### 4. Diagnostics

目标: 用户反馈断音、音量骤降、延迟或回声时能产出可交接证据。

- Diagnostics 页提供 30s / 45s / custom。
- 启动时把 diagnostics 写进 `PipelineConfig.diagnostics`。
- runtime status 里的 `diagnostics_session_dir` 出现后展示 session 路径。
- session 摘要优先显示 max output queue latency、input drops、stale drops、output underruns、runtime errors。

### 5. Backend Experiment

目标: 允许高级用户试听 LocalVQE 或 RTX AEC,但不影响默认 AEC3 保真路径。

- LocalVQE 标记 Experimental,需要模型和动态库路径。
- RTX AEC 只在 Windows 显示 doctor 结果;doctor 未通过时禁用并显示原因。
- Passthrough 放在诊断/高级模式。

## Screen-Level UX Requirements

### Main Screen

主屏应是运行控制台,不是 landing page。

- Primary action: Start / Stop。
- Secondary actions: Diagnostics, Open Processing。
- Always visible: backend、mic/ref/out 摘要、三个 level meter、estimated app latency。
- Status badge 优先级:
  - Runtime error
  - Dropping audio
  - High latency
  - No reference
  - Running
  - Ready
- 不要把算法论文名、内部参数解释放在主屏。

### Devices Screen

- 三个清晰区域: Microphone / Reference / Output。
- Reference 选项顺序: System audio, None, output devices, input devices。
- `devices --json` 返回空数组时显示空态,不要崩溃。
- macOS 上提示用户可能需要 BlackHole / Virtual Desktop Mic。
- Windows 上提示常见 output 是 VB-Cable Input。

### Processing Screen

- backend 用分段控制或紧凑 cards,不要做大面积营销卡片。
- AEC3 Recommended 默认展开。
- LocalVQE / RTX AEC 默认折叠为实验 backend。
- Advanced 区域折叠,并保持默认不改。

### Diagnostics Screen

- 重点不是画复杂图,而是让用户快速录证据。
- 显示最近 session 列表、session 路径、关键 counters。
- 提供打开目录动作。
- 诊断录制不是自动停止 realtime;录制达到上限后 realtime 仍可能继续运行。

## Error and Empty States

| 状态 | 触发 | UI 行为 |
|---|---|---|
| No devices | `inputs` 或 `outputs` 为空 | 禁用 Start,显示 Refresh |
| No reference | reference = `none` 或 ref 长期静音 | 允许运行,显示 warning |
| High latency | `estimated_user_latency_ms` 高于阈值 | 显示 warning,引导看 output queue |
| Dropping audio | drops/underruns 任一递增 | 显示 warning,建议诊断录制 |
| Runtime error | `runtime_errors > 0` 或 `last_backend_error` | 显示 error,允许 Stop |
| RTX unavailable | doctor 未通过 | 禁用 RTX card,显示 doctor detail |
| LocalVQE missing model | validate 返回 model 错误 | 禁用 Start 或提示选择模型 |
| Config invalid | validate 返回 errors | 显示字段级错误 |

建议阈值:

- `estimated_user_latency_ms < 80`: normal。
- `80 <= estimated_user_latency_ms < 150`: warning。
- `estimated_user_latency_ms >= 150`: high latency。
- `ref_dbfs <= -110` 持续数秒: reference is silent。

## Recommended Control Hierarchy

### Normal

- Backend: AEC3 / LocalVQE / RTX AEC / Passthrough。
- Mic / Reference / Output。
- Reference channels: mono / stereo。
- Noise suppression: off / low / moderate / high。
- Diagnostics duration。

### Advanced

- sample_rate。
- frame_ms。
- AGC。
- tail_ms。
- delay_num_filters。
- linear_stable_echo_path。
- LocalVQE model/library/threads/noise gate。
- RTX runtime/model/intensity/on runtime error。

### Hidden from First Version

- AEC3 suppressor internals。
- external delay estimator mode。
- ring buffer stale-drop thresholds。
- native HAL settings。
- native virtual mic / driver settings。

## Platform-Specific UX Rules

### Windows

- RTX AEC card can be visible, but only enabled after `echoless nvafx doctor --json` is ok。
- 常见 output 是 VB-Cable Input。
- RTX AEC 限制: 48000 Hz / 10ms / mono reference。

### macOS

- RTX AEC hidden or disabled。
- 用户可能需要 BlackHole / Virtual Desktop Mic / Aggregate Device。
- 首次实时启动可能触发麦克风权限。
- `devices --json` 可能在某些会话返回空数组,UI 必须有刷新/空态。

## Presets

首版可以把复杂参数包装成预设,避免用户误调。

| Preset | 参数意图 | 默认 |
|---|---|---|
| Voice Fidelity | AEC3, NS off, AGC off, mono reference | 是 |
| Echo Removal | AEC3, NS low/moderate,可提示可能压人声 | 否 |
| Diagnostic | Passthrough 或 AEC3 + diagnostics,显示更多 counters | 否 |

预设只应改变少量已暴露参数。不要让预设偷偷启用 AEC3 + LocalVQE 级联。

## UI 信息架构

### 1. Main

目标: 用户打开后直接能看到当前链路是否工作。

内容:

- Start / Stop
- Backend selector
- Mic level meter
- Reference level meter
- Output level meter
- Status badge:
  - Ready
  - Running
  - No reference
  - High latency
  - Dropping audio
  - Runtime error
- Estimated user latency
- Diagnostics quick action

### 2. Devices

内容:

- Mic device dropdown
- Reference source dropdown:
  - System audio
  - None
  - Input devices
  - Output devices
- Output device dropdown
- Sample rate display,默认 48000
- Frame size display,默认 10ms

说明:

- macOS 用户可能需要 BlackHole / Virtual Desktop Mic / 其他虚拟设备。
- Windows 用户常见输出是 VB-Cable Input。

### 3. Processing

内容:

- Backend cards:
  - AEC3 Recommended
  - LocalVQE Experimental
  - RTX AEC Windows RTX
  - Passthrough Diagnostic
- AEC3 controls:
  - reference mono/stereo
  - noise suppression selector
  - AGC toggle in advanced
  - tail_ms in advanced
- LocalVQE controls:
  - model path
  - library path
  - threads
  - noise gate
- RTX controls:
  - doctor status
  - runtime dir
  - model path
  - intensity ratio
  - on runtime error

### 4. Diagnostics

内容:

- Record diagnostics button
- Duration selector
- Latest session list
- Stats summary:
  - max output queue latency
  - input drops
  - stale drops
  - output underruns
  - runtime errors
- Open session directory

## Target Data Types

这些类型是前端应围绕的目标 contract。字段可随后端实现微调,但语义不要变。

```ts
export type ReferenceChannels = "mono" | "stereo";

export type ProcessorKind =
  | "passthrough"
  | "sonora_aec3"
  | "localvqe"
  | "nvidia_afx_aec";

export interface PipelineConfig {
  mic: string;
  reference: string;
  output: string;
  sample_rate: number;
  frame_ms: number;
  reference_channels: ReferenceChannels;
  diagnostics?: DiagnosticsConfig;
  chain: ChainNode[];
}

export interface DiagnosticsConfig {
  record_dir?: string;
  max_seconds?: number;
}

export interface ChainNode {
  kind: ProcessorKind;
  [param: string]: unknown;
}
```

```ts
export interface RuntimeStatus {
  type: "status";
  elapsed_s: number;
  frames: number;
  sample_rate: number;
  frame_ms: number;
  backend: string;
  mic_dbfs: number;
  ref_dbfs: number;
  out_dbfs: number;
  mic_q_samples: number;
  ref_q_samples: number;
  out_q_samples: number;
  output_queue_latency_ms: number;
  algorithmic_latency_ms: number;
  estimated_user_latency_ms: number;
  aec_estimated_delay_ms: number;
  mic_input_drops: number;
  ref_input_drops: number;
  input_drops: number;
  ref_underruns: number;
  output_underruns: number;
  output_overruns: number;
  stale_drops: number;
  node_process_time_ms: number;
  runtime_errors: number;
  diverged: boolean;
  last_backend_error?: string | null;
  diagnostics_session_dir?: string | null;
}
```

延迟计算:

```ts
export function estimateUserLatencyMs(status: {
  frame_ms: number;
  sample_rate: number;
  out_q_samples: number;
  algorithmic_latency_ms?: number;
}) {
  return (
    status.frame_ms / 2 +
    (status.algorithmic_latency_ms ?? 0) +
    (status.out_q_samples / status.sample_rate) * 1000
  );
}
```

注意:

- `aec_estimated_delay_ms` 是算法估计的回声对齐延迟。
- `estimated_user_latency_ms` 是用户说话到虚拟麦输出的估算延迟。
- 两者不能混用。

## 当前 CLI 可用命令

这些命令现在就可用于人工调试:

```bash
echoless devices
echoless processors
echoless run --config configs/example.toml
echoless run --config configs/example.toml --verbose --stats-interval-ms 1000
echoless run --config configs/example.toml --diagnostic-dir diagnostics/aec3 --diagnostic-seconds 45 --verbose
echoless offline --mic m.wav --reference r.wav --out o.wav --chain sonora_aec3
echoless nvafx doctor
echoless nvafx doctor --json
```

这些 JSON 命令当前已经可直接用于前端 sidecar adapter:

```bash
echoless devices --json
echoless processors --json
echoless run --config config.toml --status-json
echoless config validate --config config.toml --json
```

集成规则:

- `run --status-json` 的 stdout 是 JSONL status event;启动提示、设备摘要、诊断路径等人类文本走 stderr。
- `run --status-json` 默认 1000ms 一条 status;可以用 `--stats-interval-ms` 调整。
- `config validate --json` 即使配置无效也会先在 stdout 输出 `{ ok, errors }`,然后用非 0 exit code 表达失败。
- `devices --json` 在某些 macOS 会话可能返回空设备数组,前端要显示空态和刷新按钮。
- 不要把现有中文 stdout 文本当作稳定 API。

## Backend Visibility Rules

### AEC3

显示条件:

- 所有平台默认显示。

默认:

- sample rate: 48000
- frame: 10ms
- reference: mono
- NS: off
- AGC: off

### LocalVQE

显示条件:

- artifact 内存在模型和动态库,或用户手动选择路径。

标记:

- Experimental

说明:

- 16k mono 推理。
- `algorithmic_latency_ms` 约 16ms。
- 不默认级联 AEC3。

### RTX AEC

显示条件:

- Windows。
- `nvafx doctor` 通过。

禁用条件:

- macOS。
- 没有 RTX runtime。
- doctor 报 runtime / GPU / driver 不可用。

说明:

- 只支持 48000 Hz。
- 只支持 10ms frame。
- 只支持 mono reference。
- 作为独立 backend 测试,不默认和 AEC3 级联。

### Passthrough

显示条件:

- 高级/诊断模式。

用途:

- 排查虚拟麦、设备链路、延迟和 drop 是否来自 AEC backend。

## CLI Backend Readiness Assessment

这是面向前端开工的当前完成度判断。

### Overall

CLI sidecar 后端已经达到 **MVP frontend-ready**:

- 可以列设备。
- 可以列 backend manifest。
- 可以校验配置。
- 可以启动/停止实时进程。
- 可以输出 JSONL runtime status。
- 可以输出 diagnostics 证据。
- 可以继续保留人工 CLI 调试路径。

建议把当前后端视为 **可开始 Tauri 前端实现,但不是最终产品后端**。

### Completion Matrix

| 模块 | 完成度 | 前端可接入程度 | 说明 |
|---|---:|---|---|
| CLI text commands | 90% | 可用 | `devices/processors/run/offline/nvafx` 保留 |
| JSON devices | 75% | 可接 | schema 可用;设备为空态需前端处理 |
| JSON processor manifest | 80% | 可接 | 参数 manifest 可渲染 UI;后续可继续细化 labels/help |
| Config validate JSON | 75% | 可接 | 结构校验可用;不加载模型/driver 做重型校验 |
| Realtime AEC3 | 80% | 可接 | 主路径可用;真实设备体验仍要继续调参 |
| Runtime JSONL status | 80% | 可接 | 电平、延迟、drop、runtime error 已输出 |
| Diagnostics recording | 85% | 可接 | WAV/stats/metadata 可用;session 浏览由前端做 |
| LocalVQE backend | 60% | 实验可接 | standalone 可用;音质和延迟不作为默认承诺 |
| RTX AEC backend | 70% | Windows 可接 | doctor/install/offline/realtime 已有;授权/分发仍需谨慎 |
| Config save/load | 60% | 前端自行实现 | 后端有 config schema 和 validate;没有专门 save command |
| Process lifecycle API | 60% | sidecar 可实现 | 首版用 spawn/kill;还没有 daemon `start/stop` RPC |
| Hot parameter updates | 20% | 不接 | 首版参数变化重启 runtime |
| Native virtual mic | 0% | 不接 | 明确不做;依赖外部虚拟设备 |
| Native platform HAL | 20% | 不接 | crate 存在但 stub;实时 MVP 走 cpal;不是虚拟麦 |

### Ready for Frontend Now

- 设备页: `echoless devices --json`。
- Backend/Processing 页: `echoless processors --json`。
- 配置保存前校验: `echoless config validate --config <file> --json`。
- 运行页: spawn `echoless run --config <file> --status-json`。
- Stop: 对 sidecar 发 graceful stop,超时后 kill。
- Diagnostics: 在 config 中写入 `[diagnostics]`,然后从 status 读取 `diagnostics_session_dir`。

### Known Backend Gaps for UI Planning

- 没有长期驻留 daemon;首版按 sidecar child process 管理。
- 没有专门的 JSON command channel;配置通过文件传入。
- 没有配置热更新;变更后重启。
- 没有统一 settings store;前端自己管理用户配置文件。
- 原生虚拟麦安装/创建能力不在路线图内;前端只管理外部虚拟设备选择。
- 没有 built-in device permission assistant。
- 没有自动读取 diagnostics 历史 session 的 JSON API;前端可先用文件系统扫描或只显示当前 session。
- `devices --json` 的设备 id 目前是索引字符串,跨重启不保证稳定;保存配置时更稳的是保存用户可识别名称和最近选择。

### Native HAL / Virtual Mic Boundary

- Native virtual mic is out of scope. Do not design install, create, driver health, or driver update flows for an in-project virtual microphone.
- Native HAL means platform audio I/O replacement for `cpal`: Windows WASAPI capture/loopback/QPC/MMCSS/device recovery, or macOS CoreAudio AUHAL/Process Tap/mach time.
- GUI MVP should not surface native HAL as a user-facing mode. Treat it as an internal future optimization only if diagnostics prove `cpal` is the bottleneck.
- The durable scope note is `docs/architecture/native_hal_scope.md`.

### Recommended Frontend Adapter Shape

前端可以把后端封成四个 adapter:

```ts
interface EcholessBackend {
  listDevices(): Promise<DeviceManifest>;
  listProcessors(): Promise<ProcessorManifest>;
  validateConfig(configPath: string): Promise<ConfigValidationResult>;
  start(configPath: string): AsyncIterable<RuntimeStatus>;
  stop(): Promise<void>;
}
```

实现建议:

- `listDevices/listProcessors/validateConfig` 用一次性 command。
- `start` spawn 长运行 sidecar,读取 stdout JSONL。
- stderr 作为 human log 面板输入,不要当状态源。
- 运行中修改配置时: stop -> rewrite config -> validate -> start。

## Frontend State Machine

建议状态:

```ts
export type RuntimeState =
  | "idle"
  | "starting"
  | "running"
  | "stopping"
  | "error";
```

行为:

- `idle -> starting`: spawn sidecar / call start。
- `starting -> running`: 收到第一条 status event。
- `running -> stopping`: 用户 stop。
- `stopping -> idle`: sidecar 退出。
- `any -> error`: 启动失败、设备丢失、runtime error 不可恢复。

设备或 backend 变更:

- 首版直接提示需要重启 runtime。
- 停止后应用新配置。

## UX Copy Guidance

普通用户文案要避免把内部概念混在一起:

- "Estimated app latency" 用于 `estimated_user_latency_ms`。
- "AEC alignment delay" 用于 `aec_estimated_delay_ms`。
- "Audio drop detected" 用于 `input_drops/stale_drops/output_underruns > 0`。
- "Reference is silent" 用于 ref 电平长期接近 -120dB。

不要把 `10ms frame` 写成总延迟。它只是处理粒度。

## Test Checklist

前端完成后至少覆盖:

- macOS:
  - AEC3 48k / mono / NS off 可启动。
  - 设备列表可刷新。
  - runtime status 能持续更新。
  - diagnostics 生成 session。
  - RTX AEC 禁用。
- Windows:
  - AEC3 可启动。
  - VB-Cable / CABLE Input 可选。
  - `nvafx doctor` 通过时 RTX AEC 可选。
  - `nvafx doctor` 失败时 RTX AEC 显示原因。
- Cross-platform:
  - 保存配置后重新打开仍能加载。
  - Stop 后没有残留 running 状态。
  - backend 切换不会生成无效配置。
  - `echoless run --config configs/example.toml` CLI 仍可用。

## Do Not Change Without Backend Coordination

- `PipelineConfig` 字段语义。
- `NodeConfig.kind` 名称。
- `reference` 字符串约定: `system` / `none` / `input:<name>` / `output:<name>`。
- `reference_channels` 值: `mono` / `stereo`。
- `nvidia_afx_aec` 约束: Windows + 48k + 10ms + mono reference。
- CLI 命令名和现有 flags。
