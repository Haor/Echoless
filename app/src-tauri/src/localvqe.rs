use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::Manager;

use crate::bin_resolve::{echoless_bin, localvqe_library_path, process_tap_helper_bin};
use crate::proc::{command_output_with_timeout, command_status_error, MODEL_DOWNLOAD_TIMEOUT};

// ---- LocalVQE model/native management: brand data root + HF downloads ----
// revision 跟 main:完整性由每文件 sha256 pin 保证,新上传的文件无需改代码即可下载。
// (曾 pin 具体 commit,但该 rev 在 HF 上不存在导致下载全挂。)
const LOCALVQE_HF_REVISION: &str = "main";

#[derive(Clone, Copy)]
pub(crate) struct LocalVqeModelPin {
    pub(crate) filename: &'static str,
    pub(crate) sha256: &'static str,
    pub(crate) size: u64,
}

const LOCALVQE_MODEL_PINS: &[LocalVqeModelPin] = &[
    LocalVqeModelPin {
        filename: "localvqe-v1-1.3M-f32.gguf",
        sha256: "d5eaf577449d0f920d8ee5e1042b8ddc7b6627313a042c62e2ada1b42719ab30",
        size: 5_162_720,
    },
    LocalVqeModelPin {
        filename: "localvqe-v1.2-1.3M-f32.gguf",
        sha256: "4856ecf5f522b23fb2bc5caeac81f323c0ef1c4c156a9c7d40a6adbe092ba9ce",
        size: 5_173_088,
    },
    LocalVqeModelPin {
        filename: "localvqe-v1.3-4.8M-f32.gguf",
        sha256: "c4f7912485c32cfc206c536f2f050b52513f2f613fdbc616391f6b26ab1d51ec",
        size: 19_268_160,
    },
    LocalVqeModelPin {
        filename: "localvqe-v1.4-aec-200K-f32.gguf",
        sha256: "b6e43138588a83bfe903ab5e143b4020b91c1e1629f5a575ac5855ff0003c731",
        size: 2_924_224,
    },
];

pub(crate) fn localvqe_model_pin(filename: &str) -> Option<&'static LocalVqeModelPin> {
    LOCALVQE_MODEL_PINS
        .iter()
        .find(|pin| pin.filename == filename)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("打开文件失败: {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("读取文件失败: {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_pinned_file(
    path: &Path,
    expected_sha256: &str,
    expected_size: u64,
    label: &str,
) -> Result<(), String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("读取文件信息失败: {}: {e}", path.display()))?
        .len();
    if size != expected_size {
        return Err(format!(
            "{label}大小不匹配: file={}, actual={}, expected={}",
            path.display(),
            size,
            expected_size
        ));
    }
    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(format!(
            "{label} SHA256 不匹配: file={}, actual={}, expected={}",
            path.display(),
            actual,
            expected_sha256
        ));
    }
    Ok(())
}

pub(crate) fn verify_localvqe_model_file(
    path: &Path,
    pin: &LocalVqeModelPin,
) -> Result<(), String> {
    verify_pinned_file(path, pin.sha256, pin.size, "LocalVQE 模型")
}

fn localvqe_data_dir_path() -> PathBuf {
    let (base, _) = echoless_paths::brand_data_root();
    base.join("localvqe")
}

pub(crate) fn localvqe_models_dir_path() -> PathBuf {
    localvqe_data_dir_path().join("models")
}

pub(crate) fn localvqe_native_dir_path() -> PathBuf {
    localvqe_data_dir_path().join("native")
}

fn localvqe_native_dir() -> Result<PathBuf, String> {
    let dir = localvqe_native_dir_path();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Local directory for downloaded models: <brand data root>/localvqe/models.
fn localvqe_models_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = localvqe_models_dir_path();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    migrate_legacy_localvqe_models(app, &dir);
    // User-supplied and in-app downloaded .gguf files both live here.
    let readme = dir.join("README.txt");
    if !readme.exists() {
        let _ = std::fs::write(
            &readme,
            "LocalVQE models\n\
             ===============\n\n\
             Put LocalVQE .gguf models in this folder. Models downloaded from\n\
             within Echoless also land here. Any .gguf found here is detected\n\
             automatically and can be selected on the Engine page.\n\n\
             Official models: https://huggingface.co/LocalAI-io/LocalVQE\n",
        );
    }
    Ok(dir)
}

fn migrate_legacy_localvqe_models(app: &tauri::AppHandle, dest_dir: &Path) {
    let Ok(legacy_base) = app.path().app_local_data_dir() else {
        return;
    };
    migrate_legacy_localvqe_models_from_base(&legacy_base, dest_dir);
}

pub(crate) fn migrate_legacy_localvqe_models_from_base(legacy_base: &Path, dest_dir: &Path) {
    let legacy_dir = legacy_base.join("localvqe").join("models");
    if legacy_dir == dest_dir || !legacy_dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&legacy_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("gguf") {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let dest = dest_dir.join(name);
        if dest.exists() {
            continue;
        }
        if let Err(rename_err) = std::fs::rename(&path, &dest) {
            if let Err(copy_err) =
                std::fs::copy(&path, &dest).and_then(|_| std::fs::remove_file(&path))
            {
                eprintln!(
                    "LocalVQE legacy model migration skipped: {} -> {}: rename={rename_err}; copy={copy_err}",
                    path.display(),
                    dest.display()
                );
            }
        }
    }
}

fn collect_gguf(dir: &Path) -> Vec<Value> {
    let mut models = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("gguf") {
                continue;
            }
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                models.push(serde_json::json!({
                    "filename": name,
                    "path": p.to_string_lossy(),
                    "source": "downloaded",
                }));
            }
        }
    }
    models.sort_by(|a, b| {
        a["filename"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["filename"].as_str().unwrap_or_default())
    });
    models
}

fn collect_native_files(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

/// List available LocalVQE models from the single local model directory.
#[tauri::command]
pub(crate) fn localvqe_assets(app: tauri::AppHandle) -> Result<Value, String> {
    let dir = localvqe_models_dir(&app)?;
    let models = collect_gguf(&dir);
    let native_dir = localvqe_native_dir()?;
    let cli = echoless_bin(Some(&app)).ok();
    let library = cli
        .as_deref()
        .and_then(|path| localvqe_library_path(Some(&app), path));
    let native_files = collect_native_files(&native_dir);
    let process_tap_helper = cli
        .as_deref()
        .and_then(|path| process_tap_helper_bin(Some(&app), path));
    Ok(serde_json::json!({
        "models_dir": dir.to_string_lossy(),
        "models": models,
        "native_ready": library.is_some(),
        "library_path": library.map(|p| p.to_string_lossy().to_string()),
        "native_dir": native_dir.to_string_lossy(),
        "native_files": native_files,
        "cli_path": cli.map(|p| p.to_string_lossy().to_string()),
        "process_tap_helper_path": process_tap_helper.map(|p| p.to_string_lossy().to_string()),
    }))
}

/// 从官方 HF repo 下载指定模型到本地目录,回传完整路径。用 curl(免新增依赖)。
#[tauri::command]
pub(crate) async fn download_localvqe_model(
    app: tauri::AppHandle,
    filename: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || download_localvqe_model_blocking(&app, &filename))
        .await
        .map_err(|e| format!("download LocalVQE model task join failed: {e}"))?
}

fn download_localvqe_model_blocking(
    app: &tauri::AppHandle,
    filename: &str,
) -> Result<String, String> {
    let pin =
        localvqe_model_pin(filename).ok_or_else(|| "unsupported LocalVQE model".to_string())?;
    let dir = localvqe_models_dir(app)?;
    let dest = dir.join(pin.filename);
    if dest.exists() {
        match verify_localvqe_model_file(&dest, pin) {
            Ok(()) => return Ok(dest.to_string_lossy().to_string()),
            Err(_) => {
                let _ = std::fs::remove_file(&dest);
            }
        }
    }

    let tmp = dir.join(format!("{}.part", pin.filename));
    let _ = std::fs::remove_file(&tmp);
    let url = format!(
        "https://huggingface.co/LocalAI-io/LocalVQE/resolve/{LOCALVQE_HF_REVISION}/{}",
        pin.filename
    );
    let mut curl = Command::new("curl");
    // -sS:去掉进度表(否则 curl 把整张进度表写进 stderr,报错时被原样灌进 UI)。
    curl.args(["-sSfL", "--retry", "2", "-o"])
        .arg(&tmp)
        .arg(&url);
    let out =
        command_output_with_timeout(&mut curl, MODEL_DOWNLOAD_TIMEOUT, "LocalVQE model download")?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "下载失败({url}): {}",
            command_status_error("curl", &out)
        ));
    }
    if let Err(err) = verify_localvqe_model_file(&tmp, pin) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}
