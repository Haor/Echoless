//! 处理器注册表:kind 字符串 → `Box<dyn EchoProcessor>`。新增方案在此登记一行。

use crate::{
    localvqe::LocalVqe, nvafx::NvidiaAfxAec, passthrough::Passthrough, sonora_aec3::SonoraAec3,
    EchoProcessor,
};

pub fn build(kind: &str) -> anyhow::Result<Box<dyn EchoProcessor>> {
    Ok(match kind {
        "passthrough" => Box::new(Passthrough::new()),
        "sonora_aec3" => Box::new(SonoraAec3::new()),
        "localvqe" => Box::new(LocalVqe::new()),
        "nvidia_afx_aec" => Box::new(NvidiaAfxAec::new()),
        other => anyhow::bail!(
            "未知处理器 kind: {other}(可用: passthrough / sonora_aec3 / localvqe / nvidia_afx_aec)"
        ),
    })
}

/// 已注册的处理器种类(供 CLI/前端列出)。
pub fn kinds() -> &'static [&'static str] {
    &["passthrough", "sonora_aec3", "localvqe", "nvidia_afx_aec"]
}
