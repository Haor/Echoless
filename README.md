# echoless — 跨平台实时 AEC 工具(cargo workspace 骨架)

面向 **Windows 10/11 + macOS 14.4+** 的本地自用 reference-based AEC 工具。
设计蓝本:`../research/cross_platform_architecture.md`。

> **当前是骨架**:架构/trait/链路/配置/CLI 全部就位且**可编译**,离线链路**已能跑通**(stub 处理器先直通)。
> sonora / LocalVQE 的真实算法、平台 HAL(WASAPI/CoreAudio)、原生虚拟麦为标注好的 TODO。

## crate 结构

| crate | 职责 | 状态 |
|---|---|---|
| `echoless-hal` | 平台无关 trait(`AudioSource`/`AudioSink`/`MonotonicClock`)+ 类型 + 文件/null 后端 | ✅ |
| `echoless-hal-win` | Windows HAL(WASAPI/WaveRT/QPC) | stub(TODO) |
| `echoless-hal-mac` | macOS HAL(CoreAudio/Process Tap/AudioServerPlugin/mach) | stub(TODO) |
| `echoless-processors` | `EchoProcessor` trait + `ProcessorChain` + sonora/localvqe 节点 | ✅ 框架 / 算法 TODO |
| `echoless-core` | 管线编排 + `PipelineConfig` + `ControlApi` + `run_offline` | ✅ 离线 / 实时 TODO |
| `echoless-cli` | CLI 前端(`aec`):`offline` 可用,`devices`/`run` TODO | ✅ |

依赖单向:`echoless-cli/daemon → 平台HAL → echoless-hal`;`echoless-cli → echoless-core → echoless-processors`。**核心永不依赖平台 crate;前端只经 `ControlApi`。**

## 核心设计:统一可组合处理器

sonora 经典 AEC3 与 LocalVQE 都是平级 `EchoProcessor` 节点,**可单开 / 串联 / 自由组合 / 扩展**:
- 单开经典:`--chain sonora_aec3`
- 单开 LocalVQE:`--chain localvqe`
- 串联:`--chain sonora_aec3,localvqe`
- 加新方案 = 在 `echoless-processors` 写一个 `impl EchoProcessor` + 在 `registry` 登记一行,其余不动。

`ProcessorChain` 自动处理节点间采样率/声道适配(sonora 48k/stereo-ref ↔ LocalVQE 16k/mono)与 far ref 分发(每级都拿真实 ref)。

## 构建与试跑

```bash
cd aec
cargo build

# 离线跑链(用 tools/echoless-recorder 录的 WAV;stub 阶段输出≈输入,验证链路通)
cargo run -p echoless-cli --bin echoless -- offline \
    --mic takes/doubletalk_01.mic.wav \
    --reference takes/doubletalk_01.ref.wav \
    --out out.wav \
    --chain "sonora_aec3,localvqe"

# 或用配置文件
cargo run -p echoless-cli --bin echoless -- offline --mic m.wav --reference r.wav --out o.wav --config configs/example.toml

# 列出处理器种类
cargo run -p echoless-cli --bin echoless -- processors

# 实时(当前会在平台 HAL 处报「未实现」,精确指向待办)
cargo run -p echoless-cli --bin echoless -- run --config configs/example.toml
```

## 下一步(给算法/平台填空)

1. `echoless-processors/sonora_aec3.rs` → 接 sonora APM/AEC3(analyze_render + process_capture)。
2. `echoless-processors/localvqe.rs` → FFI 接 liblocalvqe(`localvqe_process_frame_f32`)。
3. `echoless-processors/chain.rs` → 占位线性 SRC 换成 rubato 有状态 SRC + 立体声保留。
4. `echoless-hal-win` / `echoless-hal-mac` → 实现采集/loopback(Process Tap)/虚拟麦;参考 `../tools/echoless-recorder`。
5. `echoless-core::run_realtime` → 线程化实时管线(SPSC ring + 对齐 + drift);加 `ControlApi` 实体。
6. `echoless-daemon`(新 crate)+ Electron 前端(蓝本 §14)。
