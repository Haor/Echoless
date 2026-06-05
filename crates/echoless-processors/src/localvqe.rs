//! LocalVQE 节点(GGML,端到端 AEC+NS+dereverb)。骨架阶段为 stub:io_spec 真实(16k/mono),process 暂直通。
//!
//! 实现阶段:经 FFI 调 liblocalvqe(`localvqe_process_frame_f32`,16k mono mic + 16k mono ref)。
//! 可单开(链长 1)或串联(链中,near=上一级输出,far=真实 ref)。详见蓝本 §7、主文档 §3.3。
//! 注意:16ms 算法延迟;GGML/TFLite 实测须单线程(探索报告 §4.3 #20)。

use crate::{EchoProcessor, IoSpec, ProcessorStats};

pub struct LocalVqe {
    model_path: Option<String>,
    // TODO(impl): liblocalvqe handle (FFI)
}

impl LocalVqe {
    pub fn new() -> Self {
        Self { model_path: None }
    }
}
impl Default for LocalVqe {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoProcessor for LocalVqe {
    fn name(&self) -> &'static str {
        "localvqe"
    }
    fn io_spec(&self) -> IoSpec {
        IoSpec { sample_rate: 16000, near_channels: 1, far_channels: 1, algorithmic_latency_ms: 16.0 }
    }
    fn configure(&mut self, params: &toml::Table) -> anyhow::Result<()> {
        self.model_path = params.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
        // TODO(impl): 加载 GGUF 权重;无 model 时报错或回退
        Ok(())
    }
    fn process(&mut self, near: &[f32], _far: &[f32], out: &mut [f32], _frames: u32) {
        // TODO(impl): localvqe_process_frame_f32(near 16k mono, far 16k mono) → out
        let n = out.len().min(near.len());
        out[..n].copy_from_slice(&near[..n]);
        for v in out[n..].iter_mut() {
            *v = 0.0;
        }
    }
    fn stats(&self) -> ProcessorStats {
        ProcessorStats::empty("localvqe")
    }
    fn reset(&mut self) {}
}
