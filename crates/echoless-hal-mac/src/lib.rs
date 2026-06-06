//! macOS HAL 后端(骨架 stub)。
//!
//! 实现路线见蓝本 §8(基线 macOS 14.4+):
//!
//! - MicSource         = CoreAudio AUHAL(kAudioUnitSubType_HALOutput,enable input)
//! - SystemAudioSource = Core Audio Process Tap(`AudioHardwareCreateProcessTap` + aggregate device)
//! - VirtualMicSink    = 写入用户选择的外部虚拟设备,例如 BlackHole
//! - 时钟 = AudioTimeStamp.mHostTime(mach 时基);实时线程 os_workgroup_join
//!
//! 现阶段 start() 报错,标明待实现点。

use std::time::Duration;

use echoless_hal::{AudioFormat, AudioSink, AudioSource, OwnedPacket};

pub struct MicSource {
    id: String,
}
impl MicSource {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_string() }
    }
}
impl AudioSource for MicSource {
    fn start(&mut self) -> anyhow::Result<AudioFormat> {
        anyhow::bail!("[mac] CoreAudio AUHAL 麦克风采集未实现 (id={}) — 见蓝本 §8", self.id)
    }
    fn read(&mut self, _t: Duration) -> anyhow::Result<Option<OwnedPacket>> {
        Ok(None)
    }
    fn stop(&mut self) {}
}

pub struct SystemAudioSource {
    id: String,
}
impl SystemAudioSource {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_string() }
    }
}
impl AudioSource for SystemAudioSource {
    fn start(&mut self) -> anyhow::Result<AudioFormat> {
        anyhow::bail!("[mac] Core Audio Process Tap 系统音频捕获未实现 (id={}) — 见蓝本 §8.1(需 TCC 授权)", self.id)
    }
    fn read(&mut self, _t: Duration) -> anyhow::Result<Option<OwnedPacket>> {
        Ok(None)
    }
    fn stop(&mut self) {}
}

pub struct VirtualMicSink {
    id: String,
}
impl VirtualMicSink {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_string() }
    }
}
impl AudioSink for VirtualMicSink {
    fn start(&mut self, _format: AudioFormat) -> anyhow::Result<()> {
        anyhow::bail!("[mac] 虚拟设备输出未实现 (id={}) — 当前实时路径走 cpal 写入 BlackHole 等外部设备", self.id)
    }
    fn write(&mut self, _interleaved: &[f32], _frames: u32) -> anyhow::Result<()> {
        Ok(())
    }
    fn stop(&mut self) {}
}
