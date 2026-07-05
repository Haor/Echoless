// Echoless GUI 的 Tauri 后端:
//   - 平台探测(标题栏镜像)
//   - 把 `echoless` CLI 作为 sidecar 调用,只消费 JSON / JSONL 契约
//   - run 的 --status-json 以 JSONL 流式解析,经事件推给前端
//
// 契约真理源:docs/CLI.md + CLI `--json` 实测。
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
#[cfg(target_os = "macos")]
use tauri_plugin_decorum::WebviewWindowExt;

mod bin_resolve;
mod localvqe;
mod platform;
mod proc;
mod tray;

#[cfg(test)]
use bin_resolve::find_localvqe_library_in_dir;
use bin_resolve::{echoless_command, suppress_child_console};
use localvqe::{download_localvqe_model, localvqe_assets};
#[cfg(test)]
use localvqe::{
    localvqe_model_pin, migrate_legacy_localvqe_models_from_base, verify_localvqe_model_file,
    LocalVqeModelPin,
};
#[cfg(test)]
use platform::{
    browser_open_command, validate_browser_url, validate_open_path, write_toml_create_new,
};
use platform::{
    cleanup_run_config, default_diag_dir, open_path, open_url, transient_config_dir,
    write_transient_config_toml,
};
use proc::{
    command_output_with_timeout, command_status_error, mark_run_exited, parse_jsonl_line_event,
    push_tail_line, run_json_async, run_json_blocking, run_state_guard, shutdown_child_gracefully,
    terminate_run, JsonlLineEvent, RunChild, RunState, JSON_COMMAND_TIMEOUT, NVAFX_INSTALL_TIMEOUT,
    PROBE_DELAY_TIMEOUT, STREAM_TAIL_LIMIT_BYTES, VALIDATE_COMMAND_TIMEOUT,
};
#[cfg(test)]
use tray::set_tray_prefs_inner;
use tray::{close_to_tray_enabled, set_tray_prefs, update_tray_tooltip, TrayPrefs};
#[cfg(target_os = "windows")]
use tray::{register_windows_tray, TrayIconState};

#[tauri::command]
fn get_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

#[tauri::command]
async fn list_devices(app: tauri::AppHandle) -> Result<Value, String> {
    run_json_async(
        app,
        vec!["devices".into(), "--json".into(), "--fast".into()],
        JSON_COMMAND_TIMEOUT,
        "devices",
    )
    .await
}

#[tauri::command]
async fn list_processors(app: tauri::AppHandle) -> Result<Value, String> {
    run_json_async(
        app,
        vec!["processors".into(), "--json".into()],
        JSON_COMMAND_TIMEOUT,
        "processors",
    )
    .await
}

#[tauri::command]
async fn doctor_audio(app: tauri::AppHandle) -> Result<Value, String> {
    run_json_async(
        app,
        vec![
            "doctor".into(),
            "audio".into(),
            "--json".into(),
            "--fast-devices".into(),
        ],
        JSON_COMMAND_TIMEOUT,
        "doctor audio",
    )
    .await
}

/// 用户点击「请求系统音频权限」时调用:跑一次极短 Process Tap probe 触发 macOS 授权弹窗,
/// 回传 system_audio_permission + system_audio_permission_probe。普通 doctor 不会触发弹窗。
#[tauri::command]
async fn request_system_audio(app: tauri::AppHandle) -> Result<Value, String> {
    run_json_async(
        app,
        vec![
            "doctor".into(),
            "audio".into(),
            "--fast-devices".into(),
            "--request-system-audio".into(),
            "--json".into(),
        ],
        JSON_COMMAND_TIMEOUT,
        "request system audio",
    )
    .await
}

/// 主动近端延迟侦测 / AEC 链路诊断。shell `echoless probe-delay --json`:播放一串蜂鸣、
/// probe-delay 专用 runner:stderr 的 JSONL 进度行实时转发为
/// `echoless://probe-progress` 事件(前端用 beep_train_start 把进度灯对齐真实播放时刻),
/// stdout 仍在进程结束后整体解析为最终 JSON 结果。
fn run_probe_streaming(
    app: &tauri::AppHandle,
    args: &[&str],
    timeout: Duration,
) -> Result<Value, String> {
    let label = "probe-delay";
    let mut command = echoless_command(Some(app))?;
    command.args(args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    suppress_child_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn {label} failed: {e}"))?;
    let stderr = child.stderr.take().ok_or("probe stderr not captured")?;
    let app_ev = app.clone();
    // stderr 尾巴留存:CLI 失败时错误原因在 stderr(stdout 无 JSON)。
    let stderr_tail = Arc::new(Mutex::new(String::new()));
    let tail_writer = stderr_tail.clone();
    let reader = std::thread::spawn(move || {
        // probe-delay 进度契约:stderr 上每条进度都是完整 JSONL;坏行降级为日志保留证据。
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            match parse_jsonl_line_event(&line) {
                JsonlLineEvent::Empty => {}
                JsonlLineEvent::Json(v) => {
                    let _ = app_ev.emit("echoless://probe-progress", v);
                }
                JsonlLineEvent::Unparsed(line) => {
                    let _ = app_ev.emit("echoless://log", format!("unparsed probe line: {line}"));
                }
            }
            let mut tail = tail_writer.lock().unwrap();
            push_tail_line(&mut tail, &line, STREAM_TAIL_LIMIT_BYTES);
        }
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                let tail = stderr_tail.lock().unwrap().trim().to_string();
                return Err(format!(
                    "{label} timed out after {}s; stderr: {tail}",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("wait {label} failed: {e}")),
        }
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("read {label} output failed: {e}"))?;
    let _ = reader.join();
    if !out.status.success() {
        let tail = stderr_tail.lock().unwrap().trim().to_string();
        // 与 command_status_error 相同的 240 字符截断(错误直达前端 UI)。
        let detail: String = if tail.chars().count() > 240 {
            format!("{}…", tail.chars().take(240).collect::<String>())
        } else {
            tail
        };
        return Err(format!(
            "{label} failed with status {}; output: {detail}",
            out.status
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("parse json failed: {e}; raw: {stdout}"))
}

/// 同时录 ref/mic、分析两路相对到达时差,返回 NearDelayProbeResult(含 recommended_near_delay_ms)。
/// 约 15 秒、会外放蜂鸣 —— 故必须先停掉主 run(probe 内部自起子进程占用设备),由前端 gating。
/// 当前后端只支持 macOS Process Tap;其它平台 CLI 会非 0 退出,错误经 stderr 透传给前端。
#[tauri::command]
async fn probe_delay(
    app: tauri::AppHandle,
    mic: String,
    reference: String,
    output: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut args: Vec<String> = vec!["probe-delay".into(), "--json".into()];
        // selector 透传(含 "default",与 run 同一套解析);仅空串时省略走 CLI 内置默认。
        let opt = |flag: &str, v: &str, args: &mut Vec<String>| {
            if !v.is_empty() {
                args.push(flag.into());
                args.push(v.into());
            }
        };
        opt("--mic", &mic, &mut args);
        opt("--reference", &reference, &mut args);
        opt("--output", &output, &mut args);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_probe_streaming(&app, &arg_refs, PROBE_DELAY_TIMEOUT)
    })
    .await
    .map_err(|e| format!("probe task join failed: {e}"))?
}

/// NVIDIA AFX / RTX AEC 引擎就绪探针。
/// 返回 { ok, report: { runtime_dir, runtime_dir_source, gpus[], selected_arch, checks[] } }。
/// macOS/Linux 上后端会返回 ok=false + platform unsupported 检查项(诚实降级)。
#[tauri::command]
async fn nvafx_doctor(app: tauri::AppHandle, runtime_dir: Option<String>) -> Result<Value, String> {
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
async fn nvafx_install(
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
async fn nvafx_download_install(
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

#[tauri::command]
async fn validate_config(app: tauri::AppHandle, toml_text: String) -> Result<Value, String> {
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

#[tauri::command]
fn start_run(
    app: tauri::AppHandle,
    state: State<RunState>,
    toml_text: String,
    stats_interval_ms: Option<u32>,
) -> Result<(), String> {
    let mut guard = run_state_guard(&state);
    // 幂等启动:若有残留子进程(并发重启 / 上次崩溃遗留),经 RunChild::Drop
    // 优雅停机(stopping 标记使其 reader 判定为 intentional,不报崩溃)。
    drop(guard.take());
    let dir = transient_config_dir(&app)?;
    let path = write_transient_config_toml(&dir, "run", &toml_text)?;
    let config_arg = path.to_string_lossy().to_string();
    let interval = stats_interval_ms.unwrap_or(80).to_string();

    let mut command = echoless_command(Some(&app))?;
    suppress_child_console(&mut command);
    let child_result = command
        .args([
            "run",
            "--config",
            &config_arg,
            "--status-json",
            "--stats-interval-ms",
            &interval,
        ])
        .stdin(Stdio::piped()) // 录制就地控制:start/stop_diagnostics 经 stdin JSONL 下发
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child_result {
        Ok(child) => child,
        Err(err) => {
            cleanup_run_config(&path);
            return Err(format!("spawn echoless run failed: {err}"));
        }
    };

    // 本子进程专属的 stopping flag:被主动停/重启时置 true。
    let stopping = Arc::new(AtomicBool::new(false));

    // stdout = JSONL status events
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            shutdown_child_gracefully(&mut child);
            cleanup_run_config(&path);
            return Err("no stdout".to_string());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            shutdown_child_gracefully(&mut child);
            cleanup_run_config(&path);
            return Err("no stderr".to_string());
        }
    };
    let app_out = app.clone();
    let stop_reader = stopping.clone();
    let reader_config_path = path.clone();
    std::thread::spawn(move || {
        // CLI stdout 契约:只输出完整 JSONL status/control 行;坏行降级为日志保留证据。
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => match parse_jsonl_line_event(&line) {
                    JsonlLineEvent::Empty => {}
                    JsonlLineEvent::Json(v) => {
                        let _ = app_out.emit("echoless://status", v);
                    }
                    JsonlLineEvent::Unparsed(line) => {
                        let _ =
                            app_out.emit("echoless://log", format!("unparsed status line: {line}"));
                    }
                },
                Err(err) => {
                    let _ = app_out.emit(
                        "echoless://log",
                        format!("failed to read echoless stdout: {err}"),
                    );
                    break;
                }
            }
        }
        // 退出归因:intentional=主动停/重启(本 flag 已被置 true);否则=子进程自己退出(崩溃)。
        let intentional = stop_reader.load(Ordering::SeqCst);
        let run_state = app_out.state::<RunState>();
        mark_run_exited(&run_state, &reader_config_path);
        update_tray_tooltip(&app_out, false);
        let _ = app_out.emit(
            "echoless://exit",
            serde_json::json!({ "intentional": intentional }),
        );
    });

    // stderr = 人类日志
    let app_err = app.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            match line {
                Ok(line) => {
                    if !line.trim().is_empty() {
                        let _ = app_err.emit("echoless://log", line);
                    }
                }
                Err(err) => {
                    let _ = app_err.emit(
                        "echoless://log",
                        format!("failed to read echoless stderr: {err}"),
                    );
                    break;
                }
            }
        }
    });

    *guard = Some(RunChild {
        child,
        stopping,
        config_path: path,
    });
    update_tray_tooltip(&app, true);
    Ok(())
}

/// 向运行中的 echoless run 子进程 stdin 写一行 JSON 控制命令。
/// 具体能力由 CLI started.supported_controls 上报。
#[tauri::command]
fn send_run_control(state: State<RunState>, line: String) -> Result<(), String> {
    write_run_control_line(&state, &line)
}

fn write_run_control_line(state: &RunState, line: &str) -> Result<(), String> {
    let mut guard = run_state_guard(state);
    let write_result = {
        let rc = guard.as_mut().ok_or("not running")?;
        let stdin = rc.child.stdin.as_mut().ok_or("no stdin")?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|e| e.to_string())
    };
    if let Err(err) = write_result {
        let status = guard
            .as_mut()
            .and_then(|rc| rc.child.try_wait().ok().flatten());
        if let Some(status) = status {
            drop(guard.take());
            return Err(format!(
                "run process exited before control command was applied: {status}"
            ));
        }
        return Err(err);
    }

    let status = {
        let rc = guard.as_mut().ok_or("not running")?;
        rc.child
            .try_wait()
            .map_err(|e| format!("failed to check run process after control command: {e}"))?
    };
    if let Some(status) = status {
        drop(guard.take());
        return Err(format!(
            "run process exited before control command was applied: {status}"
        ));
    }
    Ok(())
}

#[tauri::command]
fn set_bypass(state: State<RunState>, enabled: bool) -> Result<(), String> {
    let line = bypass_control_line(enabled);
    write_run_control_line(&state, &line)
}

fn bypass_control_line(enabled: bool) -> String {
    json!({
        "cmd": "set_bypass",
        "enabled": enabled,
    })
    .to_string()
}

#[tauri::command]
fn stop_run(app: tauri::AppHandle, state: State<RunState>) -> Result<(), String> {
    terminate_run(&state);
    update_tray_tooltip(&app, false);
    Ok(())
}

// macOS 设备热插拔监听:CoreAudio 设备列表('dev#')变更即推事件给前端刷新。
// WKWebView 不触发 navigator.mediaDevices 的 devicechange,只能原生侧监听;
// Windows 的 WebView2(Chromium)会触发,前端已挂 devicechange,无需原生监听。
#[cfg(target_os = "macos")]
mod device_watch {
    use std::ffi::c_void;
    use std::sync::Mutex;
    use tauri::Emitter;

    // CoreAudio/AudioHardware.h
    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const SYSTEM_OBJECT: u32 = 1; // kAudioObjectSystemObject
    const DEVICES_ADDRESS: AudioObjectPropertyAddress = AudioObjectPropertyAddress {
        selector: u32::from_be_bytes(*b"dev#"), // kAudioHardwarePropertyDevices
        scope: u32::from_be_bytes(*b"glob"),    // kAudioObjectPropertyScopeGlobal
        element: 0,                             // kAudioObjectPropertyElementMain
    };

    type Listener = extern "C" fn(u32, u32, *const AudioObjectPropertyAddress, *mut c_void) -> i32;

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectAddPropertyListener(
            object_id: u32,
            address: *const AudioObjectPropertyAddress,
            listener: Listener,
            client_data: *mut c_void,
        ) -> i32;
        fn AudioObjectRemovePropertyListener(
            object_id: u32,
            address: *const AudioObjectPropertyAddress,
            listener: Listener,
            client_data: *mut c_void,
        ) -> i32;
    }

    #[derive(Default)]
    pub struct DeviceWatchState {
        client: Mutex<Option<usize>>,
    }

    // HAL 通知线程回调:只透传「变了」,枚举仍由前端调 list_devices 完成。
    extern "C" fn on_devices_changed(
        _object_id: u32,
        _num_addresses: u32,
        _addresses: *const AudioObjectPropertyAddress,
        client_data: *mut c_void,
    ) -> i32 {
        let app = unsafe { &*(client_data as *const tauri::AppHandle) };
        let _ = app.emit("echoless://devices-changed", ());
        0
    }

    pub fn start(app: &tauri::AppHandle, state: &DeviceWatchState) {
        stop(state);
        let client = Box::into_raw(Box::new(app.clone()));
        let status = unsafe {
            AudioObjectAddPropertyListener(
                SYSTEM_OBJECT,
                &DEVICES_ADDRESS,
                on_devices_changed,
                client as *mut c_void,
            )
        };
        if status != 0 {
            let _ = app.emit(
                "echoless://log",
                format!("device watch: AudioObjectAddPropertyListener failed ({status})"),
            );
            unsafe {
                drop(Box::from_raw(client));
            }
            return;
        }
        if let Ok(mut guard) = state.client.lock() {
            *guard = Some(client as usize);
        } else {
            let _ = app.emit(
                "echoless://log",
                "device watch: failed to store CoreAudio listener state",
            );
            unsafe {
                let _ = AudioObjectRemovePropertyListener(
                    SYSTEM_OBJECT,
                    &DEVICES_ADDRESS,
                    on_devices_changed,
                    client as *mut c_void,
                );
                drop(Box::from_raw(client));
            }
        }
    }

    pub fn stop(state: &DeviceWatchState) {
        let Some(client) = state.client.lock().ok().and_then(|mut guard| guard.take()) else {
            return;
        };
        unsafe {
            let _ = AudioObjectRemovePropertyListener(
                SYSTEM_OBJECT,
                &DEVICES_ADDRESS,
                on_devices_changed,
                client as *mut c_void,
            );
            drop(Box::from_raw(client as *mut tauri::AppHandle));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_decorum::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(RunState(Mutex::new(None)))
        .manage(TrayPrefs::default());

    #[cfg(target_os = "windows")]
    let builder = builder.manage(TrayIconState(Mutex::new(None)));
    #[cfg(target_os = "macos")]
    let builder = builder.manage(device_watch::DeviceWatchState::default());

    builder
        .invoke_handler(tauri::generate_handler![
            get_platform,
            list_devices,
            list_processors,
            doctor_audio,
            request_system_audio,
            probe_delay,
            localvqe_assets,
            download_localvqe_model,
            nvafx_doctor,
            nvafx_install,
            nvafx_download_install,
            open_url,
            default_diag_dir,
            open_path,
            validate_config,
            start_run,
            send_run_control,
            set_bypass,
            stop_run,
            set_tray_prefs
        ])
        .setup(|app| {
            // 默认打开基线 1040×640(v17 设计稿画布,布局按此定稿);
            // B1:min 锁到默认尺寸 —— plate 分格在更小窗口必然破版。
            // B3:builder 背景色 = 新色板 --bg #1d1d1b,resize 瞬间不露白边。
            let mut builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Echoless")
                    .inner_size(1040.0, 640.0)
                    .min_inner_size(1040.0, 640.0)
                    .max_inner_size(1600.0, 1100.0)
                    .background_color(tauri::window::Color(0x1d, 0x1d, 0x1b, 0xff))
                    .resizable(true)
                    .visible(true);

            // 平台镜像标题栏(见 Design.md §5.1):
            //   macOS  → Overlay + 隐藏标题,保留系统红绿灯(OS 绘制,左上)
            //   其它   → 去原生装饰,自绘 caption 按钮(右上),恢复阴影/圆角
            #[cfg(target_os = "macos")]
            {
                builder = builder
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true);
            }
            #[cfg(not(target_os = "macos"))]
            {
                builder = builder.decorations(false).shadow(true);
            }

            let window = builder.build()?;

            // macOS:把系统红绿灯垂直居中到 40px 标题栏,并与左侧内容对齐。
            #[cfg(target_os = "macos")]
            {
                let _ = window.set_traffic_lights_inset(16.0, 13.0);
            }
            #[cfg(target_os = "windows")]
            {
                register_windows_tray(app)?;
            }
            #[cfg(target_os = "macos")]
            {
                let device_watch_state = app.state::<device_watch::DeviceWatchState>();
                device_watch::start(app.handle(), &device_watch_state);
            }
            let _ = &window;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let prefs = window.state::<TrayPrefs>();
                if close_to_tray_enabled(&prefs) {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    let state = window.state::<RunState>();
                    terminate_run(&state);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Cmd+Q / 菜单退出 / Dock Quit 不产生 CloseRequested(审计 B-01):
            // 统一在 ExitRequested 回收 sidecar,避免孤儿 CLI 占用麦克风/虚拟麦。
            if let tauri::RunEvent::ExitRequested { .. } = event {
                terminate_run(&app_handle.state::<RunState>());
                #[cfg(target_os = "macos")]
                device_watch::stop(&app_handle.state::<device_watch::DeviceWatchState>());
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    static DATA_ROOT_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn with_test_data_root<T>(root: &Path, run: impl FnOnce() -> T) -> T {
        let _guard = DATA_ROOT_ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os(echoless_paths::DATA_ROOT_ENV_VAR);
        std::env::set_var(echoless_paths::DATA_ROOT_ENV_VAR, root);
        let result = run();
        if let Some(previous) = previous {
            std::env::set_var(echoless_paths::DATA_ROOT_ENV_VAR, previous);
        } else {
            std::env::remove_var(echoless_paths::DATA_ROOT_ENV_VAR);
        }
        result
    }

    #[cfg(unix)]
    fn slow_child_command() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 2"]);
        command
    }

    #[cfg(windows)]
    fn slow_child_command() -> Command {
        let mut command = Command::new("cmd");
        command.args(["/C", "ping -n 3 127.0.0.1 > nul"]);
        command
    }

    #[cfg(unix)]
    fn exited_child_with_open_stdin_command() -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", "cat >/dev/null & exit 0"]);
        command.stdin(Stdio::piped());
        command
    }

    #[test]
    fn command_timeout_kills_hung_child() {
        let mut command = slow_child_command();
        let started = Instant::now();
        let err =
            command_output_with_timeout(&mut command, Duration::from_millis(80), "slow child test")
                .unwrap_err();

        assert!(err.contains("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn run_state_guard_recovers_poisoned_lock() {
        let state = RunState(Mutex::new(None));
        let _ = std::panic::catch_unwind(|| {
            let _guard = state.0.lock().expect("test lock should start healthy");
            panic!("poison run state");
        });

        assert!(state.0.is_poisoned());
        let guard = run_state_guard(&state);
        assert!(guard.is_none());
    }

    #[test]
    fn bypass_control_line_matches_runtime_contract() {
        let enabled: Value = serde_json::from_str(&bypass_control_line(true)).unwrap();
        assert_eq!(enabled["cmd"], "set_bypass");
        assert_eq!(enabled["enabled"], true);

        let disabled: Value = serde_json::from_str(&bypass_control_line(false)).unwrap();
        assert_eq!(disabled["cmd"], "set_bypass");
        assert_eq!(disabled["enabled"], false);
    }

    #[test]
    fn jsonl_line_event_classifies_status_lines() {
        assert_eq!(parse_jsonl_line_event("   "), JsonlLineEvent::Empty);
        assert_eq!(
            parse_jsonl_line_event(r#"{"type":"status","ok":true}"#),
            JsonlLineEvent::Json(json!({"type": "status", "ok": true}))
        );
        assert_eq!(
            parse_jsonl_line_event("not json"),
            JsonlLineEvent::Unparsed("not json".to_string())
        );
    }

    #[test]
    fn push_tail_line_truncates_without_splitting_utf8() {
        let mut tail = String::new();
        push_tail_line(&mut tail, "ascii-prefix", 32);
        push_tail_line(&mut tail, "错误错误错误错误错误", 16);

        assert!(tail.len() <= 16, "{tail:?}");
        assert!(tail.ends_with('\n'));
        assert!(std::str::from_utf8(tail.as_bytes()).is_ok());
    }

    #[test]
    fn default_diag_dir_uses_brand_data_root() {
        let root = unique_temp_dir("echoless-diag-root");
        with_test_data_root(&root, || {
            assert_eq!(PathBuf::from(default_diag_dir()), root.join("diagnostics"));
        });
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migrate_legacy_localvqe_models_moves_only_missing_gguf_files() {
        let legacy_base = unique_temp_dir("echoless-legacy-localvqe");
        let legacy_models = legacy_base.join("localvqe").join("models");
        std::fs::create_dir_all(&legacy_models).unwrap();
        let dest = unique_temp_dir("echoless-localvqe-dest");

        std::fs::write(legacy_models.join("move-me.gguf"), b"new").unwrap();
        std::fs::write(legacy_models.join("keep-existing.gguf"), b"legacy").unwrap();
        std::fs::write(legacy_models.join("notes.txt"), b"ignore").unwrap();
        std::fs::write(dest.join("keep-existing.gguf"), b"dest").unwrap();

        migrate_legacy_localvqe_models_from_base(&legacy_base, &dest);

        assert_eq!(std::fs::read(dest.join("move-me.gguf")).unwrap(), b"new");
        assert!(!legacy_models.join("move-me.gguf").exists());
        assert_eq!(
            std::fs::read(dest.join("keep-existing.gguf")).unwrap(),
            b"dest"
        );
        assert!(legacy_models.join("keep-existing.gguf").exists());
        assert!(!dest.join("notes.txt").exists());

        let _ = std::fs::remove_dir_all(legacy_base);
        let _ = std::fs::remove_dir_all(dest);
    }

    #[test]
    fn terminate_run_marks_stopping_waits_and_cleans_config() {
        let dir = unique_temp_dir("echoless-terminate-run");
        let config_path = dir.join("run.toml");
        std::fs::write(&config_path, "stub = true").unwrap();
        let stopping = Arc::new(AtomicBool::new(false));
        let child = slow_child_command().spawn().unwrap();
        let state = RunState(Mutex::new(Some(RunChild {
            child,
            stopping: stopping.clone(),
            config_path: config_path.clone(),
        })));

        terminate_run(&state);

        assert!(stopping.load(Ordering::SeqCst));
        assert!(run_state_guard(&state).is_none());
        assert!(!config_path.exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_run_control_line_reaps_exited_child_after_successful_write() {
        let dir = unique_temp_dir("echoless-run-control-exited");
        let config_path = dir.join("run.toml");
        std::fs::write(&config_path, "stub = true").unwrap();
        let stopping = Arc::new(AtomicBool::new(false));
        let child = exited_child_with_open_stdin_command().spawn().unwrap();
        let state = RunState(Mutex::new(Some(RunChild {
            child,
            stopping: stopping.clone(),
            config_path: config_path.clone(),
        })));

        for _ in 0..50 {
            let exited = run_state_guard(&state)
                .as_mut()
                .and_then(|rc| rc.child.try_wait().ok().flatten())
                .is_some();
            if exited {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let err = write_run_control_line(&state, &bypass_control_line(true)).unwrap_err();

        assert!(
            err.contains("exited before control command was applied"),
            "{err}"
        );
        assert!(stopping.load(Ordering::SeqCst));
        assert!(run_state_guard(&state).is_none());
        assert!(!config_path.exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tray_prefs_default_false_and_follow_platform_gate() {
        let prefs = TrayPrefs::default();
        assert!(!prefs.close_to_tray.load(Ordering::SeqCst));

        set_tray_prefs_inner(&prefs, true);

        #[cfg(target_os = "windows")]
        assert!(prefs.close_to_tray.load(Ordering::SeqCst));
        #[cfg(not(target_os = "windows"))]
        assert!(!prefs.close_to_tray.load(Ordering::SeqCst));
    }

    #[test]
    fn finds_platform_localvqe_native_library() {
        let dir = unique_temp_dir("echoless-localvqe-native");
        let name = if cfg!(target_os = "windows") {
            "localvqe.dll"
        } else if cfg!(target_os = "macos") {
            "liblocalvqe.0.1.0.dylib"
        } else {
            "liblocalvqe.so"
        };
        let expected = dir.join(name);
        std::fs::write(&expected, b"stub").unwrap();
        std::fs::write(dir.join("not-localvqe.txt"), b"stub").unwrap();
        std::fs::write(dir.join("readme.solutions"), b"stub").unwrap();
        std::fs::write(dir.join("liblocalvqe.so.notes"), b"stub").unwrap();

        assert_eq!(
            find_localvqe_library_in_dir(&dir).as_deref(),
            Some(expected.as_path())
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validates_only_allowlisted_browser_urls() {
        assert_eq!(
            validate_browser_url(" https://vb-audio.com/Cable/?x=1 ").unwrap(),
            "https://vb-audio.com/Cable/?x=1"
        );
        assert_eq!(
            validate_browser_url("https://www.nvidia.com/Download/index.aspx").unwrap(),
            "https://www.nvidia.com/Download/index.aspx"
        );
        assert_eq!(
            validate_browser_url("https://aka.ms/vs/17/release/vc_redist.x64.exe").unwrap(),
            "https://aka.ms/vs/17/release/vc_redist.x64.exe"
        );
        // 系统设置深链白名单(隐私面板跳转)。
        assert_eq!(
            validate_browser_url(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
            )
            .unwrap(),
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
        );

        for bad in [
            "",
            "https://",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "mailto:test@example.com",
            "/Applications/Echoless.app",
            "https://example.com/a b",
            "https://example.com/\ncmd",
            "http://vb-audio.com/Cable/",
            "https://vb-audio.com.evil.example/Cable/",
            "x-apple.systempreferences:com.apple.preference.security?General",
        ] {
            assert!(validate_browser_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn validate_open_path_stays_under_brand_data_root() {
        let root = unique_temp_dir("echoless-open-path-root");
        let diagnostics = root.join("diagnostics").join("session-1");
        let models = root.join("localvqe").join("models");
        let external = unique_temp_dir("echoless-open-path-external");
        std::fs::create_dir_all(&diagnostics).unwrap();
        std::fs::create_dir_all(&models).unwrap();

        with_test_data_root(&root, || {
            assert_eq!(
                validate_open_path(diagnostics.to_str().unwrap()).unwrap(),
                diagnostics.canonicalize().unwrap()
            );
            assert_eq!(
                validate_open_path(models.to_str().unwrap()).unwrap(),
                models.canonicalize().unwrap()
            );
            assert!(validate_open_path(root.join("missing").to_str().unwrap()).is_err());
            assert!(validate_open_path(external.to_str().unwrap()).is_err());
        });

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(external);
    }

    #[test]
    fn browser_open_command_avoids_windows_cmd_shell() {
        let (prog, args) = browser_open_command("https://example.com");
        #[cfg(target_os = "windows")]
        {
            assert_eq!(prog, "rundll32.exe");
            assert!(!args.iter().any(|arg| arg == "cmd" || arg == "/C"));
        }
        #[cfg(target_os = "macos")]
        assert_eq!(
            (prog, args),
            ("open", vec!["https://example.com".to_string()])
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            (prog, args),
            ("xdg-open", vec!["https://example.com".to_string()])
        );
    }

    #[test]
    fn config_writer_uses_create_new_and_refuses_existing_path() {
        let dir = unique_temp_dir("echoless-config-create-new");
        let path = dir.join("existing.toml");
        std::fs::write(&path, "old = true").unwrap();

        let err = write_toml_create_new(&path, "new = true").unwrap_err();
        assert!(err.contains("创建配置文件失败"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old = true");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn transient_config_writer_creates_unique_files() {
        let dir = unique_temp_dir("echoless-transient-config");
        let first = write_transient_config_toml(&dir, "run", "one = true").unwrap();
        let second = write_transient_config_toml(&dir, "run", "two = true").unwrap();

        assert_ne!(first, second);
        assert_ne!(
            first.file_name().and_then(|name| name.to_str()),
            Some("echoless-run.toml")
        );
        assert_ne!(
            second.file_name().and_then(|name| name.to_str()),
            Some("echoless-run.toml")
        );
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "one = true");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "two = true");

        cleanup_run_config(&first);
        cleanup_run_config(&second);
        assert!(!first.exists());
        assert!(!second.exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn localvqe_model_pins_reject_unknown_filenames() {
        assert!(localvqe_model_pin("localvqe-v1.3-4.8M-f32.gguf").is_some());
        assert!(localvqe_model_pin("localvqe-v1.2-1.3M-f32.gguf").is_some());
        assert!(localvqe_model_pin("../localvqe-v1.3-4.8M-f32.gguf").is_none());
        assert!(localvqe_model_pin("localvqe-v1.3-4.8M-f32.gguf.part").is_none());
        assert!(localvqe_model_pin("unknown.gguf").is_none());
    }

    #[test]
    fn localvqe_model_verification_checks_size_and_sha256() {
        let dir = unique_temp_dir("echoless-localvqe-model-verify");
        let path = dir.join("model.gguf");
        std::fs::write(&path, b"abc").unwrap();

        let good = LocalVqeModelPin {
            filename: "model.gguf",
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            size: 3,
        };
        verify_localvqe_model_file(&path, &good).unwrap();

        let wrong_hash = LocalVqeModelPin {
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            ..good
        };
        let err = verify_localvqe_model_file(&path, &wrong_hash)
            .unwrap_err()
            .to_string();
        assert!(err.contains("SHA256 不匹配"), "{err}");

        let wrong_size = LocalVqeModelPin { size: 4, ..good };
        let err = verify_localvqe_model_file(&path, &wrong_size)
            .unwrap_err()
            .to_string();
        assert!(err.contains("大小不匹配"), "{err}");

        let _ = std::fs::remove_dir_all(dir);
    }
}
