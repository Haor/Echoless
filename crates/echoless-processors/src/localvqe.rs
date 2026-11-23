//! LocalVQE node(GGML, end-to-end AEC + NS + dereverb).
//!
//! Runs through the upstream flat C ABI in `localvqe_api.h`. The node domain is
//! fixed at 16 kHz mono mic + 16 kHz mono reference; `ProcessorChain` performs
//! the boundary adaptation from the runtime sample rate.

use anyhow::{bail, Context, Result};
use libloading::Library;
use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_float, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

#[cfg(target_os = "windows")]
use crate::win_dll::DllDirectoryGuard;
use crate::{dsp::copy_or_zero, EchoProcessor, IoSpec, ProcessorStats};

const LOCALVQE_SAMPLE_RATE: u32 = 16_000;
const DEFAULT_NOISE_GATE_THRESHOLD_DBFS: f32 = -45.0;
/// Upper bound for one input block at the node rate (10 ms at 16 kHz is 160 samples).
const STREAM_BLOCK_RESERVE: usize = 512;

#[cfg(target_os = "windows")]
const DEFAULT_LIBRARY_NAMES: &[&str] = &["localvqe.dll"];
#[cfg(target_os = "macos")]
const DEFAULT_LIBRARY_NAMES: &[&str] = &["liblocalvqe.dylib", "localvqe.dylib"];
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
const DEFAULT_LIBRARY_NAMES: &[&str] = &["liblocalvqe.so", "localvqe.so"];

// Opaque C handles; owned values are stored as NonNull, raw pointers only cross the ABI.
type LocalVqeCtx = *mut c_void;
type LocalVqeOptions = *mut c_void;
type OptionsNewFn = unsafe extern "C" fn() -> LocalVqeOptions;
type OptionsFreeFn = unsafe extern "C" fn(LocalVqeOptions);
type OptionsSetStringFn = unsafe extern "C" fn(LocalVqeOptions, *const c_char) -> c_int;
type OptionsSetIntFn = unsafe extern "C" fn(LocalVqeOptions, c_int) -> c_int;
type NewWithOptionsFn = unsafe extern "C" fn(LocalVqeOptions) -> LocalVqeCtx;
type FreeFn = unsafe extern "C" fn(LocalVqeCtx);
type ProcessFrameF32Fn =
    unsafe extern "C" fn(LocalVqeCtx, *const f32, *const f32, c_int, *mut f32) -> c_int;
type LastErrorFn = unsafe extern "C" fn(LocalVqeCtx) -> *const c_char;
type IntGetterFn = unsafe extern "C" fn(LocalVqeCtx) -> c_int;
type ResetFn = unsafe extern "C" fn(LocalVqeCtx);
type SetNoiseGateFn = unsafe extern "C" fn(LocalVqeCtx, c_int, c_float) -> c_int;

pub struct LocalVqe {
    model_path: Option<PathBuf>,
    library_path: Option<PathBuf>,
    backend: Option<String>,
    device: Option<i32>,
    threads: Option<i32>,
    noise_gate: bool,
    noise_gate_threshold_dbfs: f32,
    runtime: Option<Box<dyn LocalVqeBackend>>,
    near_buffer: VecDeque<f32>,
    far_buffer: VecDeque<f32>,
    frame_out: Vec<f32>,
    out_queue: VecDeque<f32>,
    started: bool,
    recovering: bool,
    last_error: Option<String>,
    runtime_errors: u64,
}

impl LocalVqe {
    pub fn new() -> Self {
        Self {
            model_path: None,
            library_path: None,
            backend: None,
            device: None,
            threads: None,
            noise_gate: false,
            noise_gate_threshold_dbfs: DEFAULT_NOISE_GATE_THRESHOLD_DBFS,
            runtime: None,
            near_buffer: VecDeque::new(),
            far_buffer: VecDeque::new(),
            frame_out: Vec::new(),
            out_queue: VecDeque::new(),
            started: false,
            recovering: false,
            last_error: None,
            runtime_errors: 0,
        }
    }

    fn load_runtime(&self) -> Result<Box<dyn LocalVqeBackend>> {
        let model_path = self
            .model_path
            .as_ref()
            .context("localvqe model path is required; set `model = \"models/....gguf\"`")?;
        if !model_path.exists() {
            bail!("localvqe model not found: {}", model_path.display());
        }

        let candidates = library_candidates(self.library_path.as_deref());
        let mut errors = Vec::new();
        for candidate in &candidates {
            match LocalVqeApi::load(candidate) {
                Ok(api) => {
                    let config = LocalVqeRuntimeConfig {
                        model_path,
                        backend: self.backend.as_deref(),
                        device: self.device,
                        threads: self.threads,
                        noise_gate: self.noise_gate,
                        noise_gate_threshold_dbfs: self.noise_gate_threshold_dbfs,
                    };
                    return LocalVqeRuntime::new(api, config)
                        .map(|runtime| Box::new(runtime) as Box<dyn LocalVqeBackend>);
                }
                Err(err) => errors.push(format!("{}: {err}", candidate.display())),
            }
        }

        if candidates.is_empty() {
            bail!(
                "localvqe library not found; set `library = \"localvqe.dll\"` or ECHOLESS_LOCALVQE_LIBRARY"
            );
        }

        bail!(
            "localvqe library could not be loaded; tried: {}",
            errors.join(" | ")
        );
    }

    fn reset_stream_state(&mut self) {
        self.near_buffer.clear();
        self.far_buffer.clear();
        self.out_queue.clear();
        self.started = false;
        self.recovering = false;
        self.last_error = None;
    }

    fn recover_from_runtime_error(&mut self, error: String) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.reset();
        }
        self.near_buffer.clear();
        self.far_buffer.clear();
        self.out_queue.clear();
        self.started = false;
        self.recovering = true;
        self.last_error = Some(error);
        self.runtime_errors = self.runtime_errors.saturating_add(1);
    }

    fn process_loaded(
        &mut self,
        near: &[f32],
        far: &[f32],
        out: &mut [f32],
        frames: usize,
    ) -> Result<()> {
        let samples = out.len().min(frames);
        if samples == 0 {
            return Ok(());
        }

        for i in 0..samples {
            self.near_buffer
                .push_back(near.get(i).copied().unwrap_or(0.0));
            self.far_buffer
                .push_back(far.get(i).copied().unwrap_or(0.0));
        }

        let runtime = self
            .runtime
            .as_mut()
            .context("localvqe runtime is not configured")?;
        let hop = runtime.hop();
        if self.frame_out.len() != hop {
            self.frame_out.resize(hop, 0.0);
        }
        while self.near_buffer.len() >= hop && self.far_buffer.len() >= hop {
            {
                let near_buffer = self.near_buffer.make_contiguous();
                let far_buffer = self.far_buffer.make_contiguous();
                runtime.process_frame(
                    &near_buffer[..hop],
                    &far_buffer[..hop],
                    &mut self.frame_out,
                )?;
            }
            for _ in 0..hop {
                let _ = self.near_buffer.pop_front();
                let _ = self.far_buffer.pop_front();
            }
            self.out_queue.extend(self.frame_out.iter().copied());
        }

        let start_threshold = samples.saturating_add(hop);
        if !self.started && self.out_queue.len() >= start_threshold {
            self.started = true;
            self.recovering = false;
        }

        if self.started {
            for sample in out.iter_mut().take(samples) {
                *sample = self.out_queue.pop_front().unwrap_or(0.0);
            }
        } else if self.recovering {
            copy_or_zero(&near[..near.len().min(samples)], &mut out[..samples]);
        } else {
            out[..samples].fill(0.0);
        }
        out[samples..].fill(0.0);
        Ok(())
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
        IoSpec {
            sample_rate: LOCALVQE_SAMPLE_RATE,
            near_channels: 1,
            far_channels: 1,
            algorithmic_latency_ms: 16.0,
        }
    }

    fn configure(&mut self, params: &toml::Table) -> Result<()> {
        self.model_path = required_path(params, "model")?;
        self.library_path = optional_path(params, "library")
            .or_else(|| std::env::var_os("ECHOLESS_LOCALVQE_LIBRARY").map(PathBuf::from));
        self.backend = optional_string(params, "backend");
        self.device = optional_i32(params, "device")?;
        self.threads = optional_i32(params, "threads")?;
        self.noise_gate = optional_bool(params, "noise_gate").unwrap_or(false);
        self.noise_gate_threshold_dbfs = optional_f32(params, "noise_gate_threshold_dbfs")?
            .unwrap_or(DEFAULT_NOISE_GATE_THRESHOLD_DBFS);

        let runtime = self.load_runtime()?;
        let hop = runtime.hop();
        self.frame_out.resize(hop, 0.0);
        // Steady state holds under one hop plus one input block per queue; reserve
        // headroom so the first audio-thread frames do not allocate.
        let capacity = 4 * (hop + STREAM_BLOCK_RESERVE);
        self.near_buffer.reserve(capacity);
        self.far_buffer.reserve(capacity);
        self.out_queue.reserve(capacity);
        self.runtime = Some(runtime);
        self.reset_stream_state();
        Ok(())
    }

    fn process(&mut self, near: &[f32], far: &[f32], out: &mut [f32], frames: u32) {
        if self.runtime.is_none() {
            copy_or_zero(near, out);
            return;
        }

        if let Err(err) = self.process_loaded(near, far, out, frames as usize) {
            self.recover_from_runtime_error(err.to_string());
            copy_or_zero(near, out);
        }
    }

    fn set_runtime_param(&mut self, key: &str, value: &toml::Value) -> Result<bool> {
        match key {
            "noise_gate" => {
                let candidate = value
                    .as_bool()
                    .ok_or_else(|| anyhow::anyhow!("noise_gate must be a boolean"))?;
                self.apply_runtime_noise_gate(candidate, self.noise_gate_threshold_dbfs)?;
                Ok(true)
            }
            "noise_gate_threshold_dbfs" => {
                let candidate = toml_value_to_f32(value, "noise_gate_threshold_dbfs")?;
                self.apply_runtime_noise_gate(self.noise_gate, candidate)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    // A neural model has no AEC divergence state, so `diverged` stays false.
    fn stats(&self) -> ProcessorStats {
        ProcessorStats {
            runtime_error_count: self.runtime_errors,
            last_backend_error: self.last_error.clone(),
            selected_model: self
                .model_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
            ..ProcessorStats::empty("localvqe")
        }
    }

    fn reset(&mut self) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.reset();
        }
        self.reset_stream_state();
    }
}

impl LocalVqe {
    fn apply_runtime_noise_gate(&mut self, enabled: bool, threshold_dbfs: f32) -> Result<()> {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.set_noise_gate(enabled, threshold_dbfs)?;
        }
        self.noise_gate = enabled;
        self.noise_gate_threshold_dbfs = threshold_dbfs;
        Ok(())
    }
}

struct LocalVqeRuntime {
    api: LocalVqeApi,
    ctx: NonNull<c_void>,
    hop: usize,
}

// The context is used by one owner at a time (no Sync); moving it to the audio thread is fine.
unsafe impl Send for LocalVqeRuntime {}

trait LocalVqeBackend: Send {
    fn hop(&self) -> usize;
    fn process_frame(&mut self, near: &[f32], far: &[f32], out: &mut [f32]) -> Result<()>;
    fn reset(&mut self);
    fn set_noise_gate(&mut self, enabled: bool, threshold_dbfs: f32) -> Result<()>;
}

struct LocalVqeRuntimeConfig<'a> {
    model_path: &'a Path,
    backend: Option<&'a str>,
    device: Option<i32>,
    threads: Option<i32>,
    noise_gate: bool,
    noise_gate_threshold_dbfs: f32,
}

impl LocalVqeRuntime {
    fn new(api: LocalVqeApi, config: LocalVqeRuntimeConfig<'_>) -> Result<Self> {
        let model = cstring_path(config.model_path)?;
        let backend = config.backend.map(cstring).transpose()?;

        let options = NonNull::new(unsafe { (api.options_new)() })
            .context("localvqe_options_new returned null")?;
        let guard = LocalVqeOptionsGuard { api: &api, options };
        let options = options.as_ptr();

        check_setter(
            unsafe { (api.options_set_model_path)(options, model.as_ptr()) },
            "model_path",
        )?;
        if let Some(backend) = &backend {
            check_setter(
                unsafe { (api.options_set_backend)(options, backend.as_ptr()) },
                "backend",
            )?;
        }
        if let Some(device) = config.device {
            check_setter(
                unsafe { (api.options_set_device)(options, device) },
                "device",
            )?;
        }
        if let Some(threads) = config.threads {
            check_setter(
                unsafe { (api.options_set_threads)(options, threads) },
                "threads",
            )?;
        }

        let ctx = unsafe { (api.new_with_options)(options) };
        drop(guard);
        let ctx = NonNull::new(ctx).with_context(|| {
            format!(
                "localvqe_new_with_options returned null for model {}",
                config.model_path.display()
            )
        })?;
        // From here on Drop frees the context on every error path.
        let mut runtime = Self { api, ctx, hop: 0 };

        let sample_rate = unsafe { (runtime.api.sample_rate)(ctx.as_ptr()) };
        if sample_rate != LOCALVQE_SAMPLE_RATE as c_int {
            bail!("localvqe sample rate {sample_rate} is unsupported; expected {LOCALVQE_SAMPLE_RATE}");
        }
        let hop = unsafe { (runtime.api.hop_length)(ctx.as_ptr()) };
        let fft_size = unsafe { (runtime.api.fft_size)(ctx.as_ptr()) };
        if hop <= 0 || fft_size <= 0 {
            let msg = runtime.api.last_error(ctx);
            bail!("localvqe returned invalid shape hop={hop}, fft={fft_size}: {msg}");
        }
        runtime.hop = hop as usize;
        runtime.set_noise_gate(config.noise_gate, config.noise_gate_threshold_dbfs)?;
        Ok(runtime)
    }
}

impl LocalVqeBackend for LocalVqeRuntime {
    fn hop(&self) -> usize {
        self.hop
    }

    fn process_frame(&mut self, near: &[f32], far: &[f32], out: &mut [f32]) -> Result<()> {
        let ret = unsafe {
            (self.api.process_frame_f32)(
                self.ctx.as_ptr(),
                near.as_ptr(),
                far.as_ptr(),
                self.hop as c_int,
                out.as_mut_ptr(),
            )
        };
        check_runtime(&self.api, self.ctx, ret, "process_frame_f32")
    }

    fn reset(&mut self) {
        unsafe { (self.api.reset)(self.ctx.as_ptr()) };
    }

    fn set_noise_gate(&mut self, enabled: bool, threshold_dbfs: f32) -> Result<()> {
        let ret = unsafe {
            (self.api.set_noise_gate)(
                self.ctx.as_ptr(),
                if enabled { 1 } else { 0 },
                threshold_dbfs,
            )
        };
        check_runtime(&self.api, self.ctx, ret, "set_noise_gate")
    }
}

impl Drop for LocalVqeRuntime {
    fn drop(&mut self) {
        unsafe { (self.api.free)(self.ctx.as_ptr()) };
    }
}

struct LocalVqeOptionsGuard<'a> {
    api: &'a LocalVqeApi,
    options: NonNull<c_void>,
}

impl Drop for LocalVqeOptionsGuard<'_> {
    fn drop(&mut self) {
        unsafe { (self.api.options_free)(self.options.as_ptr()) };
    }
}

struct LocalVqeApi {
    // Field order matters: the library unloads before its DLL directory is removed.
    _lib: Library,
    #[cfg(target_os = "windows")]
    _dll_dirs: Option<DllDirectoryGuard>,
    options_new: OptionsNewFn,
    options_free: OptionsFreeFn,
    options_set_model_path: OptionsSetStringFn,
    options_set_backend: OptionsSetStringFn,
    options_set_device: OptionsSetIntFn,
    options_set_threads: OptionsSetIntFn,
    new_with_options: NewWithOptionsFn,
    free: FreeFn,
    process_frame_f32: ProcessFrameF32Fn,
    last_error: LastErrorFn,
    sample_rate: IntGetterFn,
    hop_length: IntGetterFn,
    fft_size: IntGetterFn,
    reset: ResetFn,
    set_noise_gate: SetNoiseGateFn,
}

impl LocalVqeApi {
    fn load(path: &Path) -> Result<Self> {
        #[cfg(target_os = "windows")]
        let dll_dirs = localvqe_dll_directory_guard(path)?;
        let lib = unsafe { Library::new(path) }
            .with_context(|| format!("failed to open localvqe library {}", path.display()))?;
        unsafe {
            Ok(Self {
                options_new: symbol(&lib, b"localvqe_options_new\0")?,
                options_free: symbol(&lib, b"localvqe_options_free\0")?,
                options_set_model_path: symbol(&lib, b"localvqe_options_set_model_path\0")?,
                options_set_backend: symbol(&lib, b"localvqe_options_set_backend\0")?,
                options_set_device: symbol(&lib, b"localvqe_options_set_device\0")?,
                options_set_threads: symbol(&lib, b"localvqe_options_set_threads\0")?,
                new_with_options: symbol(&lib, b"localvqe_new_with_options\0")?,
                free: symbol(&lib, b"localvqe_free\0")?,
                process_frame_f32: symbol(&lib, b"localvqe_process_frame_f32\0")?,
                last_error: symbol(&lib, b"localvqe_last_error\0")?,
                sample_rate: symbol(&lib, b"localvqe_sample_rate\0")?,
                hop_length: symbol(&lib, b"localvqe_hop_length\0")?,
                fft_size: symbol(&lib, b"localvqe_fft_size\0")?,
                reset: symbol(&lib, b"localvqe_reset\0")?,
                set_noise_gate: symbol(&lib, b"localvqe_set_noise_gate\0")?,
                _lib: lib,
                #[cfg(target_os = "windows")]
                _dll_dirs: dll_dirs,
            })
        }
    }

    fn last_error(&self, ctx: NonNull<c_void>) -> String {
        let ptr = unsafe { (self.last_error)(ctx.as_ptr()) };
        if ptr.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}

unsafe fn symbol<T: Copy>(lib: &Library, name: &[u8]) -> Result<T> {
    Ok(*lib.get::<T>(name).with_context(|| {
        format!(
            "missing symbol {}",
            String::from_utf8_lossy(name).trim_end_matches('\0')
        )
    })?)
}

#[cfg(target_os = "windows")]
fn localvqe_dll_directory_guard(path: &Path) -> Result<Option<DllDirectoryGuard>> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(None);
    };
    let dir = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()
            .with_context(|| format!("failed to resolve current directory for {}", path.display()))?
            .join(parent)
    };
    DllDirectoryGuard::new(&[dir]).map(Some)
}

fn required_path(params: &toml::Table, key: &str) -> Result<Option<PathBuf>> {
    match params.get(key).and_then(|v| v.as_str()) {
        Some(value) if !value.trim().is_empty() => Ok(Some(PathBuf::from(value))),
        _ => bail!("localvqe {key} path is required"),
    }
}

fn optional_path(params: &toml::Table, key: &str) -> Option<PathBuf> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
}

fn optional_string(params: &toml::Table, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn optional_bool(params: &toml::Table, key: &str) -> Option<bool> {
    params.get(key).and_then(|v| v.as_bool())
}

fn optional_i32(params: &toml::Table, key: &str) -> Result<Option<i32>> {
    match params.get(key) {
        Some(toml::Value::Integer(v)) => {
            let v = i32::try_from(*v)
                .with_context(|| format!("localvqe {key} is outside i32 range"))?;
            Ok(Some(v))
        }
        Some(_) => bail!("localvqe {key} must be an integer"),
        None => Ok(None),
    }
}

fn optional_f32(params: &toml::Table, key: &str) -> Result<Option<f32>> {
    match params.get(key) {
        Some(v @ (toml::Value::Float(_) | toml::Value::Integer(_))) => {
            Ok(Some(toml_value_to_f32(v, key)?))
        }
        Some(_) => bail!("localvqe {key} must be a number"),
        None => Ok(None),
    }
}

fn toml_value_to_f32(value: &toml::Value, key: &str) -> Result<f32> {
    let f = match value {
        toml::Value::Float(v) => *v,
        toml::Value::Integer(v) => *v as f64,
        _ => bail!("localvqe {key} must be a number"),
    };
    if !f.is_finite() {
        bail!("localvqe {key} must be finite");
    }
    Ok(f as f32)
}

fn library_candidates(configured: Option<&Path>) -> Vec<PathBuf> {
    if let Some(path) = configured {
        if path.is_absolute() {
            return vec![path.to_path_buf()];
        }
        let mut candidates = vec![path.to_path_buf()];
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                push_unique_path(&mut candidates, dir.join(path));
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            push_unique_path(&mut candidates, cwd.join(path));
        }
        return candidates;
    }

    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            push_default_library_candidates(&mut candidates, dir);
            push_default_library_candidates(&mut candidates, &dir.join("localvqe"));
        }
    }
    candidates
}

fn push_default_library_candidates(candidates: &mut Vec<PathBuf>, dir: &Path) {
    for name in DEFAULT_LIBRARY_NAMES {
        push_unique_path(candidates, dir.join(name));
    }
}

fn push_unique_path(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|existing| existing == &path) {
        candidates.push(path);
    }
}

fn cstring_path(path: &Path) -> Result<CString> {
    cstring(path.to_string_lossy())
}

fn cstring<S: AsRef<str>>(value: S) -> Result<CString> {
    CString::new(value.as_ref()).context("localvqe string contains an interior NUL byte")
}

fn check_setter(ret: c_int, field: &str) -> Result<()> {
    if ret == 0 {
        Ok(())
    } else {
        bail!("localvqe option setter {field} failed with code {ret}");
    }
}

fn check_runtime(api: &LocalVqeApi, ctx: NonNull<c_void>, ret: c_int, call: &str) -> Result<()> {
    if ret == 0 {
        Ok(())
    } else {
        let error = api.last_error(ctx);
        bail!("localvqe {call} failed with code {ret}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    type NoiseGateState = Arc<Mutex<(bool, f32)>>;

    struct ScriptedBackend {
        hop: usize,
        fail_calls: VecDeque<bool>,
        fail_noise_gate_calls: VecDeque<bool>,
        noise_gate_state: NoiseGateState,
        resets: Arc<AtomicUsize>,
    }

    impl LocalVqeBackend for ScriptedBackend {
        fn hop(&self) -> usize {
            self.hop
        }

        fn process_frame(&mut self, near: &[f32], _far: &[f32], out: &mut [f32]) -> Result<()> {
            if self.fail_calls.pop_front().unwrap_or(false) {
                bail!("scripted localvqe native failure");
            }
            out.copy_from_slice(near);
            Ok(())
        }

        fn reset(&mut self) {
            self.resets.fetch_add(1, Ordering::SeqCst);
        }

        fn set_noise_gate(&mut self, enabled: bool, threshold_dbfs: f32) -> Result<()> {
            if self.fail_noise_gate_calls.pop_front().unwrap_or(false) {
                bail!("scripted localvqe noise gate failure");
            }
            *self.noise_gate_state.lock().unwrap() = (enabled, threshold_dbfs);
            Ok(())
        }
    }

    fn scripted_processor(
        hop: usize,
        fail_calls: impl IntoIterator<Item = bool>,
    ) -> (LocalVqe, Arc<AtomicUsize>) {
        let (processor, resets, _) =
            scripted_processor_with_noise_gate_failures(hop, fail_calls, []);
        (processor, resets)
    }

    fn scripted_processor_with_noise_gate_failures(
        hop: usize,
        fail_calls: impl IntoIterator<Item = bool>,
        fail_noise_gate_calls: impl IntoIterator<Item = bool>,
    ) -> (LocalVqe, Arc<AtomicUsize>, NoiseGateState) {
        let resets = Arc::new(AtomicUsize::new(0));
        let noise_gate_state = Arc::new(Mutex::new((false, DEFAULT_NOISE_GATE_THRESHOLD_DBFS)));
        let mut processor = LocalVqe::new();
        processor.frame_out.resize(hop, 0.0);
        processor.runtime = Some(Box::new(ScriptedBackend {
            hop,
            fail_calls: fail_calls.into_iter().collect(),
            fail_noise_gate_calls: fail_noise_gate_calls.into_iter().collect(),
            noise_gate_state: noise_gate_state.clone(),
            resets: resets.clone(),
        }));
        (processor, resets, noise_gate_state)
    }

    fn process_block(processor: &mut LocalVqe, value: f32, hop: usize) -> Vec<f32> {
        let near = vec![value; hop];
        let far = vec![0.0; hop];
        let mut out = vec![0.0; hop];
        processor.process(&near, &far, &mut out, hop as u32);
        out
    }

    #[test]
    fn configure_requires_model_path() {
        let mut processor = LocalVqe::new();
        let err = processor.configure(&toml::Table::new()).unwrap_err();
        assert!(err.to_string().contains("model"));
    }

    #[test]
    fn configure_reports_missing_library_path() {
        let model = unique_temp_model_path();
        std::fs::write(&model, []).unwrap();

        let mut params = toml::Table::new();
        params.insert(
            "model".into(),
            toml::Value::String(model.to_string_lossy().into_owned()),
        );
        params.insert(
            "library".into(),
            toml::Value::String("/definitely/missing/localvqe.dll".into()),
        );

        let mut processor = LocalVqe::new();
        let err = processor.configure(&params).unwrap_err();
        let _ = std::fs::remove_file(&model);
        assert!(err.to_string().contains("localvqe"));
        assert!(err.to_string().contains("library"));
    }

    #[test]
    fn reset_stream_state_keeps_reusable_frame_buffer() {
        let mut processor = LocalVqe::new();
        processor.near_buffer.extend([1.0, 2.0]);
        processor.far_buffer.extend([3.0, 4.0]);
        processor.out_queue.extend([5.0, 6.0]);
        processor.frame_out.resize(160, 0.0);

        processor.reset_stream_state();

        assert!(processor.near_buffer.is_empty());
        assert!(processor.far_buffer.is_empty());
        assert!(processor.out_queue.is_empty());
        assert_eq!(processor.frame_out.len(), 160);
    }

    #[test]
    fn continuous_native_errors_passthrough_and_keep_queues_bounded() {
        let hop = 4;
        let (mut processor, resets) = scripted_processor(hop, [true; 16]);

        for block in 0..16 {
            let value = block as f32 + 1.0;
            assert_eq!(process_block(&mut processor, value, hop), vec![value; hop]);
            assert!(processor.near_buffer.len() < hop);
            assert!(processor.far_buffer.len() < hop);
            assert!(processor.out_queue.len() <= hop * 2);
            assert!(!processor.started);
            assert!(processor.recovering);
        }

        let stats = processor.stats();
        assert_eq!(stats.runtime_error_count, 16);
        assert!(stats
            .last_backend_error
            .as_deref()
            .is_some_and(|error| error.contains("scripted localvqe native failure")));
        assert_eq!(resets.load(Ordering::SeqCst), 16);
    }

    #[test]
    fn transient_error_recovers_without_pre_error_audio() {
        let hop = 4;
        let (mut processor, resets) = scripted_processor(hop, [true, false, false]);

        assert_eq!(process_block(&mut processor, 1.0, hop), vec![1.0; hop]);
        assert_eq!(process_block(&mut processor, 2.0, hop), vec![2.0; hop]);
        let recovered = process_block(&mut processor, 3.0, hop);

        assert_eq!(recovered, vec![2.0; hop]);
        assert!(!recovered.contains(&1.0));
        assert!(processor.started);
        assert!(!processor.recovering);
        assert_eq!(processor.stats().runtime_error_count, 1);
        assert_eq!(resets.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn repeated_recovery_discards_each_failed_generation() {
        let hop = 4;
        let (mut processor, resets) =
            scripted_processor(hop, [true, false, false, true, false, false]);

        assert_eq!(process_block(&mut processor, 1.0, hop), vec![1.0; hop]);
        assert_eq!(process_block(&mut processor, 2.0, hop), vec![2.0; hop]);
        assert_eq!(process_block(&mut processor, 3.0, hop), vec![2.0; hop]);
        assert_eq!(process_block(&mut processor, 4.0, hop), vec![4.0; hop]);
        assert_eq!(process_block(&mut processor, 5.0, hop), vec![5.0; hop]);
        let recovered = process_block(&mut processor, 6.0, hop);

        assert_eq!(recovered, vec![5.0; hop]);
        assert!(!recovered.contains(&3.0));
        assert_eq!(processor.stats().runtime_error_count, 2);
        assert_eq!(resets.load(Ordering::SeqCst), 2);
        assert!(processor.near_buffer.len() < hop);
        assert!(processor.far_buffer.len() < hop);
        assert!(processor.out_queue.len() <= hop * 2);
    }

    #[test]
    fn runtime_params_update_localvqe_noise_gate_tuning() {
        let mut processor = LocalVqe::new();

        assert!(processor
            .set_runtime_param("noise_gate", &toml::Value::Boolean(true))
            .unwrap());
        assert!(processor
            .set_runtime_param("noise_gate_threshold_dbfs", &toml::Value::Float(-40.5))
            .unwrap());
        assert!(!processor
            .set_runtime_param("threads", &toml::Value::Integer(2))
            .unwrap());

        assert!(processor.noise_gate);
        assert_eq!(processor.noise_gate_threshold_dbfs, -40.5);
    }

    #[test]
    fn runtime_noise_gate_setter_failure_keeps_previous_gate_config() {
        let (mut processor, resets, backend_state) =
            scripted_processor_with_noise_gate_failures(4, [], [false, true]);
        processor
            .set_runtime_param("noise_gate_threshold_dbfs", &toml::Value::Float(-40.5))
            .unwrap();

        let err = processor
            .set_runtime_param("noise_gate", &toml::Value::Boolean(true))
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("scripted localvqe noise gate failure"));
        assert!(!processor.noise_gate);
        assert_eq!(processor.noise_gate_threshold_dbfs, -40.5);
        assert_eq!(*backend_state.lock().unwrap(), (false, -40.5));
        assert_eq!(resets.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn runtime_noise_gate_setter_failure_keeps_previous_threshold_config() {
        let (mut processor, resets, backend_state) =
            scripted_processor_with_noise_gate_failures(4, [], [false, true]);
        processor
            .set_runtime_param("noise_gate", &toml::Value::Boolean(true))
            .unwrap();

        let err = processor
            .set_runtime_param("noise_gate_threshold_dbfs", &toml::Value::Float(-40.5))
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("scripted localvqe noise gate failure"));
        assert!(processor.noise_gate);
        assert_eq!(
            processor.noise_gate_threshold_dbfs,
            DEFAULT_NOISE_GATE_THRESHOLD_DBFS
        );
        assert_eq!(
            *backend_state.lock().unwrap(),
            (true, DEFAULT_NOISE_GATE_THRESHOLD_DBFS)
        );
        assert_eq!(resets.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn runtime_params_validate_localvqe_noise_gate_types() {
        let mut processor = LocalVqe::new();

        assert!(processor
            .set_runtime_param("noise_gate", &toml::Value::String("true".into()))
            .unwrap_err()
            .to_string()
            .contains("boolean"));
        assert!(processor
            .set_runtime_param(
                "noise_gate_threshold_dbfs",
                &toml::Value::String("-40".into()),
            )
            .unwrap_err()
            .to_string()
            .contains("number"));
    }

    #[test]
    fn default_library_candidates_exclude_current_directory() {
        let cwd = std::env::current_dir().unwrap();
        let candidates = library_candidates(None);

        for name in DEFAULT_LIBRARY_NAMES {
            assert!(
                !candidates.contains(&cwd.join(name)),
                "default candidates should not include CWD library {name}: {candidates:?}"
            );
            assert!(
                !candidates.contains(&cwd.join("localvqe").join(name)),
                "default candidates should not include CWD/localvqe library {name}: {candidates:?}"
            );
        }
    }

    #[test]
    fn explicit_relative_library_keeps_cwd_candidate() {
        let relative = Path::new(DEFAULT_LIBRARY_NAMES[0]);
        let cwd = std::env::current_dir().unwrap();
        let candidates = library_candidates(Some(relative));

        assert!(candidates.contains(&relative.to_path_buf()));
        assert!(candidates.contains(&cwd.join(relative)));
    }

    #[test]
    #[ignore = "requires ECHOLESS_LOCALVQE_LIBRARY and ECHOLESS_LOCALVQE_MODEL"]
    fn localvqe_ffi_smoke() {
        let library = std::env::var("ECHOLESS_LOCALVQE_LIBRARY").unwrap();
        let model = std::env::var("ECHOLESS_LOCALVQE_MODEL").unwrap();

        let mut params = toml::Table::new();
        params.insert("model".into(), toml::Value::String(model));
        params.insert("library".into(), toml::Value::String(library));
        params.insert("threads".into(), toml::Value::Integer(1));
        params.insert("noise_gate".into(), toml::Value::Boolean(true));

        let mut processor = LocalVqe::new();
        processor.configure(&params).unwrap();

        let frames = 160usize;
        let mut saw_finite_output = false;
        for block in 0..8 {
            let near = synthetic_sine(frames, block);
            let far = vec![0.0; frames];
            let mut out = vec![0.0; frames];
            processor.process(&near, &far, &mut out, frames as u32);
            assert!(out.iter().all(|v| v.is_finite()));
            saw_finite_output |= out.iter().any(|v| v.abs() > 0.0);
        }
        assert!(saw_finite_output);
    }

    fn synthetic_sine(frames: usize, block: usize) -> Vec<f32> {
        let offset = block * frames;
        (0..frames)
            .map(|i| {
                let phase = ((offset + i) as f32 * 440.0 * std::f32::consts::TAU)
                    / LOCALVQE_SAMPLE_RATE as f32;
                0.05 * phase.sin()
            })
            .collect()
    }

    fn unique_temp_model_path() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("echoless-localvqe-test-{nanos}.gguf"))
    }
}
