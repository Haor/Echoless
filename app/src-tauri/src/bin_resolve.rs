use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::Manager;

const TAURI_TARGET_TRIPLE: &str = env!("TAURI_ENV_TARGET_TRIPLE");
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn exe_suffix() -> &'static str {
    if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    }
}

fn push_file_candidate(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|existing| existing == &path) {
        candidates.push(path);
    }
}

fn resource_path(app: Option<&tauri::AppHandle>, relative: &str) -> Option<PathBuf> {
    app.and_then(|handle| {
        handle
            .path()
            .resolve(relative, tauri::path::BaseDirectory::Resource)
            .ok()
    })
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// 解析 echoless CLI 路径。顺序刻意区分 dev / Tauri build / packaged app:
///   1. ECHOLESS_BIN(开发者显式覆盖);
///   2. Tauri externalBin 被 tauri-build 复制到当前可执行文件旁的 `echoless`;
///   3. Tauri Resource 目录中的候选;
///   4. dev 生成的 `src-tauri/binaries/echoless-<target-triple>`;
///   5. root target release/debug 回退。
pub(crate) fn echoless_bin(app: Option<&tauri::AppHandle>) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(p) = std::env::var("ECHOLESS_BIN") {
        push_file_candidate(&mut candidates, PathBuf::from(p));
    }

    let exe_name = format!("echoless{}", exe_suffix());
    if let Some(dir) = current_exe_dir() {
        push_file_candidate(&mut candidates, dir.join(&exe_name));
        push_file_candidate(
            &mut candidates,
            dir.join(format!("echoless-{}{}", TAURI_TARGET_TRIPLE, exe_suffix())),
        );
    }

    for rel in [
        format!("echoless{}", exe_suffix()),
        format!("binaries/echoless{}", exe_suffix()),
        format!("binaries/echoless-{}{}", TAURI_TARGET_TRIPLE, exe_suffix()),
    ] {
        if let Some(path) = resource_path(app, &rel) {
            push_file_candidate(&mut candidates, path);
        }
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")); // .../echoless/app/src-tauri
    push_file_candidate(
        &mut candidates,
        manifest
            .join("binaries")
            .join(format!("echoless-{}{}", TAURI_TARGET_TRIPLE, exe_suffix())),
    );
    push_file_candidate(
        &mut candidates,
        manifest
            .join("../../target/release")
            .join(format!("echoless{}", exe_suffix())),
    );
    push_file_candidate(
        &mut candidates,
        manifest
            .join("../../target/debug")
            .join(format!("echoless{}", exe_suffix())),
    );

    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            format!(
                "echoless CLI not found; tried: {}",
                candidates
                    .iter()
                    .map(|p| p.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        })
}

pub(crate) fn process_tap_helper_bin(
    app: Option<&tauri::AppHandle>,
    cli: &Path,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(p) = std::env::var("ECHOLESS_PROCESS_TAP_HELPER") {
        push_file_candidate(&mut candidates, PathBuf::from(p));
    }

    if let Some(dir) = cli.parent() {
        for name in ["echoless-process-tap-poc", "echoless-process-tap"] {
            push_file_candidate(&mut candidates, dir.join(name));
        }
    }

    for rel in [
        "resources/helpers/echoless-process-tap-poc",
        "resources/helpers/echoless-process-tap",
    ] {
        if let Some(path) = resource_path(app, rel) {
            push_file_candidate(&mut candidates, path);
        }
    }

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")); // .../echoless/app/src-tauri
    for base in manifest.ancestors() {
        let candidate = base
            .join("tools")
            .join("macos-process-tap-poc")
            .join(".build")
            .join("echoless-process-tap-poc");
        push_file_candidate(&mut candidates, candidate);
    }

    candidates.into_iter().find(|path| path.is_file())
}

pub(crate) fn find_localvqe_library_in_dir(dir: &Path) -> Option<PathBuf> {
    let mut matches = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let is_match = if cfg!(target_os = "windows") {
                name.eq_ignore_ascii_case("localvqe.dll")
            } else if cfg!(target_os = "macos") {
                name.starts_with("liblocalvqe") && name.ends_with(".dylib")
            } else {
                name.starts_with("liblocalvqe") && has_shared_object_suffix(name)
            };
            if is_match {
                matches.push(path);
            }
        }
    }
    matches.sort();
    matches.into_iter().next()
}

fn has_shared_object_suffix(name: &str) -> bool {
    if name.ends_with(".so") {
        return true;
    }
    let Some((_, version)) = name.rsplit_once(".so.") else {
        return false;
    };
    !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

pub(crate) fn localvqe_library_path(app: Option<&tauri::AppHandle>, cli: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(p) = std::env::var("ECHOLESS_LOCALVQE_LIBRARY") {
        push_file_candidate(&mut candidates, PathBuf::from(p));
    }

    // 产品决策(2026-07-05 修正):native runtime 随包分发,只有模型走 HF 下载。
    // 打包 Resource 目录 → dev 的 src-tauri/resources → 品牌数据根(下载兜底)。
    if let Some(resource_native) = resource_path(app, "resources/localvqe/native") {
        if let Some(path) = find_localvqe_library_in_dir(&resource_native) {
            push_file_candidate(&mut candidates, path);
        }
    }
    let manifest_native = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("localvqe")
        .join("native");
    if let Some(path) = find_localvqe_library_in_dir(&manifest_native) {
        push_file_candidate(&mut candidates, path);
    }

    if let Some(path) = find_localvqe_library_in_dir(&crate::localvqe::localvqe_native_dir_path()) {
        push_file_candidate(&mut candidates, path);
    }

    if let Some(dir) = cli.parent() {
        if let Some(path) = find_localvqe_library_in_dir(dir) {
            push_file_candidate(&mut candidates, path);
        }
        let localvqe_dir = dir.join("localvqe");
        if let Some(path) = find_localvqe_library_in_dir(&localvqe_dir) {
            push_file_candidate(&mut candidates, path);
        }
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn prepend_env_path(command: &mut Command, key: &str, dir: &Path) {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os(key) {
        paths.extend(std::env::split_paths(&existing));
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        command.env(key, joined);
    }
}

pub(crate) fn suppress_child_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

pub(crate) fn echoless_command(app: Option<&tauri::AppHandle>) -> Result<Command, String> {
    let cli = echoless_bin(app)?;
    let mut command = Command::new(&cli);
    if let Some(helper) = process_tap_helper_bin(app, &cli) {
        command.env("ECHOLESS_PROCESS_TAP_HELPER", helper);
    }
    if let Some(library) = localvqe_library_path(app, &cli) {
        if let Some(dir) = library.parent() {
            prepend_env_path(&mut command, "PATH", dir);
            prepend_env_path(&mut command, "LD_LIBRARY_PATH", dir);
            prepend_env_path(&mut command, "DYLD_LIBRARY_PATH", dir);
            prepend_env_path(&mut command, "DYLD_FALLBACK_LIBRARY_PATH", dir);
        }
        command.env("ECHOLESS_LOCALVQE_LIBRARY", library);
    }
    Ok(command)
}
