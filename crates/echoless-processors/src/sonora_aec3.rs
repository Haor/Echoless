//! 经典 AEC3 节点(包 sonora — 纯 Rust WebRTC AudioProcessing 移植)。
//!
//! io_spec:48k,near mono,far stereo(保留 L/R,蓝本 §7 / 主文档 §8.3)。
//! 处理域固定 10ms = 480 样本/声道(sonora 硬要求);本节点内部按 480 分块 + 末块零填充。
//! 调用顺序铁律:每块先 process_render(far),再 process_capture(near)。
//!
//! feature `sonora-engine`(默认开)= 真实 AEC3;关掉 = 直通(便于无网络/快速构建)。

use crate::{EchoProcessor, IoSpec, ProcessorStats};

const SR: u32 = 48_000;
const FRAME: usize = 480; // 10ms @ 48k

pub struct SonoraAec3 {
    initial_delay_ms: i32,
    last: ProcessorStats,
    #[cfg(feature = "sonora-engine")]
    inner: Inner,
    #[cfg(feature = "sonora-engine")]
    delay_applied: bool,
}

impl SonoraAec3 {
    pub fn new() -> Self {
        Self {
            initial_delay_ms: 0,
            last: ProcessorStats::empty("sonora_aec3"),
            #[cfg(feature = "sonora-engine")]
            inner: Inner::new(),
            #[cfg(feature = "sonora-engine")]
            delay_applied: false,
        }
    }
}
impl Default for SonoraAec3 {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoProcessor for SonoraAec3 {
    fn name(&self) -> &'static str {
        "sonora_aec3"
    }
    fn io_spec(&self) -> IoSpec {
        IoSpec { sample_rate: SR, near_channels: 1, far_channels: 2, algorithmic_latency_ms: 0.0 }
    }
    fn configure(&mut self, params: &toml::Table) -> anyhow::Result<()> {
        if let Some(v) = params.get("initial_delay_ms").and_then(|v| v.as_integer()) {
            self.initial_delay_ms = v as i32;
        }
        // 注:max_tail / EchoCanceller3Config 细调在 sonora 0.1 高层 API 未暴露(探索报告 §4.1)
        Ok(())
    }
    fn set_stream_delay_ms(&mut self, ms: i32) {
        self.initial_delay_ms = ms;
        #[cfg(feature = "sonora-engine")]
        {
            self.delay_applied = false;
        }
    }
    fn process(&mut self, near: &[f32], far: &[f32], out: &mut [f32], frames: u32) {
        #[cfg(feature = "sonora-engine")]
        {
            self.process_sonora(near, far, out, frames as usize);
        }
        #[cfg(not(feature = "sonora-engine"))]
        {
            let _ = (far, frames);
            let n = out.len().min(near.len());
            out[..n].copy_from_slice(&near[..n]);
            for v in out[n..].iter_mut() {
                *v = 0.0;
            }
        }
    }
    fn stats(&self) -> ProcessorStats {
        self.last.clone()
    }
    fn reset(&mut self) {
        #[cfg(feature = "sonora-engine")]
        {
            self.inner = Inner::new();
            self.delay_applied = false;
        }
    }
}

// ── 真实 AEC3 实现 ────────────────────────────────────────────────────────────
#[cfg(feature = "sonora-engine")]
struct Inner {
    apm: sonora::AudioProcessing,
    near_buf: Vec<f32>,
    far_l: Vec<f32>,
    far_r: Vec<f32>,
    far_out_l: Vec<f32>,
    far_out_r: Vec<f32>,
    out_buf: Vec<f32>,
}

#[cfg(feature = "sonora-engine")]
impl Inner {
    fn new() -> Self {
        use sonora::config::{EchoCanceller, Pipeline};
        use sonora::{AudioProcessing, Config, StreamConfig};

        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            pipeline: Pipeline {
                multi_channel_render: true,   // far = stereo
                multi_channel_capture: false, // near = mono
                ..Default::default()
            },
            ..Default::default()
        };
        let apm = AudioProcessing::builder()
            .config(config)
            .capture_config(StreamConfig::new(SR, 1))
            .render_config(StreamConfig::new(SR, 2))
            .echo_detector(true) // 提供 residual_echo_likelihood
            .build();

        Self {
            apm,
            near_buf: vec![0.0; FRAME],
            far_l: vec![0.0; FRAME],
            far_r: vec![0.0; FRAME],
            far_out_l: vec![0.0; FRAME],
            far_out_r: vec![0.0; FRAME],
            out_buf: vec![0.0; FRAME],
        }
    }
}

#[cfg(feature = "sonora-engine")]
impl SonoraAec3 {
    fn process_sonora(&mut self, near: &[f32], far: &[f32], out: &mut [f32], frames: usize) {
        if !self.delay_applied && self.initial_delay_ms > 0 {
            let _ = self.inner.apm.set_stream_delay_ms(self.initial_delay_ms);
            self.delay_applied = true;
        }

        let mut i = 0;
        while i < frames {
            let blk = (frames - i).min(FRAME);

            // near(mono)→ pad 到 480
            for j in 0..FRAME {
                self.inner.near_buf[j] = if j < blk { near.get(i + j).copied().unwrap_or(0.0) } else { 0.0 };
            }
            // far(stereo interleaved)→ 去交织 + pad
            for j in 0..FRAME {
                if j < blk {
                    self.inner.far_l[j] = far.get((i + j) * 2).copied().unwrap_or(0.0);
                    self.inner.far_r[j] = far.get((i + j) * 2 + 1).copied().unwrap_or(0.0);
                } else {
                    self.inner.far_l[j] = 0.0;
                    self.inner.far_r[j] = 0.0;
                }
            }

            // 先 render(far),再 capture(near)
            let _ = self.inner.apm.process_render_f32(
                &[&self.inner.far_l, &self.inner.far_r],
                &mut [&mut self.inner.far_out_l, &mut self.inner.far_out_r],
            );
            let _ = self.inner.apm.process_capture_f32(
                &[&self.inner.near_buf],
                &mut [&mut self.inner.out_buf],
            );

            let n = blk.min(out.len().saturating_sub(i));
            out[i..i + n].copy_from_slice(&self.inner.out_buf[..n]);
            i += blk;
        }

        let s = self.inner.apm.statistics();
        self.last = ProcessorStats {
            name: "sonora_aec3",
            erle_db: s.echo_return_loss_enhancement.unwrap_or(0.0) as f32,
            residual_echo_likelihood: s.residual_echo_likelihood.unwrap_or(0.0) as f32,
            estimated_delay_ms: s.delay_ms.unwrap_or(0),
            diverged: s.divergent_filter_fraction.map(|f| f > 0.5).unwrap_or(false),
            mic_clipped: false,
        };
    }
}
