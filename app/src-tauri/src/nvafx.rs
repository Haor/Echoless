use serde_json::Value;

use crate::bin_resolve::echoless_command;
use crate::proc::{
    command_output_with_timeout, command_status_error, run_json_async, run_json_blocking,
    JSON_COMMAND_TIMEOUT, NVAFX_INSTALL_TIMEOUT,
};

/// NVIDIA AFX / RTX AEC 引擎就绪探针。
/// 返回 { ok, report: { runtime_dir, runtime_dir_source, gpus[], selected_arch, checks[] } }。
/// macOS/Linux 上后端会返回 ok=false + platform unsupported 检查项(诚实降级)。
#[tauri::command]
pub(crate) async fn nvafx_doctor(
    app: tauri::AppHandle,
    runtime_dir: Option<String>,
) -> Result<Value, String> {
    let mut args: Vec<String> = vec!["nvafx".into(), "doctor".into(), "--json".into()];
    if let Some(dir) = runtime_dir {
        if !dir.is_empty() {
            args.push("--runtime-dir".into());
            args.push(dir);
        }
    }
    run_json_async(app, args, JSON_COMMAND_TIMEOUT, "nvafx doctor").await
}

/// NVAFX runtime 安装:校验+解压 common zip 与按架构选的 model zip,然后回传安装后的 doctor 报告。
/// 实际只在 Windows 生效(CLI `nvafx install` 在非 Windows 会 bail);mac/Linux 上返回 Err。
#[tauri::command]
pub(crate) async fn nvafx_install(
    app: tauri::AppHandle,
    common_zip: String,
    model_zip: String,
    runtime_dir: Option<String>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let rdir = runtime_dir.filter(|d| !d.is_empty());
        let mut args: Vec<String> = vec![
            "nvafx".into(),
            "install".into(),
            "--common-zip".into(),
            common_zip,
            "--model-zip".into(),
            model_zip,
        ];
        if let Some(dir) = rdir.as_deref() {
            args.push("--runtime-dir".into());
            args.push(dir.to_string());
        }
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut command = echoless_command(Some(&app))?;
        command.args(&arg_refs);
        let out =
            command_output_with_timeout(&mut command, NVAFX_INSTALL_TIMEOUT, "nvafx install")?;
        if !out.status.success() {
            return Err(command_status_error("nvafx install", &out));
        }

        // 安装后用 doctor --json 验证,回传报告供前端重算状态。
        let mut dargs: Vec<String> = vec!["nvafx".into(), "doctor".into(), "--json".into()];
        if let Some(dir) = rdir.as_deref() {
            dargs.push("--runtime-dir".into());
            dargs.push(dir.to_string());
        }
        let darg_refs: Vec<&str> = dargs.iter().map(String::as_str).collect();
        run_json_blocking(Some(&app), &darg_refs, JSON_COMMAND_TIMEOUT, "nvafx doctor")
    })
    .await
    .map_err(|e| format!("nvafx install task join failed: {e}"))?
}

/// 从公共 GitHub release 下载 common+架构 model zip,然后安装并回传 doctor。
/// shell `echoless nvafx download-install [--runtime-dir D] --json`;该子命令需打印
/// `{ok, report}` doctor JSON 到 stdout。后端(Codex)实现该子命令后此处即生效;
/// 未实现前 CLI 会非 0 退出,错误经 stderr 透传给前端。
#[tauri::command]
pub(crate) async fn nvafx_download_install(
    app: tauri::AppHandle,
    runtime_dir: Option<String>,
) -> Result<Value, String> {
    let rdir = runtime_dir.filter(|d| !d.is_empty());
    let mut args: Vec<String> = vec!["nvafx".into(), "download-install".into(), "--json".into()];
    if let Some(dir) = rdir {
        args.push("--runtime-dir".into());
        args.push(dir);
    }
    run_json_async(app, args, NVAFX_INSTALL_TIMEOUT, "nvafx download-install").await
}
