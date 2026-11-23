use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::atomic::Ordering;

use serde_json::{json, Value};
use tauri::{Emitter, Manager, State};

use crate::bin_resolve::{echoless_command, suppress_child_console};
use crate::platform::{cleanup_run_config, transient_config_dir, write_transient_config_toml};
use crate::proc::{
    begin_run, command_output_with_timeout, install_run, is_active_run, parse_json_output,
    parse_jsonl_line_event, run_control_stdin, run_json_async, take_finished_run, terminate_run,
    write_run_control, JsonlLineEvent, RunChild, RunId, RunState, PROBE_DELAY_TIMEOUT,
    VALIDATE_COMMAND_TIMEOUT,
};
use crate::tray::update_tray_tooltip;

/// 同时录 ref/mic、分析两路相对到达时差,返回 NearDelayProbeResult(含 recommended_near_delay_ms)。
/// 通常约 15 秒(首次 macOS 权限/Process Tap 启动可能更久)、会外放蜂鸣 —— 故必须先停掉
/// 主 run(probe 内部自起子进程占用设备),由前端 gating。
/// 支持 macOS / Windows / Linux(见 probe_delay.rs);不支持的平台 CLI 会非 0 退出,错误经 stderr 透传给前端。
#[tauri::command]
pub(crate) async fn probe_delay(
    app: tauri::AppHandle,
    mic: String,
    reference: String,
    output: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut command = echoless_command(Some(&app))?;
        command.args(["probe-delay", "--json"]);
        // selector 透传(含 "default",与 run 同一套解析);仅空串时省略走 CLI 内置默认。
        for (flag, value) in [
            ("--mic", &mic),
            ("--reference", &reference),
            ("--output", &output),
        ] {
            if !value.is_empty() {
                command.args([flag, value.as_str()]);
            }
        }
        // stderr carries JSONL progress lines; the frontend aligns its progress
        // lights to beep_train_start. Stdout is the final JSON result.
        let events = app.clone();
        let out = command_output_with_timeout(
            &mut command,
            PROBE_DELAY_TIMEOUT,
            "probe-delay",
            move |line| match parse_jsonl_line_event(line) {
                JsonlLineEvent::Empty => true,
                JsonlLineEvent::Json(value) => {
                    let _ = events.emit("echoless://probe-progress", value);
                    true
                }
                JsonlLineEvent::Unparsed(line) => {
                    let _ = events.emit("echoless://log", format!("unparsed probe line: {line}"));
                    false
                }
            },
        )?;
        parse_json_output("probe-delay", out)
    })
    .await
    .map_err(|e| format!("probe task join failed: {e}"))?
}

#[tauri::command]
pub(crate) async fn validate_config(
    app: tauri::AppHandle,
    toml_text: String,
) -> Result<Value, String> {
    let dir = transient_config_dir(&app)?;
    let path = write_transient_config_toml(&dir, "validate", &toml_text)?;
    let config_arg = path.to_string_lossy().to_string();
    let result = run_json_async(
        app,
        vec![
            "config".into(),
            "validate".into(),
            "--config".into(),
            config_arg,
            "--json".into(),
        ],
        VALIDATE_COMMAND_TIMEOUT,
        "config validate",
    )
    .await;
    cleanup_run_config(&path);
    result
}

/// Runs on the main thread after the stdout of `run_id` closes. A run that was
/// already stopped or replaced was reported by whoever took it, so only the
/// current run is reaped and reported here.
fn finalize_run_on_main_thread(
    app: &tauri::AppHandle,
    run_id: RunId,
    intentional: bool,
    recoverable: bool,
) {
    let Some(run) = take_finished_run(&app.state::<RunState>(), run_id) else {
        return;
    };
    drop(run);
    update_tray_tooltip(app, false);
    let _ = app.emit(
        "echoless://exit",
        json!({
            "run_id": run_id,
            "intentional": intentional,
            "recoverable": recoverable,
        }),
    );
}

/// Sync commands run on the main thread, like stop, reader finalization and
/// app exit, so a start never interleaves with another lifecycle action.
#[tauri::command]
pub(crate) fn start_run(
    app: tauri::AppHandle,
    state: State<RunState>,
    toml_text: String,
    stats_interval_ms: Option<u32>,
) -> Result<RunId, String> {
    let (run_id, previous) = begin_run(&state);
    // Release the previous run's audio devices before the new process opens them.
    drop(previous);

    let mut command = echoless_command(Some(&app))?;
    let path = write_transient_config_toml(&transient_config_dir(&app)?, "run", &toml_text)?;
    let interval = stats_interval_ms.unwrap_or(80).to_string();
    suppress_child_console(&mut command);
    command
        .args(["run", "--config"])
        .arg(&path)
        .args(["--status-json", "--stats-interval-ms", &interval])
        // stdin carries JSONL control commands; EOF asks the CLI to stop.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            cleanup_run_config(&path);
            let message = format!("spawn echoless run failed: {err}");
            crate::logging::log("error", "sidecar", &message);
            return Err(message);
        }
    };
    let mut run = RunChild::new(run_id, child, path);
    let (Some(stdout), Some(stderr)) = (run.child.stdout.take(), run.child.stderr.take()) else {
        return Err("echoless run output pipes are missing".to_string());
    };
    let stopping = run.stopping.clone();
    // Install before the readers start so the first status lines pass the
    // generation filter.
    install_run(&state, run);

    // stdout: JSONL status events.
    let app_out = app.clone();
    std::thread::spawn(move || {
        let log_if_active = |level: &str, message: String| {
            if is_active_run(&app_out.state::<RunState>(), run_id) {
                crate::logging::log(level, "sidecar", &message);
                let _ = app_out.emit("echoless://log", message);
            }
        };
        let mut recoverable = false;
        for line in BufReader::new(stdout).lines() {
            let line = match line {
                Ok(line) => line,
                Err(err) => {
                    log_if_active("error", format!("failed to read echoless stdout: {err}"));
                    break;
                }
            };
            match parse_jsonl_line_event(&line) {
                JsonlLineEvent::Empty => {}
                JsonlLineEvent::Json(value) => {
                    recoverable |= is_recoverable_stream_failure(&value);
                    match attach_run_id(value, run_id) {
                        Ok(value) => {
                            if is_active_run(&app_out.state::<RunState>(), run_id) {
                                let _ = app_out.emit("echoless://status", value);
                            }
                        }
                        Err(value) => log_if_active(
                            "warn",
                            format!("non-object status JSON ignored: {value}"),
                        ),
                    }
                }
                JsonlLineEvent::Unparsed(line) => {
                    log_if_active("warn", format!("unparsed status line: {line}"));
                }
            }
        }
        // Set by RunChild::drop: the GUI stopped or replaced this run.
        // Otherwise the child exited on its own (crash).
        let intentional = stopping.load(Ordering::SeqCst);
        crate::logging::log(
            if intentional { "info" } else { "error" },
            "sidecar",
            &format!("echoless run {run_id} exited (intentional={intentional})"),
        );
        let finalizer_app = app_out.clone();
        if let Err(err) = app_out.run_on_main_thread(move || {
            finalize_run_on_main_thread(&finalizer_app, run_id, intentional, recoverable);
        }) {
            crate::logging::log(
                "error",
                "sidecar",
                &format!("failed to schedule run {run_id} finalization on main thread: {err}"),
            );
        }
    });

    // stderr: human-readable log, forwarded to the UI and written to the log file.
    let app_err = app.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            match line {
                Ok(line) => {
                    if !line.trim().is_empty() {
                        // Persist first, then gate only the UI event by generation. The stdout
                        // finalizer can clear the active run before this reader receives the
                        // trailing error line; dropping it here would erase the crash evidence.
                        let level = if line.contains(" stream error:") {
                            "error"
                        } else {
                            "info"
                        };
                        crate::logging::log(level, "cli", &format!("run {run_id}: {line}"));
                        if is_active_run(&app_err.state::<RunState>(), run_id) {
                            let _ = app_err.emit("echoless://log", line);
                        }
                    }
                }
                Err(err) => {
                    crate::logging::log(
                        "error",
                        "sidecar",
                        &format!("failed to read echoless run {run_id} stderr: {err}"),
                    );
                    if is_active_run(&app_err.state::<RunState>(), run_id) {
                        let _ = app_err.emit(
                            "echoless://log",
                            format!("failed to read echoless stderr: {err}"),
                        );
                    }
                    break;
                }
            }
        }
    });

    crate::logging::log("info", "sidecar", "echoless run started");
    update_tray_tooltip(&app, true);
    Ok(run_id)
}

pub(crate) fn attach_run_id(mut value: Value, run_id: RunId) -> Result<Value, Value> {
    let Some(object) = value.as_object_mut() else {
        return Err(value);
    };
    object.insert("run_id".to_string(), json!(run_id));
    Ok(value)
}

pub(crate) fn is_recoverable_stream_failure(value: &Value) -> bool {
    value.as_object().is_some_and(|event| {
        event.get("type").and_then(Value::as_str) == Some("stream_error")
            && event.get("fatal").and_then(Value::as_bool) == Some(true)
            && event.get("recoverable").and_then(Value::as_bool) == Some(true)
    })
}

/// 向运行中的 echoless run 子进程 stdin 写一行 JSON 控制命令。
/// 具体能力由 CLI started.supported_controls 上报。
#[tauri::command]
pub(crate) async fn send_run_control(
    state: State<'_, RunState>,
    line: String,
) -> Result<(), String> {
    let stdin = run_control_stdin(&state)?;
    tauri::async_runtime::spawn_blocking(move || write_run_control(&stdin, &line))
        .await
        .map_err(|err| format!("run control task join failed: {err}"))?
}

#[tauri::command]
pub(crate) fn stop_run(
    app: tauri::AppHandle,
    state: State<RunState>,
) -> Result<Option<RunId>, String> {
    let run_id = terminate_run(&state);
    if let Some(run_id) = run_id {
        update_tray_tooltip(&app, false);
        let _ = app.emit(
            "echoless://exit",
            json!({ "run_id": run_id, "intentional": true }),
        );
    }
    Ok(run_id)
}
