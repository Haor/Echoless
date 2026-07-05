use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::Manager;

pub(crate) fn transient_config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("runtime-configs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn transient_config_path(dir: &Path, label: &str, attempt: usize) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    dir.join(format!(
        "echoless-{label}-{}-{nanos}-{attempt}.toml",
        std::process::id()
    ))
}

pub(crate) fn write_toml_create_new(path: &Path, toml_text: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("创建配置文件失败: {}: {e}", path.display()))?;
    if let Err(err) = file.write_all(toml_text.as_bytes()) {
        drop(file);
        cleanup_run_config(path);
        return Err(format!("写入配置文件失败: {}: {err}", path.display()));
    }
    if let Err(err) = file.flush() {
        drop(file);
        cleanup_run_config(path);
        return Err(format!("刷新配置文件失败: {}: {err}", path.display()));
    }
    Ok(())
}

pub(crate) fn write_transient_config_toml(
    dir: &Path,
    label: &str,
    toml_text: &str,
) -> Result<PathBuf, String> {
    for attempt in 0..16 {
        let path = transient_config_path(dir, label, attempt);
        match write_toml_create_new(&path, toml_text) {
            Ok(()) => return Ok(path),
            Err(err) if path.exists() => {
                if attempt == 15 {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }
    }
    Err("无法创建唯一配置文件".to_string())
}

pub(crate) fn cleanup_run_config(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

/// 在系统默认浏览器打开外部链接(驱动 / VC++ 下载页)。
#[tauri::command]
pub(crate) fn open_url(url: String) -> Result<(), String> {
    let url = validate_browser_url(&url)?;
    let (prog, args) = browser_open_command(&url);
    Command::new(prog)
        .args(&args)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn validate_browser_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL 不能为空".to_string());
    }
    if trimmed
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("URL 不能包含空白或控制字符".to_string());
    }
    // 系统设置深链只允许跳到隐私面板;不要把整个 scheme 当作通用白名单。
    if trimmed.starts_with("x-apple.systempreferences:") {
        if !trimmed.starts_with("x-apple.systempreferences:com.apple.preference.security?Privacy_")
        {
            return Err("仅允许打开系统隐私设置面板".to_string());
        }
        return Ok(trimmed.to_string());
    }
    if !trimmed.starts_with("https://") {
        return Err("仅允许打开 https URL".to_string());
    }

    let host = https_url_host(trimmed).ok_or_else(|| "URL 缺少主机名".to_string())?;
    if !is_allowed_browser_host(&host) {
        return Err("URL 主机不在允许列表".to_string());
    }

    Ok(trimmed.to_string())
}

fn https_url_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    if host_end == 0 {
        return None;
    }
    let host_port = &rest[..host_end];
    let host = host_port
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(host_port);
    let host = host
        .split_once(':')
        .map(|(host, _)| host)
        .unwrap_or(host)
        .trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn is_allowed_browser_host(host: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "aka.ms",
        "developer.nvidia.com",
        "existential.audio",
        "github.com",
        "huggingface.co",
        "learn.microsoft.com",
        "nvidia.com",
        "vb-audio.com",
    ];
    ALLOWED
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

pub(crate) fn browser_open_command(url: &str) -> (&'static str, Vec<String>) {
    #[cfg(target_os = "macos")]
    return ("open", vec![url.to_string()]);
    #[cfg(target_os = "windows")]
    return (
        "rundll32.exe",
        vec!["url.dll,FileProtocolHandler".to_string(), url.to_string()],
    );
    #[cfg(target_os = "linux")]
    return ("xdg-open", vec![url.to_string()]);
}

/// 诊断录制默认目录(绝对路径,session-* 会写在其下)。
#[tauri::command]
pub(crate) fn default_diag_dir() -> String {
    let (base, _) = echoless_paths::brand_data_root();
    base.join("diagnostics").to_string_lossy().to_string()
}

/// 在系统文件管理器里打开目录。
#[tauri::command]
pub(crate) fn open_path(path: String) -> Result<(), String> {
    let p = validate_open_path(&path)?;
    #[cfg(target_os = "macos")]
    let prog = "open";
    #[cfg(target_os = "windows")]
    let prog = "explorer";
    #[cfg(target_os = "linux")]
    let prog = "xdg-open";
    Command::new(prog)
        .arg(&p)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn validate_open_path(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let canonical = p
        .canonicalize()
        .map_err(|e| format!("目录不存在或不可访问: {e}"))?;
    if !canonical.is_dir() {
        return Err("只能打开目录".to_string());
    }
    let allowed_roots = allowed_open_path_roots();
    if allowed_roots
        .iter()
        .any(|root| canonical == *root || canonical.starts_with(root))
    {
        return Ok(canonical);
    }
    Err("目录不在 Echoless 允许范围内".to_string())
}

fn allowed_open_path_roots() -> Vec<PathBuf> {
    let (brand_root, _) = echoless_paths::brand_data_root();
    [
        brand_root.clone(),
        brand_root.join("diagnostics"),
        crate::localvqe_models_dir_path(),
    ]
    .into_iter()
    .filter_map(|path| path.canonicalize().ok())
    .collect()
}
