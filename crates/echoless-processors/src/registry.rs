//! 处理器注册表:kind 字符串 → `Box<dyn EchoProcessor>`。新增方案在此登记一行。

use crate::{
    aec3::Aec3Engine, localvqe::LocalVqe, nvafx::NvidiaAfxAec, passthrough::Passthrough,
    rnnoise::RnNoise, webrtc_ns::WebRtcNs, EchoProcessor,
};

pub fn build(kind: &str) -> anyhow::Result<Box<dyn EchoProcessor>> {
    Ok(match kind {
        "passthrough" => Box::new(Passthrough::new()),
        "aec3" => Box::new(Aec3Engine::new()),
        "localvqe" => Box::new(LocalVqe::new()),
        "nvidia_afx_aec" => Box::new(NvidiaAfxAec::new()),
        "webrtc_ns" => Box::new(WebRtcNs::new()),
        "rnnoise" => Box::new(RnNoise::try_new()?),
        other => anyhow::bail!(
            "unknown processor kind: {other} (available: passthrough / aec3 / localvqe / nvidia_afx_aec / webrtc_ns / rnnoise)"
        ),
    })
}

/// 已注册的处理器种类(供 CLI/前端列出)。
pub fn kinds() -> &'static [&'static str] {
    &[
        "passthrough",
        "aec3",
        "localvqe",
        "nvidia_afx_aec",
        "webrtc_ns",
        "rnnoise",
    ]
}
