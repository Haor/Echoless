use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use ringbuf::traits::Consumer;

pub(super) struct InterleavedLinearResampler {
    in_rate: u32,
    out_rate: u32,
    channels: usize,
    input_frames_seen: u64,
    next_output_source_pos: f64,
    prev_frame: Option<Vec<f32>>,
}

impl InterleavedLinearResampler {
    pub(super) fn new(in_rate: u32, out_rate: u32, channels: usize) -> Self {
        Self {
            in_rate,
            out_rate,
            channels: channels.max(1),
            input_frames_seen: 0,
            next_output_source_pos: 0.0,
            prev_frame: None,
        }
    }

    pub(super) fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.in_rate == self.out_rate || input.is_empty() {
            return input.to_vec();
        }
        let frames = input.len() / self.channels;
        if frames == 0 {
            return Vec::new();
        }

        let start_abs = self.input_frames_seen;
        let end_abs = start_abs + frames as u64;
        let step = self.in_rate as f64 / self.out_rate as f64;
        let mut out = Vec::with_capacity(((frames as f64) / step).ceil() as usize * self.channels);

        while self.next_output_source_pos.floor() as u64 + 1 < end_abs {
            let pos = self.next_output_source_pos;
            let i0 = pos.floor() as u64;
            let i1 = i0 + 1;
            let frac = (pos - i0 as f64) as f32;
            for ch in 0..self.channels {
                let a = self.sample_at(input, start_abs, i0, ch).unwrap_or(0.0);
                let b = self.sample_at(input, start_abs, i1, ch).unwrap_or(a);
                out.push(a + (b - a) * frac);
            }
            self.next_output_source_pos += step;
        }

        self.input_frames_seen = end_abs;
        let last_start = (frames - 1) * self.channels;
        self.prev_frame = Some(input[last_start..last_start + self.channels].to_vec());
        out
    }

    fn sample_at(
        &self,
        input: &[f32],
        start_abs: u64,
        index_abs: u64,
        channel: usize,
    ) -> Option<f32> {
        if index_abs + 1 == start_abs {
            return self
                .prev_frame
                .as_ref()
                .and_then(|frame| frame.get(channel).copied());
        }
        if index_abs < start_abs {
            return None;
        }
        let local = (index_abs - start_abs) as usize;
        input.get(local * self.channels + channel).copied()
    }
}

pub(super) struct OutputLinearResampler {
    step: f64,
    pos: f64,
    buffer: VecDeque<f32>,
}

impl OutputLinearResampler {
    pub(super) fn new(in_rate: u32, out_rate: u32) -> Self {
        let step = if out_rate == 0 {
            1.0
        } else {
            in_rate as f64 / out_rate as f64
        };
        Self {
            step,
            pos: 0.0,
            buffer: VecDeque::new(),
        }
    }

    pub(super) fn next_sample<C>(&mut self, consumer: &mut C, underruns: &AtomicU64) -> f32
    where
        C: Consumer<Item = f32>,
    {
        let needed = (self.pos.floor() as usize).saturating_add(2);
        while self.buffer.len() < needed {
            match consumer.try_pop() {
                Some(sample) => self.buffer.push_back(sample.clamp(-1.0, 1.0)),
                None => {
                    underruns.fetch_add(1, Ordering::Relaxed);
                    return 0.0;
                }
            }
        }

        let i0 = self.pos.floor() as usize;
        let frac = (self.pos - i0 as f64) as f32;
        let a = self.buffer.get(i0).copied().unwrap_or(0.0);
        let b = self.buffer.get(i0 + 1).copied().unwrap_or(a);
        let sample = (a + (b - a) * frac).clamp(-1.0, 1.0);

        self.pos += self.step;
        let consumed = self.pos.floor() as usize;
        for _ in 0..consumed {
            let _ = self.buffer.pop_front();
        }
        self.pos -= consumed as f64;
        sample
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use ringbuf::traits::{Producer, Split};
    use ringbuf::HeapRb;

    use super::*;

    #[test]
    fn input_resampler_upsamples_and_preserves_state() {
        let mut resampler = InterleavedLinearResampler::new(24_000, 48_000, 1);

        let first = resampler.process(&[0.0, 1.0, 2.0, 3.0]);
        let second = resampler.process(&[4.0, 5.0]);

        assert_eq!(first, vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.5]);
        assert_eq!(second, vec![3.0, 3.5, 4.0, 4.5]);
    }

    #[test]
    fn input_resampler_downsamples_fixed_ratio() {
        let mut resampler = InterleavedLinearResampler::new(48_000, 24_000, 1);

        let out = resampler.process(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);

        assert_eq!(out, vec![0.0, 2.0, 4.0]);
    }

    #[test]
    fn output_resampler_pulls_pipeline_samples_at_device_rate() {
        let drops = AtomicU64::new(0);
        let (mut prod, mut cons) = HeapRb::<f32>::new(8).split();
        assert_eq!(prod.push_slice(&[0.0, 0.25, 0.5, 0.75]), 4);
        let mut resampler = OutputLinearResampler::new(48_000, 24_000);
        assert_eq!(resampler.step, 2.0);

        let first = resampler.next_sample(&mut cons, &drops);
        assert_eq!(resampler.pos, 0.0);
        assert_eq!(resampler.buffer.len(), 0);
        let second = resampler.next_sample(&mut cons, &drops);

        assert_eq!(first, 0.0);
        assert_eq!(second, 0.5);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
    }
}
