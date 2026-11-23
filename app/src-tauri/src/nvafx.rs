use serde_json::Value;
use tauri::Emitter;

use crate::bin_resolve::echoless_command;
use crate::proc::{
    command_output_with_timeout, command_status_error, parse_json_output, parse_jsonl_line_event,
    run_json_async, run_json_blocking, JsonlLineEvent, JSON_COMMAND_TIMEOUT,
    NVAFX_DOWNLOAD_TIMEOUT, NVAFX_INSTALL_TIMEOUT,
};

/// NVIDIA AFX / RTX AEC 引擎就绪探针。
/// 返回 { ok, report: { runtime_dir, runtime_dir_source, gpus[], selected_arch, checks[] } }。
/// macOS/Linux 上后端会返回 ok=false + platform unsupported 检查项(诚实降级)。
#[tauri::command]
pub(crate) async fn nvafx_doctor(app: tauri::AppHandle) -> Result<Value, String> {
    let args: Vec<String> = vec!["nvafx".into(), "doctor".into(), "--json".into()];
    run_json_async(app, args, JSON_COMMAND_TIMEOUT, "nvafx doctor").await
}

/// NVAFX runtime 安装:校验+解压 common zip 与按架构选的 model zip,然后回传安装后的 doctor 报告。
/// 实际只在 Windows 生效(CLI `nvafx install` 在非 Windows 会 bail);mac/Linux 上返回 Err。
#[tauri::command]
pub(crate) async fn nvafx_install(
    app: tauri::AppHandle,
    common_zip: String,
    model_zip: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut command = echoless_command(Some(&app))?;
        command.args(["nvafx", "install", "--common-zip"]);
        command.arg(&common_zip).arg("--model-zip").arg(&model_zip);
        let out = command_output_with_timeout(
            &mut command,
            NVAFX_INSTALL_TIMEOUT,
            "nvafx install",
            |_| false,
        )?;
        if !out.status.success() {
            return Err(command_status_error("nvafx install", &out));
        }

        // 安装后用 doctor --json 验证,回传报告供前端重算状态。
        run_json_blocking(
            Some(&app),
            &["nvafx", "doctor", "--json"],
            JSON_COMMAND_TIMEOUT,
            "nvafx doctor",
        )
    })
    .await
    .map_err(|e| format!("nvafx install task join failed: {e}"))?
}

/// 从公共 GitHub release 下载 common+架构 model zip,然后安装并回传 doctor。
/// shell `echoless nvafx download-install --json`;stdout 是最终的 `{ok, report}` doctor JSON。
#[tauri::command]
pub(crate) async fn nvafx_download_install(app: tauri::AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let label = "nvafx download-install";
        let mut command = echoless_command(Some(&app))?;
        command.args(["nvafx", "download-install", "--json"]);
        // stderr carries nvafx_download_progress JSONL (forwarded as progress
        // events) and human-readable log lines (forwarded as log events).
        let events = app.clone();
        let out = command_output_with_timeout(
            &mut command,
            NVAFX_DOWNLOAD_TIMEOUT,
            label,
            move |line| match parse_jsonl_line_event(line) {
                JsonlLineEvent::Json(value)
                    if value.get("event").and_then(Value::as_str)
                        == Some("nvafx_download_progress") =>
                {
                    let _ = events.emit("echoless://nvafx-progress", value);
                    true
                }
                _ => {
                    let _ = events.emit("echoless://log", line.to_string());
                    false
                }
            },
        )?;
        parse_json_output(label, out)
    })
    .await
    .map_err(|e| format!("nvafx download-install task join failed: {e}"))?
}
