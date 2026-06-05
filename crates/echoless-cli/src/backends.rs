//! 平台后端 dispatch:按 cfg 选 Windows / macOS HAL,其余 OS 用 null。
//! 演示「核心无关平台,前端按 cfg 装配」的边界(蓝本 §1/§5)。

use echoless_hal::{AudioSink, AudioSource};

#[cfg(target_os = "windows")]
pub fn make_mic(id: &str) -> Box<dyn AudioSource> {
    Box::new(echoless_hal_win::MicSource::new(id))
}
#[cfg(target_os = "windows")]
pub fn make_reference(id: &str) -> Box<dyn AudioSource> {
    Box::new(echoless_hal_win::SystemAudioSource::new(id))
}
#[cfg(target_os = "windows")]
pub fn make_output(id: &str) -> Box<dyn AudioSink> {
    Box::new(echoless_hal_win::VirtualMicSink::new(id))
}

#[cfg(target_os = "macos")]
pub fn make_mic(id: &str) -> Box<dyn AudioSource> {
    Box::new(echoless_hal_mac::MicSource::new(id))
}
#[cfg(target_os = "macos")]
pub fn make_reference(id: &str) -> Box<dyn AudioSource> {
    Box::new(echoless_hal_mac::SystemAudioSource::new(id))
}
#[cfg(target_os = "macos")]
pub fn make_output(id: &str) -> Box<dyn AudioSink> {
    Box::new(echoless_hal_mac::VirtualMicSink::new(id))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn make_mic(id: &str) -> Box<dyn AudioSource> {
    Box::new(echoless_hal::null::NullSource::new(format!("mic:{id}(不支持的 OS)")))
}
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn make_reference(id: &str) -> Box<dyn AudioSource> {
    Box::new(echoless_hal::null::NullSource::new(format!("ref:{id}(不支持的 OS)")))
}
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn make_output(id: &str) -> Box<dyn AudioSink> {
    Box::new(echoless_hal::null::NullSink::new(format!("out:{id}(不支持的 OS)")))
}
