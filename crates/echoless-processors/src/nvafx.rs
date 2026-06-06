//! NVIDIA AFX runtime discovery and preflight checks.
//!
//! This module intentionally does not load NVIDIA DLLs yet. The first
//! integration layer only answers: "would this machine have enough runtime
//! pieces for the optional RTX AEC backend to start?"

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::{EchoProcessor, IoSpec, ProcessorStats};

pub const SDK_VERSION: &str = "2.1.0";
pub const RUNTIME_FILE_VERSION: &str = "2.1.0.9";
pub const MIN_DRIVER_VERSION: &str = "572.61";
pub const DEFAULT_ENV_VAR: &str = "ECHOLESS_NVAFX_RUNTIME_DIR";

const COMMON_REQUIRED_FILES: &[&str] = &[
    "bin/NVAudioEffects.dll",
    "bin/cublas64_12.dll",
    "bin/cublasLt64_12.dll",
    "bin/cufft64_11.dll",
    "bin/libcrypto-3-x64.dll",
    "bin/nvinfer_10.dll",
    "features/nvafxaec/bin/nvafxaec.dll",
];

const VC_RUNTIME_DLLS: &[&str] = &["VCRUNTIME140.dll", "VCRUNTIME140_1.dll", "MSVCP140.dll"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpuArch {
    Turing,
    Ampere,
    Ada,
    Blackwell,
}

impl GpuArch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Turing => "turing",
            Self::Ampere => "ampere",
            Self::Ada => "ada",
            Self::Blackwell => "blackwell",
        }
    }

    pub fn from_compute_capability(value: &str) -> Option<Self> {
        match normalize_compute_capability(value).as_str() {
            "75" => Some(Self::Turing),
            "80" | "86" => Some(Self::Ampere),
            "89" => Some(Self::Ada),
            "100" | "120" => Some(Self::Blackwell),
            _ => None,
        }
    }

    pub fn model_payload_path(self) -> PathBuf {
        PathBuf::from("features")
            .join("nvafxaec")
            .join("models")
            .join(self.as_str())
            .join("aec_48k.trtpkg")
    }

    pub fn model_asset_name(self) -> String {
        format!(
            "echoless-rtx-aec-model-win64-{SDK_VERSION}-{}-aec48.zip",
            self.as_str()
        )
    }
}

impl fmt::Display for GpuArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub driver_version: String,
    pub compute_capability: String,
    pub arch: Option<GpuArch>,
}

#[derive(Clone, Debug)]
pub enum CheckStatus {
    Ok,
    Warning,
    Missing,
    Unsupported,
}

impl CheckStatus {
    pub fn is_problem(&self) -> bool {
        matches!(self, Self::Missing | Self::Unsupported)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warning => "warn",
            Self::Missing => "missing",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DoctorCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    pub action: Option<String>,
}

impl DoctorCheck {
    fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Ok,
            detail: detail.into(),
            action: None,
        }
    }

    fn warning(
        name: impl Into<String>,
        detail: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Warning,
            detail: detail.into(),
            action: Some(action.into()),
        }
    }

    fn missing(
        name: impl Into<String>,
        detail: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Missing,
            detail: detail.into(),
            action: Some(action.into()),
        }
    }

    fn unsupported(
        name: impl Into<String>,
        detail: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Unsupported,
            detail: detail.into(),
            action: Some(action.into()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DoctorReport {
    pub runtime_dir: PathBuf,
    pub runtime_dir_source: String,
    pub gpus: Vec<GpuInfo>,
    pub selected_arch: Option<GpuArch>,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn ok(&self) -> bool {
        !self.checks.iter().any(|check| check.status.is_problem())
    }

    pub fn expected_model_asset(&self) -> Option<String> {
        self.selected_arch.map(GpuArch::model_asset_name)
    }
}

#[derive(Default)]
pub struct NvidiaAfxAec {
    runtime_dir: Option<PathBuf>,
}

impl NvidiaAfxAec {
    pub fn new() -> Self {
        Self { runtime_dir: None }
    }
}

impl EchoProcessor for NvidiaAfxAec {
    fn name(&self) -> &'static str {
        "nvidia_afx_aec"
    }

    fn io_spec(&self) -> IoSpec {
        IoSpec {
            sample_rate: 48_000,
            near_channels: 1,
            far_channels: 1,
            algorithmic_latency_ms: 0.0,
        }
    }

    fn configure(&mut self, params: &toml::Table) -> Result<()> {
        self.runtime_dir = params
            .get("runtime_dir")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty() && *value != "auto")
            .map(PathBuf::from);

        let report = doctor_report(self.runtime_dir.as_deref())?;
        if !report.ok() {
            bail!("nvidia_afx_aec 尚未可用;请先运行 `echoless nvafx doctor` 修复依赖");
        }
        bail!("nvidia_afx_aec runtime 预检已通过,但实时 AFX DLL 调用尚未接入")
    }

    fn process(&mut self, near: &[f32], _far: &[f32], out: &mut [f32], _frames: u32) {
        let n = near.len().min(out.len());
        out[..n].copy_from_slice(&near[..n]);
        out[n..].fill(0.0);
    }

    fn stats(&self) -> ProcessorStats {
        ProcessorStats::empty("nvidia_afx_aec")
    }

    fn reset(&mut self) {}
}

pub fn doctor_report(runtime_dir_override: Option<&Path>) -> Result<DoctorReport> {
    let (runtime_dir, runtime_dir_source) = resolve_runtime_dir(runtime_dir_override);
    let mut checks = Vec::new();

    if cfg!(windows) {
        checks.extend(check_windows_system_dependencies());
    } else {
        checks.push(DoctorCheck::unsupported(
            "platform",
            "NVIDIA AFX AEC runtime 目前只支持 Windows x64",
            "在 Windows RTX 机器上使用 RTX AEC backend",
        ));
    }

    let gpus = detect_gpus().unwrap_or_else(|err| {
        checks.push(DoctorCheck::missing(
            "nvidia-smi",
            format!("无法运行 nvidia-smi: {err:#}"),
            "安装 NVIDIA graphics driver 572.61 或更新版本",
        ));
        Vec::new()
    });
    let selected_arch = gpus.iter().find_map(|gpu| gpu.arch);
    checks.extend(check_gpus(&gpus));
    checks.extend(check_runtime_files(&runtime_dir, selected_arch));

    Ok(DoctorReport {
        runtime_dir,
        runtime_dir_source,
        gpus,
        selected_arch,
        checks,
    })
}

pub fn resolve_runtime_dir(override_dir: Option<&Path>) -> (PathBuf, String) {
    if let Some(dir) = override_dir {
        return (dir.to_path_buf(), "argument".to_string());
    }
    if let Some(dir) = env::var_os(DEFAULT_ENV_VAR).filter(|value| !value.is_empty()) {
        return (PathBuf::from(dir), DEFAULT_ENV_VAR.to_string());
    }
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    (
        base.join("Echoless").join("nvafx").join(SDK_VERSION),
        "%LOCALAPPDATA%".to_string(),
    )
}

fn check_windows_system_dependencies() -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    for dll in VC_RUNTIME_DLLS {
        if find_windows_system_dll(dll).is_some() {
            checks.push(DoctorCheck::ok(
                format!("vc-runtime:{dll}"),
                "Microsoft VC++ runtime 已存在",
            ));
        } else {
            checks.push(DoctorCheck::missing(
                format!("vc-runtime:{dll}"),
                "未找到 Microsoft VC++ runtime DLL",
                "安装 Microsoft Visual C++ 2015-2022 Redistributable x64",
            ));
        }
    }
    if let Some(path) = find_windows_system_dll("nvcuda.dll") {
        checks.push(DoctorCheck::ok(
            "nvcuda.dll",
            format!("CUDA driver DLL: {}", path.display()),
        ));
    } else {
        checks.push(DoctorCheck::missing(
            "nvcuda.dll",
            "未找到 NVIDIA CUDA driver DLL",
            "安装 NVIDIA graphics driver 572.61 或更新版本",
        ));
    }
    checks
}

fn find_windows_system_dll(name: &str) -> Option<PathBuf> {
    let mut roots = Vec::new();
    if let Some(system_root) = env::var_os("SystemRoot") {
        let root = PathBuf::from(system_root);
        roots.push(root.join("System32"));
        roots.push(root.join("SysWOW64"));
    }
    if let Some(path) = env::var_os("PATH") {
        roots.extend(env::split_paths(&path));
    }
    roots
        .into_iter()
        .map(|root| root.join(name))
        .find(|path| path.is_file())
}

fn detect_gpus() -> Result<Vec<GpuInfo>> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,driver_version,compute_cap",
            "--format=csv,noheader",
        ])
        .output()
        .context("运行 nvidia-smi 失败")?;
    if !output.status.success() {
        bail!(
            "nvidia-smi 退出码 {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(parse_nvidia_smi_gpu_line)
        .collect())
}

fn parse_nvidia_smi_gpu_line(line: &str) -> Option<GpuInfo> {
    let parts: Vec<_> = line.split(',').map(str::trim).collect();
    let [name, driver_version, compute_capability] = parts.as_slice() else {
        return None;
    };
    Some(GpuInfo {
        name: (*name).to_string(),
        driver_version: (*driver_version).to_string(),
        compute_capability: (*compute_capability).to_string(),
        arch: GpuArch::from_compute_capability(compute_capability),
    })
}

fn check_gpus(gpus: &[GpuInfo]) -> Vec<DoctorCheck> {
    if gpus.is_empty() {
        return vec![DoctorCheck::missing(
            "gpu",
            "未检测到 NVIDIA GPU",
            "确认机器有 RTX / Tensor Core GPU 并已安装 NVIDIA driver",
        )];
    }

    let mut checks = Vec::new();
    for (index, gpu) in gpus.iter().enumerate() {
        let label = format!("gpu:{index}");
        if compare_versions(&gpu.driver_version, MIN_DRIVER_VERSION).is_some_and(|ord| ord.is_lt())
        {
            checks.push(DoctorCheck::missing(
                format!("{label}:driver"),
                format!(
                    "{} driver={} 低于最低要求 {}",
                    gpu.name, gpu.driver_version, MIN_DRIVER_VERSION
                ),
                "更新 NVIDIA graphics driver",
            ));
        } else {
            checks.push(DoctorCheck::ok(
                format!("{label}:driver"),
                format!("{} driver={}", gpu.name, gpu.driver_version),
            ));
        }
        match gpu.arch {
            Some(arch) => checks.push(DoctorCheck::ok(
                format!("{label}:arch"),
                format!(
                    "{} compute_cap={} -> {arch}",
                    gpu.name, gpu.compute_capability
                ),
            )),
            None => checks.push(DoctorCheck::unsupported(
                format!("{label}:arch"),
                format!(
                    "{} compute_cap={} 不在支持列表",
                    gpu.name, gpu.compute_capability
                ),
                "RTX AEC backend 需要 Turing/Ampere/Ada/Blackwell 架构",
            )),
        }
    }
    checks
}

fn check_runtime_files(runtime_dir: &Path, selected_arch: Option<GpuArch>) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    if runtime_dir.is_dir() {
        checks.push(DoctorCheck::ok(
            "runtime-dir",
            format!("runtime 目录: {}", runtime_dir.display()),
        ));
    } else {
        checks.push(DoctorCheck::missing(
            "runtime-dir",
            format!("runtime 目录不存在: {}", runtime_dir.display()),
            "下载并解压 echoless-rtx-aec-common-runtime-win64-2.1.0.zip",
        ));
    }

    for rel in COMMON_REQUIRED_FILES {
        let path = runtime_dir.join(rel);
        if path.is_file() {
            checks.push(DoctorCheck::ok(
                format!("runtime:{rel}"),
                format!("found {}", path.display()),
            ));
        } else {
            checks.push(DoctorCheck::missing(
                format!("runtime:{rel}"),
                format!("missing {}", path.display()),
                "下载并解压 common runtime zip",
            ));
        }
    }

    match selected_arch {
        Some(arch) => {
            let rel = arch.model_payload_path();
            let path = runtime_dir.join(&rel);
            if path.is_file() {
                checks.push(DoctorCheck::ok(
                    "runtime:model",
                    format!("found {}", path.display()),
                ));
            } else {
                checks.push(DoctorCheck::missing(
                    "runtime:model",
                    format!("missing {}", path.display()),
                    format!("下载并解压 {}", arch.model_asset_name()),
                ));
            }
        }
        None => checks.push(DoctorCheck::warning(
            "runtime:model",
            "无法判断应使用哪个 AEC 模型",
            "先修复 GPU/driver 检测",
        )),
    }

    checks
}

fn normalize_compute_capability(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect()
}

fn compare_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let mut left_parts = parse_version_parts(left)?;
    let mut right_parts = parse_version_parts(right)?;
    let len = left_parts.len().max(right_parts.len());
    left_parts.resize(len, 0);
    right_parts.resize(len, 0);
    Some(left_parts.cmp(&right_parts))
}

fn parse_version_parts(value: &str) -> Option<Vec<u32>> {
    let parts: Vec<_> = value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (!parts.is_empty()).then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_compute_capability_to_arch() {
        assert_eq!(
            GpuArch::from_compute_capability("7.5"),
            Some(GpuArch::Turing)
        );
        assert_eq!(
            GpuArch::from_compute_capability("8.6"),
            Some(GpuArch::Ampere)
        );
        assert_eq!(GpuArch::from_compute_capability("8.9"), Some(GpuArch::Ada));
        assert_eq!(
            GpuArch::from_compute_capability("12.0"),
            Some(GpuArch::Blackwell)
        );
        assert_eq!(GpuArch::from_compute_capability("9.0"), None);
    }

    #[test]
    fn parses_nvidia_smi_line() {
        let gpu = parse_nvidia_smi_gpu_line("NVIDIA GeForce RTX 5080, 596.49, 12.0").unwrap();
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 5080");
        assert_eq!(gpu.driver_version, "596.49");
        assert_eq!(gpu.arch, Some(GpuArch::Blackwell));
    }

    #[test]
    fn compares_driver_versions() {
        assert!(compare_versions("596.49", MIN_DRIVER_VERSION)
            .unwrap()
            .is_gt());
        assert!(compare_versions("572.61", MIN_DRIVER_VERSION)
            .unwrap()
            .is_eq());
        assert!(compare_versions("551.86", MIN_DRIVER_VERSION)
            .unwrap()
            .is_lt());
    }

    #[test]
    fn model_asset_and_payload_match_distribution() {
        let arch = GpuArch::Blackwell;
        assert_eq!(
            arch.model_asset_name(),
            "echoless-rtx-aec-model-win64-2.1.0-blackwell-aec48.zip"
        );
        assert_eq!(
            arch.model_payload_path(),
            PathBuf::from("features")
                .join("nvafxaec")
                .join("models")
                .join("blackwell")
                .join("aec_48k.trtpkg")
        );
    }
}
