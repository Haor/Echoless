//! Standalone RNNoise processor backed by the pure-Rust `nnnoiseless` port.

use std::time::Instant;

use crate::{EchoProcessor, IoSpec, ProcessorStats};

const SAMPLE_RATE: u32 = 48_000;
const FRAME_SAMPLES: usize = nnnoiseless::DenoiseState::FRAME_SIZE;
const PCM_SCALE: f32 = i16::MAX as f32;
pub const ALGORITHMIC_LATENCY_MS: f32 = 10.0;

pub struct RnNoise {
    state: Box<nnnoiseless::DenoiseState<'static>>,
    input: [f32; FRAME_SAMPLES],
    output: [f32; FRAME_SAMPLES],
    last: ProcessorStats,
}

impl RnNoise {
    pub fn new() -> Self {
        let mut processor = Self {
            state: nnnoiseless::DenoiseState::new(),
            input: [0.0; FRAME_SAMPLES],
            output: [0.0; FRAME_SAMPLES],
            last: ProcessorStats::empty("rnnoise"),
        };
        processor.prime_state();
        processor
    }

    fn prime_state(&mut self) {
        self.input.fill(0.0);
        self.output.fill(0.0);
        self.state.process_frame(&mut self.output, &self.input);
    }
}

impl Default for RnNoise {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoProcessor for RnNoise {
    fn name(&self) -> &'static str {
        "rnnoise"
    }

    fn io_spec(&self) -> IoSpec {
        IoSpec {
            sample_rate: SAMPLE_RATE,
            near_channels: 1,
            far_channels: 1,
            algorithmic_latency_ms: ALGORITHMIC_LATENCY_MS,
        }
    }

    fn configure(&mut self, _params: &toml::Table) -> anyhow::Result<()> {
        self.reset();
        Ok(())
    }

    fn process(&mut self, near: &[f32], _far: &[f32], out: &mut [f32], frames: u32) {
        let started = Instant::now();
        out.fill(0.0);

        let mut offset = 0;
        let requested_frames = frames as usize;
        while offset < requested_frames {
            let block_len = (requested_frames - offset).min(FRAME_SAMPLES);
            for index in 0..FRAME_SAMPLES {
                let normalized = if index < block_len {
                    near.get(offset + index)
                        .copied()
                        .map(finite_or_zero)
                        .unwrap_or(0.0)
                        .clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                self.input[index] = normalized * PCM_SCALE;
            }

            self.state.process_frame(&mut self.output, &self.input);

            let copy_len = block_len.min(out.len().saturating_sub(offset));
            for index in 0..copy_len {
                out[offset + index] =
                    finite_or_zero(self.output[index] / PCM_SCALE).clamp(-1.0, 1.0);
            }
            offset += block_len;
        }

        self.last.process_time_ms = started.elapsed().as_secs_f32() * 1_000.0;
    }

    fn stats(&self) -> ProcessorStats {
        self.last.clone()
    }

    fn reset(&mut self) {
        self.state = nnnoiseless::DenoiseState::new();
        self.prime_state();
    }
}

fn finite_or_zero(sample: f32) -> f32 {
    if sample.is_finite() {
        sample
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_rnnoise_contract_without_echo_metrics() {
        let processor = RnNoise::new();
        let spec = processor.io_spec();

        assert_eq!(processor.name(), "rnnoise");
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.near_channels, 1);
        assert_eq!(spec.algorithmic_latency_ms, 10.0);
        assert_eq!(processor.stats().erle_db, 0.0);
    }

    #[test]
    fn converts_normalized_pcm_and_preserves_finite_frame_length() {
        let mut processor = RnNoise::new();
        let mut near = (0..FRAME_SAMPLES * 2)
            .map(|index| ((index as f32) * 0.013).sin() * 0.25)
            .collect::<Vec<_>>();
        near[13] = f32::NAN;
        near[700] = f32::NEG_INFINITY;
        let mut out = vec![2.0; near.len()];

        processor.process(&near, &[], &mut out, near.len() as u32);

        assert_eq!(out.len(), near.len());
        assert!(out.iter().all(|sample| sample.is_finite()));
        assert!(out.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        assert!(out.iter().any(|sample| sample.abs() > f32::EPSILON));
        assert_eq!(processor.stats().runtime_error_count, 0);
    }

    #[test]
    fn reset_keeps_processing_ready() {
        let mut processor = RnNoise::new();
        processor.reset();
        let near = vec![0.05; FRAME_SAMPLES];
        let mut out = vec![0.0; FRAME_SAMPLES];

        processor.process(&near, &[], &mut out, FRAME_SAMPLES as u32);

        assert!(out.iter().all(|sample| sample.is_finite()));
    }
}
