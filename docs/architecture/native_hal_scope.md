# Native HAL Scope

本文固定 Echoless 对 native HAL 与虚拟麦克风的当前产品边界。

## 产品决策

- 原生虚拟麦克风驱动不做。Windows/macOS 输出长期依赖外部虚拟音频设备,例如 VB-Cable、BlackHole、Virtual Desktop Mic 或用户指定的等价设备。
- native HAL 不是虚拟麦克风。它只表示平台原生音频 I/O 抽象层,用于替换或增强当前 `cpal` 实时路径。
- GUI MVP 不依赖 native HAL。首版前端继续通过 CLI sidecar 调用 `devices`、`config validate`、`run --status-json` 和 diagnostics。

## Native HAL 是什么

HAL 是 Hardware Abstraction Layer。Echoless 里的 HAL 对应这些 trait:

- `AudioSource`: 麦克风输入和 far-end reference 输入。
- `AudioSink`: 处理后音频输出到用户选择的外部虚拟设备或试听输出。
- `MonotonicClock`: 平台单调时钟,用于对齐、延迟估算和诊断。

平台 crate 的预期职责:

- Windows: WASAPI capture、WASAPI loopback、QPC timestamp、MMCSS、device invalidation/recovery。
- macOS: CoreAudio AUHAL mic capture、Process Tap system audio capture、mach host time、TCC/设备恢复。

它不包括:

- Windows WaveRT/SysVAD/simpleaudiosample 自研虚拟麦驱动。
- macOS AudioServerPlugin 自研虚拟麦。
- 自动安装第三方虚拟设备。

## 当前实现位置

当前实时音频路径在 `echoless-cli/src/realtime.rs`:

```text
cpal mic stream + cpal reference stream + cpal output stream
        -> ringbuf
        -> ProcessorChain
        -> selected output device
```

`echoless-hal-win` 和 `echoless-hal-mac` 目前是 stub,主要保留边界和未来实现点。它们不是当前 runtime 的阻塞项。

## 什么时候值得做

只有出现可测量的 `cpal` 瓶颈时才值得做 native HAL。触发条件应来自 diagnostics 或人工测试:

- Windows loopback/reference 在目标设备上无法可靠采集。
- macOS system audio 需要 Process Tap 才能获得可接受的 reference,而 BlackHole/外部路由不可接受。
- 输入/参考 timestamp 不足以稳定估算回声延迟,导致 AEC3 双讲或音量稳定性明显变差。
- device change、sleep/wake、默认设备切换恢复不稳定。
- 端到端延迟/抖动主要来自 I/O 层,且无法通过 `frame_ms`、queue、buffer 参数改善。
- 需要更完整的 WASAPI/CoreAudio 错误码、stream format、buffer size 诊断。

## 不建议现在做的原因

- 目前最重要的产品风险是 AEC3 保真、设备路由、diagnostics、前端控制面和 Windows/macOS 真机调参。
- native HAL 会引入大量平台维护成本:COM/WASAPI/MMCSS/device notification、CoreAudio Process Tap/TCC/aggregate-device 行为、跨平台测试矩阵。
- 原生虚拟麦已经退出路线图,因此不能用“顺便做驱动”来摊薄 native HAL 的成本。

## 推荐路线

短期:

- 保持 `cpal` 实时路径。
- 前端只展示外部虚拟设备选择,不承诺 native driver 或 native HAL。
- 用 diagnostics 记录 mic/ref/out/stats,定位断音、延迟、音量骤降是否来自处理器、队列还是设备 I/O。

中期如需推进:

- 只做窄范围 native HAL,优先从 reference capture 和 timestamp 开始。
- Windows 优先 WASAPI loopback/capture + QPC/MMCSS/device recovery。
- macOS 优先 Process Tap reference capture,仍输出到 BlackHole/用户选择的外部设备。
- 不实现自研虚拟麦驱动。
