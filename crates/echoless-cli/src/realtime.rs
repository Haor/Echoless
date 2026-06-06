//! cpal 实时管线。结构移植自上游 sonora-aec(BSD-3),处理换成 echoless 的 `ProcessorChain`。
//!
//! 三股 cpal 流 + 三个 ringbuf + 一个独立处理线程:
//! ```text
//! mic 设备 ──► mic_ring ──┐
//!                         ├─► 处理线程(每 10ms)─ chain.process(near, far) ─► out_ring ──► 输出设备
//! 系统 loopback ─► render_ring┘
//! ```
//! 全程 mono、同采样率 → 链上零重采样(rubato 仅 LocalVQE 进来才需要)。
//! 跨平台靠 cpal:Windows WASAPI(含 output loopback)/ macOS CoreAudio。
//! 系统声音参考 = output 设备做 loopback(Windows 原生;macOS 需 BlackHole 之类)。
//! 虚拟麦输出 = 选 VB-Cable / BlackHole 作 output 设备。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    Device, DeviceDescription, FromSample, Sample, SampleFormat, SizedSample, Stream,
    SupportedStreamConfig, SupportedStreamConfigRange,
};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

use echoless_core::{apply_reference_channels_to_chain, PipelineConfig, ReferenceChannels};
use echoless_processors::{chain_from_nodes, ProcessorChain};

#[derive(Clone, Copy)]
enum DeviceKind {
    Input,
    Output,
}
impl DeviceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputChannelMode {
    MonoDownmix,
    PreserveFirst(usize),
}

impl InputChannelMode {
    fn from_reference_channels(mode: ReferenceChannels) -> Self {
        match mode {
            ReferenceChannels::Mono => Self::MonoDownmix,
            ReferenceChannels::Stereo => Self::PreserveFirst(2),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeOptions {
    /// None = quiet;Some(ms) = 每隔 ms 打印一行滚动状态。
    pub stats_interval_ms: Option<u64>,
}

struct SelectedDevice {
    index: Option<usize>,
    device: Device,
}

#[derive(Clone)]
struct RealtimeCounters {
    mic_input_drops: Arc<AtomicU64>,
    ref_input_drops: Arc<AtomicU64>,
    output_underruns: Arc<AtomicU64>,
}

impl RealtimeCounters {
    fn new() -> Self {
        Self {
            mic_input_drops: Arc::new(AtomicU64::new(0)),
            ref_input_drops: Arc::new(AtomicU64::new(0)),
            output_underruns: Arc::new(AtomicU64::new(0)),
        }
    }
}

struct ProcessRuntime {
    frame_size: usize,
    reference_channels: usize,
    counters: RealtimeCounters,
    stats_interval: Option<Duration>,
}

pub fn run_with_options(cfg: &PipelineConfig, options: RuntimeOptions) -> Result<()> {
    let host = cpal::default_host();

    let mic_device = select_device(&host, DeviceKind::Input, mic_selector(&cfg.mic))
        .context("选择麦克风设备失败")?;
    let output_device = select_device(&host, DeviceKind::Output, output_selector(&cfg.output))
        .context("选择输出设备失败")?;
    // reference:"none" = 无参考(纯 NS);"system" = 默认输出做 loopback;否则按名。
    let render_device = match cfg.reference.as_str() {
        "none" | "" => None,
        "system" | "default" => Some((
            select_default_device(&host, DeviceKind::Output)
                .context("无默认输出设备可作系统 loopback")?,
            DeviceKind::Output,
        )),
        sel => Some(select_render_device(&host, sel).context("选择参考设备失败")?),
    };

    let sample_rate = cfg.sample_rate;
    if cfg.frame_ms == 0 {
        bail!("帧长必须大于 0ms");
    }
    let frame_samples = sample_rate as u64 * cfg.frame_ms as u64;
    if !frame_samples.is_multiple_of(1000) {
        bail!(
            "采样率与帧长必须产生整数样本: sample_rate={sample_rate}, frame_ms={}",
            cfg.frame_ms
        );
    }
    let frame_size = (frame_samples / 1000) as usize;
    let ring_size = frame_size * 12; // ~120ms
    let reference_channels = if render_device.is_some() {
        usize::from(cfg.reference_channels.channel_count())
    } else {
        1
    };

    let mic_config = pick_config(&mic_device.device, DeviceKind::Input, sample_rate)
        .context("麦克风不支持该采样率")?;
    let output_config = pick_config(&output_device.device, DeviceKind::Output, sample_rate)
        .context("输出设备不支持该采样率")?;
    let render_config = match &render_device {
        Some((d, k)) => {
            Some(pick_config(&d.device, *k, sample_rate).context("参考设备不支持该采样率")?)
        }
        None => None,
    };
    if cfg.reference_channels == ReferenceChannels::Stereo
        && render_config.as_ref().is_some_and(|c| c.channels() < 2)
    {
        bail!("reference_channels=stereo 需要参考设备至少 2ch");
    }

    println!(
        "Mic:    {} ({})",
        selected_device_label(&mic_device),
        config_summary(&mic_config)
    );
    match (&render_device, &render_config) {
        (Some((d, k)), Some(c)) => {
            println!(
                "Ref:    {} {} ({})",
                k.label(),
                selected_device_label(d),
                config_summary(c)
            )
        }
        _ => println!("Ref:    无 —— AEC 缺少参考,仅 NS 等单端处理有效"),
    }
    println!(
        "Output: {} ({})",
        selected_device_label(&output_device),
        config_summary(&output_config)
    );

    let chain_desc = if cfg.chain.is_empty() {
        "直通".to_string()
    } else {
        cfg.chain
            .iter()
            .map(|n| n.kind.clone())
            .collect::<Vec<_>>()
            .join(" → ")
    };
    println!(
        "采样率 {sample_rate} Hz · 帧 {} ms / {frame_size} 样本 · reference={} · 链: {chain_desc}",
        cfg.frame_ms,
        cfg.reference_channels.as_str()
    );

    let running = Arc::new(AtomicBool::new(true));
    ctrlc::set_handler({
        let running = running.clone();
        move || running.store(false, Ordering::SeqCst)
    })?;

    let counters = RealtimeCounters::new();

    let (mic_prod, mic_cons) = HeapRb::<f32>::new(ring_size).split();
    let (out_prod, out_cons) = HeapRb::<f32>::new(ring_size).split();
    let (render_prod, render_cons) = if render_device.is_some() {
        let (p, c) = HeapRb::<f32>::new(ring_size * reference_channels).split();
        (Some(p), Some(c))
    } else {
        (None, None)
    };

    let mic_stream = build_input_stream(
        &mic_device.device,
        &mic_config,
        mic_prod,
        "mic",
        InputChannelMode::MonoDownmix,
        counters.mic_input_drops.clone(),
    )?;
    let render_stream = match (render_device.as_ref(), render_config.as_ref(), render_prod) {
        (Some((d, _)), Some(c), Some(p)) => Some(build_input_stream(
            &d.device,
            c,
            p,
            "ref",
            InputChannelMode::from_reference_channels(cfg.reference_channels),
            counters.ref_input_drops.clone(),
        )?),
        _ => None,
    };
    let output_stream = build_output_stream(
        &output_device.device,
        &output_config,
        out_cons,
        counters.output_underruns.clone(),
    )?;

    // 处理线程:只碰 ring(Send),cpal Stream 留在本线程(!Send)。
    let proc_running = running.clone();
    let mut chain_cfg = cfg.chain.clone();
    apply_reference_channels_to_chain(&mut chain_cfg, cfg.reference_channels);
    let chain = chain_from_nodes(&chain_cfg, sample_rate, reference_channels as u16)?;
    let stats_interval = options.stats_interval_ms.map(Duration::from_millis);
    let runtime = ProcessRuntime {
        frame_size,
        reference_channels,
        counters,
        stats_interval,
    };
    let proc = thread::spawn(move || {
        process_loop(
            proc_running,
            chain,
            mic_cons,
            render_cons,
            out_prod,
            runtime,
        );
    });

    mic_stream.play()?;
    if let Some(s) = &render_stream {
        s.play()?;
    }
    output_stream.play()?;

    println!("运行中。macOS 首次需授予麦克风权限。Ctrl+C 停止。");
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }

    drop(mic_stream);
    drop(render_stream);
    drop(output_stream);
    proc.join().ok();
    println!("已停止。");
    Ok(())
}

fn process_loop<M, R, O>(
    running: Arc<AtomicBool>,
    mut chain: ProcessorChain,
    mut mic_cons: M,
    mut render_cons: Option<R>,
    mut out_prod: O,
    runtime: ProcessRuntime,
) where
    M: Consumer<Item = f32>,
    R: Consumer<Item = f32>,
    O: Producer<Item = f32>,
{
    let frame_size = runtime.frame_size;
    let far_samples_per_frame = frame_size * runtime.reference_channels;
    let mut near = vec![0.0f32; frame_size];
    let mut far = vec![0.0f32; far_samples_per_frame];
    let mut out = vec![0.0f32; frame_size];
    let mut stats = runtime.stats_interval.map(RealtimeStats::new);

    while running.load(Ordering::SeqCst) {
        if mic_cons.occupied_len() < frame_size {
            thread::sleep(Duration::from_millis(1));
            continue;
        }
        // 控制积压(简单 drift/堆积处理):超 4 帧丢旧的。
        let mut stale_drops = skip_stale(&mut mic_cons, frame_size);
        mic_cons.pop_slice(&mut near);

        let mut ref_underrun = 0;
        if let Some(rc) = render_cons.as_mut() {
            stale_drops += skip_stale(rc, far_samples_per_frame);
            if rc.occupied_len() >= far_samples_per_frame {
                rc.pop_slice(&mut far);
            } else {
                far.fill(0.0); // 参考欠载 → 填静音
                ref_underrun = 1;
            }
        } else {
            far.fill(0.0);
        }

        chain.process(&near, &far, &mut out, frame_size as u32);
        let pushed = out_prod.push_slice(&out);
        let output_overruns = out.len().saturating_sub(pushed) as u64;

        if let Some(stats) = stats.as_mut() {
            let ref_q = render_cons
                .as_ref()
                .map(|rc| rc.occupied_len())
                .unwrap_or(0);
            stats.observe(StatsSample {
                frame_size,
                near: &near,
                far: &far,
                out: &out,
                mic_q: mic_cons.occupied_len(),
                ref_q,
                out_q: out_prod.occupied_len(),
                mic_input_drops: runtime.counters.mic_input_drops.swap(0, Ordering::Relaxed),
                ref_input_drops: runtime.counters.ref_input_drops.swap(0, Ordering::Relaxed),
                stale_drops: stale_drops as u64,
                ref_underruns: ref_underrun,
                output_overruns,
                output_underruns: runtime.counters.output_underruns.swap(0, Ordering::Relaxed),
            });
        }
    }
}

fn skip_stale<C: Consumer<Item = f32>>(consumer: &mut C, frame_size: usize) -> usize {
    let max_queued = frame_size * 4;
    let queued = consumer.occupied_len();
    if queued > max_queued {
        let dropped = queued - max_queued;
        consumer.skip(dropped);
        dropped
    } else {
        0
    }
}

struct StatsSample<'a> {
    frame_size: usize,
    near: &'a [f32],
    far: &'a [f32],
    out: &'a [f32],
    mic_q: usize,
    ref_q: usize,
    out_q: usize,
    mic_input_drops: u64,
    ref_input_drops: u64,
    stale_drops: u64,
    ref_underruns: u64,
    output_overruns: u64,
    output_underruns: u64,
}

struct RealtimeStats {
    interval: Duration,
    started: Instant,
    last_print: Instant,
    total_frames: u64,
    near_samples: u64,
    far_samples: u64,
    out_samples: u64,
    near_sq: f64,
    far_sq: f64,
    out_sq: f64,
    mic_q: usize,
    ref_q: usize,
    out_q: usize,
    mic_input_drops: u64,
    ref_input_drops: u64,
    stale_drops: u64,
    ref_underruns: u64,
    output_overruns: u64,
    output_underruns: u64,
}

impl RealtimeStats {
    fn new(interval: Duration) -> Self {
        let now = Instant::now();
        Self {
            interval,
            started: now,
            last_print: now,
            total_frames: 0,
            near_samples: 0,
            far_samples: 0,
            out_samples: 0,
            near_sq: 0.0,
            far_sq: 0.0,
            out_sq: 0.0,
            mic_q: 0,
            ref_q: 0,
            out_q: 0,
            mic_input_drops: 0,
            ref_input_drops: 0,
            stale_drops: 0,
            ref_underruns: 0,
            output_overruns: 0,
            output_underruns: 0,
        }
    }

    fn observe(&mut self, sample: StatsSample<'_>) {
        self.total_frames += sample.frame_size as u64;
        self.near_samples += sample.near.len() as u64;
        self.far_samples += sample.far.len() as u64;
        self.out_samples += sample.out.len() as u64;
        self.near_sq += sum_squares(sample.near);
        self.far_sq += sum_squares(sample.far);
        self.out_sq += sum_squares(sample.out);
        self.mic_q = sample.mic_q;
        self.ref_q = sample.ref_q;
        self.out_q = sample.out_q;
        self.mic_input_drops += sample.mic_input_drops;
        self.ref_input_drops += sample.ref_input_drops;
        self.stale_drops += sample.stale_drops;
        self.ref_underruns += sample.ref_underruns;
        self.output_overruns += sample.output_overruns;
        self.output_underruns += sample.output_underruns;
        self.maybe_print();
    }

    fn maybe_print(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_print) < self.interval {
            return;
        }
        let elapsed = now.duration_since(self.started).as_secs();
        println!(
            "t={}s frames={} mic={:.1}dB ref={:.1}dB out={:.1}dB mic_q={} ref_q={} out_q={} ref_underrun={} out_underrun={} out_overrun={} input_drop={} stale_drop={}",
            elapsed,
            self.total_frames,
            rms_dbfs(self.near_sq, self.near_samples),
            rms_dbfs(self.far_sq, self.far_samples),
            rms_dbfs(self.out_sq, self.out_samples),
            self.mic_q,
            self.ref_q,
            self.out_q,
            self.ref_underruns,
            self.output_underruns,
            self.output_overruns,
            self.mic_input_drops + self.ref_input_drops,
            self.stale_drops,
        );
        self.last_print = now;
        self.near_samples = 0;
        self.far_samples = 0;
        self.out_samples = 0;
        self.near_sq = 0.0;
        self.far_sq = 0.0;
        self.out_sq = 0.0;
        self.mic_input_drops = 0;
        self.ref_input_drops = 0;
        self.stale_drops = 0;
        self.ref_underruns = 0;
        self.output_overruns = 0;
        self.output_underruns = 0;
    }
}

fn sum_squares(samples: &[f32]) -> f64 {
    samples.iter().map(|v| (*v as f64) * (*v as f64)).sum()
}

fn rms_dbfs(sum_sq: f64, samples: u64) -> f64 {
    if samples == 0 || sum_sq <= 0.0 {
        return -120.0;
    }
    let rms = (sum_sq / samples as f64).sqrt().max(1e-6);
    (20.0 * rms.log10()).max(-120.0)
}

// ── 设备选择 ──────────────────────────────────────────────────────────────────

fn mic_selector(s: &str) -> Option<&str> {
    match s {
        "default" | "" => None,
        other => Some(other),
    }
}
fn output_selector(s: &str) -> Option<&str> {
    match s {
        "default" | "" => None,
        other => Some(other),
    }
}

fn select_default_device(host: &cpal::Host, kind: DeviceKind) -> Result<SelectedDevice> {
    let device = match kind {
        DeviceKind::Input => host.default_input_device(),
        DeviceKind::Output => host.default_output_device(),
    }
    .with_context(|| format!("无默认 {} 设备", kind.label()))?;
    let devices = devices_for(host, kind).unwrap_or_default();
    let index = find_device_index(&devices, &device);
    Ok(SelectedDevice { index, device })
}

fn select_device(
    host: &cpal::Host,
    kind: DeviceKind,
    selector: Option<&str>,
) -> Result<SelectedDevice> {
    if let Some(selector) = selector {
        let devices = devices_for(host, kind)?;
        if let Ok(index) = selector.parse::<usize>() {
            let device = devices
                .get(index)
                .cloned()
                .with_context(|| format!("无 {} 设备索引 {index}", kind.label()))?;
            return Ok(SelectedDevice {
                index: Some(index),
                device,
            });
        }
        let needle = selector.to_lowercase();
        return devices
            .into_iter()
            .enumerate()
            .find(|(_, d)| device_search_text(d).to_lowercase().contains(&needle))
            .map(|(index, device)| SelectedDevice {
                index: Some(index),
                device,
            })
            .with_context(|| format!("无名称含 {selector:?} 的 {} 设备", kind.label()));
    }
    select_default_device(host, kind)
}

fn select_render_device(host: &cpal::Host, selector: &str) -> Result<(SelectedDevice, DeviceKind)> {
    if let Some((prefix, sel)) = selector.split_once(':') {
        let kind = match prefix.to_lowercase().as_str() {
            "input" | "in" => DeviceKind::Input,
            "output" | "out" => DeviceKind::Output,
            _ => bail!("参考设备前缀须为 input: 或 output:"),
        };
        return Ok((select_device(host, kind, Some(sel))?, kind));
    }
    if let Ok(d) = select_device(host, DeviceKind::Output, Some(selector)) {
        return Ok((d, DeviceKind::Output));
    }
    select_device(host, DeviceKind::Input, Some(selector)).map(|d| (d, DeviceKind::Input))
}

fn devices_for(host: &cpal::Host, kind: DeviceKind) -> Result<Vec<Device>> {
    match kind {
        DeviceKind::Input => Ok(host.input_devices()?.collect()),
        DeviceKind::Output => Ok(host.output_devices()?.collect()),
    }
}

fn pick_config(
    device: &Device,
    kind: DeviceKind,
    sample_rate: u32,
) -> Result<SupportedStreamConfig> {
    let ranges: Vec<SupportedStreamConfigRange> = match kind {
        DeviceKind::Input => device.supported_input_configs()?.collect(),
        DeviceKind::Output => device.supported_output_configs()?.collect(),
    };
    ranges
        .into_iter()
        .filter(|r| !r.sample_format().is_dsd())
        .filter(|r| r.min_sample_rate() <= sample_rate && sample_rate <= r.max_sample_rate())
        .max_by(|a, b| a.cmp_default_heuristics(b))
        .map(|r| r.with_sample_rate(sample_rate))
        .with_context(|| {
            format!(
                "{} 在 {sample_rate} Hz 无可用 {} 配置",
                device_label(device),
                kind.label()
            )
        })
}

// ── 流构建(多采样格式)────────────────────────────────────────────────────────

macro_rules! dispatch_format {
    ($fmt:expr, $build:ident, $($arg:expr),+) => {
        match $fmt {
            SampleFormat::I16 => $build::<i16, _>($($arg),+),
            SampleFormat::I32 => $build::<i32, _>($($arg),+),
            SampleFormat::F32 => $build::<f32, _>($($arg),+),
            SampleFormat::U16 => $build::<u16, _>($($arg),+),
            other => bail!("不支持的采样格式 {other}"),
        }
    };
}

fn build_input_stream<P>(
    device: &Device,
    config: &SupportedStreamConfig,
    producer: P,
    label: &'static str,
    channel_mode: InputChannelMode,
    drops: Arc<AtomicU64>,
) -> Result<Stream>
where
    P: Producer<Item = f32> + Send + 'static,
{
    dispatch_format!(
        config.sample_format(),
        build_input_stream_t,
        device,
        config,
        producer,
        label,
        channel_mode,
        drops
    )
}

fn build_input_stream_t<T, P>(
    device: &Device,
    supported: &SupportedStreamConfig,
    mut producer: P,
    label: &'static str,
    channel_mode: InputChannelMode,
    drops: Arc<AtomicU64>,
) -> Result<Stream>
where
    T: SizedSample + Copy + Send + 'static,
    f32: FromSample<T>,
    P: Producer<Item = f32> + Send + 'static,
{
    let config = supported.config();
    let channels = usize::from(config.channels);
    device
        .build_input_stream(
            &config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                for frame in data.chunks(channels) {
                    push_input_frame(frame, channel_mode, &mut producer, &drops);
                }
            },
            move |err| eprintln!("{label} 流错误: {err}"),
            None,
        )
        .with_context(|| format!("构建 {label} 输入流失败"))
}

fn push_input_frame<T, P>(
    frame: &[T],
    channel_mode: InputChannelMode,
    producer: &mut P,
    drops: &AtomicU64,
) where
    T: Copy,
    f32: FromSample<T>,
    P: Producer<Item = f32>,
{
    match channel_mode {
        InputChannelMode::MonoDownmix => {
            let sum = frame.iter().copied().map(f32::from_sample).sum::<f32>();
            let sample = if frame.is_empty() {
                0.0
            } else {
                sum / frame.len() as f32
            };
            if producer.try_push(sample).is_err() {
                drops.fetch_add(1, Ordering::Relaxed);
            }
        }
        InputChannelMode::PreserveFirst(channels) => {
            for ch in 0..channels {
                let sample = frame.get(ch).copied().map(f32::from_sample).unwrap_or(0.0);
                if producer.try_push(sample).is_err() {
                    drops.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

fn build_output_stream<C>(
    device: &Device,
    config: &SupportedStreamConfig,
    consumer: C,
    underruns: Arc<AtomicU64>,
) -> Result<Stream>
where
    C: Consumer<Item = f32> + Send + 'static,
{
    dispatch_format!(
        config.sample_format(),
        build_output_stream_t,
        device,
        config,
        consumer,
        underruns
    )
}

fn build_output_stream_t<T, C>(
    device: &Device,
    supported: &SupportedStreamConfig,
    mut consumer: C,
    underruns: Arc<AtomicU64>,
) -> Result<Stream>
where
    T: SizedSample + FromSample<f32> + Copy + Send + 'static,
    C: Consumer<Item = f32> + Send + 'static,
{
    let config = supported.config();
    let channels = usize::from(config.channels);
    device
        .build_output_stream(
            &config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let sample = match consumer.try_pop() {
                        Some(v) => v.clamp(-1.0, 1.0),
                        None => {
                            underruns.fetch_add(1, Ordering::Relaxed);
                            0.0
                        }
                    };
                    let s = T::from_sample(sample);
                    for out in frame {
                        *out = s; // 单声道铺到所有输出声道
                    }
                }
            },
            |err| eprintln!("输出流错误: {err}"),
            None,
        )
        .context("构建输出流失败")
}

// ── 设备列表 ──────────────────────────────────────────────────────────────────

pub fn print_devices() -> Result<()> {
    let host = cpal::default_host();
    for kind in [DeviceKind::Input, DeviceKind::Output] {
        println!("{} 设备:", kind.label());
        for (i, d) in devices_for(&host, kind)?.iter().enumerate() {
            let cfg = match kind {
                DeviceKind::Input => d.default_input_config(),
                DeviceKind::Output => d.default_output_config(),
            };
            let summary = cfg
                .map(|c| config_summary(&c))
                .unwrap_or_else(|e| format!("无默认配置: {e}"));
            println!(
                "  {} ({summary})",
                format_indexed_label(Some(i), device_label(d))
            );
        }
    }
    println!(
        "\n用法:run --config 配置文件;也可用 --mic / --reference / --output 覆盖配置里的设备选择。"
    );
    println!(
        "reference 还支持 'system'(默认输出 loopback)/ 'none' / 'output:<名>' / 'input:<名>'。"
    );
    Ok(())
}

fn config_summary(c: &SupportedStreamConfig) -> String {
    format!(
        "{} Hz, {} ch, {}",
        c.sample_rate(),
        c.channels(),
        c.sample_format()
    )
}

fn selected_device_label(selected: &SelectedDevice) -> String {
    format_indexed_label(selected.index, device_label(&selected.device))
}

fn format_indexed_label(index: Option<usize>, label: String) -> String {
    match index {
        Some(index) => format!("[{index}] {label}"),
        None => label,
    }
}

fn device_label(device: &Device) -> String {
    device
        .description()
        .map(|d| format_device_description(&d))
        .unwrap_or_else(|_| "<未知>".to_owned())
}

fn format_device_description(desc: &DeviceDescription) -> String {
    let name = desc.name().trim();
    let detail = desc
        .driver()
        .filter(|v| distinct_label(name, v))
        .or_else(|| {
            desc.extended()
                .iter()
                .map(String::as_str)
                .find(|v| distinct_label(name, v))
        })
        .or_else(|| desc.manufacturer().filter(|v| distinct_label(name, v)));

    match detail {
        Some(detail) => format!("{name} / {}", detail.trim()),
        None => {
            let display = desc.to_string();
            let display = display.trim();
            if display.is_empty() {
                name.to_owned()
            } else {
                display.to_owned()
            }
        }
    }
}

fn distinct_label(primary: &str, candidate: &str) -> bool {
    let primary = primary.trim();
    let candidate = candidate.trim();
    !candidate.is_empty() && !candidate.eq_ignore_ascii_case(primary)
}

fn device_search_text(device: &Device) -> String {
    let mut parts = Vec::new();
    if let Ok(desc) = device.description() {
        parts.push(desc.name().to_owned());
        parts.extend(desc.manufacturer().map(str::to_owned));
        parts.extend(desc.driver().map(str::to_owned));
        parts.extend(desc.address().map(str::to_owned));
        parts.extend(desc.extended().iter().cloned());
        parts.push(desc.to_string());
    }
    if let Ok(id) = device.id() {
        parts.push(id.to_string());
    }
    parts.join(" ")
}

fn find_device_index(devices: &[Device], selected: &Device) -> Option<usize> {
    selected.id().ok().and_then(|id| {
        devices
            .iter()
            .position(|device| device.id().ok().as_ref() == Some(&id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::{DeviceDescriptionBuilder, DeviceType, InterfaceType};

    #[test]
    fn device_description_label_prefers_driver_detail() {
        let desc = DeviceDescriptionBuilder::new("麦克风")
            .driver("USB Condenser Microphone")
            .device_type(DeviceType::Microphone)
            .interface_type(InterfaceType::Usb)
            .build();

        assert_eq!(
            format_device_description(&desc),
            "麦克风 / USB Condenser Microphone"
        );
    }

    #[test]
    fn rms_dbfs_reports_silence_and_full_scale() {
        assert_eq!(rms_dbfs(0.0, 480), -120.0);
        assert_eq!(rms_dbfs(480.0, 480), 0.0);
    }

    #[test]
    fn input_channel_mode_downmixes_or_preserves_stereo() {
        let drops = AtomicU64::new(0);
        let (mut mono_prod, mut mono_cons) = HeapRb::<f32>::new(4).split();
        push_input_frame(
            &[0.0f32, 0.5, 1.0],
            InputChannelMode::MonoDownmix,
            &mut mono_prod,
            &drops,
        );
        let mut mono = [0.0f32; 1];
        mono_cons.pop_slice(&mut mono);
        assert_eq!(mono[0], 0.5);

        let (mut stereo_prod, mut stereo_cons) = HeapRb::<f32>::new(4).split();
        push_input_frame(
            &[0.25f32, -0.75, 0.5],
            InputChannelMode::PreserveFirst(2),
            &mut stereo_prod,
            &drops,
        );
        let mut stereo = [0.0f32; 2];
        stereo_cons.pop_slice(&mut stereo);
        assert_eq!(stereo, [0.25, -0.75]);
    }
}
