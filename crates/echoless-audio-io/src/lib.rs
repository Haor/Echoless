//! echoless-audio-io — 平台无关音频 I/O 抽象。
//!
//! 麦克风与 far-end reference 都是一个 `AudioSource`。当前实时路径直接走 cpal;
//! 本 crate 只服务离线评测:通用 trait 与 WAV 文件后端。

use std::time::Duration;

pub mod file;

/// 采样格式(交织 interleaved)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// 一帧带时间戳的音频(交织 f32)。时间戳统一为单调纳秒(平台各自换算)。
#[derive(Clone, Debug)]
pub struct OwnedPacket {
    pub data: Vec<f32>,
    pub format: AudioFormat,
    pub frames: u32,
    pub timestamp_ns: u64,
    pub device_pos: u64,
}

/// 音频输入源:麦克风 与 far-end reference 都实现它。
pub trait AudioSource: Send {
    fn start(&mut self) -> anyhow::Result<AudioFormat>;
    /// 取下一帧;EOF/无更多数据返回 `Ok(None)`。
    fn read(&mut self, timeout: Duration) -> anyhow::Result<Option<OwnedPacket>>;
    fn stop(&mut self);
}

/// 音频输出汇:写入用户选择的输出设备,通常是 VB-Cable/BlackHole 等外部虚拟设备。
pub trait AudioSink: Send {
    fn start(&mut self, format: AudioFormat) -> anyhow::Result<()>;
    fn write(&mut self, interleaved: &[f32], frames: u32) -> anyhow::Result<()>;
    fn stop(&mut self);
}
