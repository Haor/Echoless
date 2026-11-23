use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::bin_resolve::find_localvqe_library_in_dir;
use crate::localvqe::{
    localvqe_model_pin, migrate_legacy_localvqe_models_from_base, verify_localvqe_model_file,
    LocalVqeModelPin,
};
use crate::platform::{
    cleanup_run_config, validate_browser_url, validate_open_path, write_transient_config_toml,
};
use crate::proc::{
    begin_run, command_output_with_timeout, install_run, is_active_run, parse_jsonl_line_event,
    push_tail_line, run_control_stdin, take_finished_run, terminate_run, JsonlLineEvent, RunChild,
    RunId, RunState,
};
use crate::sidecar::{attach_run_id, is_recoverable_stream_failure};

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
    let mut command = Command::new("sleep");
    command.arg("2");
    command
}

#[cfg(windows)]
fn slow_child_command() -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "ping -n 3 127.0.0.1 > nul"]);
    command
}

/// A `RunChild` around a process that has already exited, so dropping it is quick.
fn exited_run_child(run_id: RunId, config_path: &Path) -> RunChild {
    std::fs::write(config_path, "stub = true").unwrap();
    #[cfg(unix)]
    let mut command = Command::new("true");
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "exit 0"]);
        command
    };
    let child = command.stdin(Stdio::piped()).spawn().unwrap();
    RunChild::new(run_id, child, config_path.to_path_buf())
}

#[test]
fn only_fatal_recoverable_stream_events_restart_the_run() {
    assert!(is_recoverable_stream_failure(&json!({
        "type": "stream_error",
        "fatal": true,
        "recoverable": true,
    })));
    assert!(!is_recoverable_stream_failure(&json!({
        "type": "stream_error",
        "fatal": true,
        "recoverable": false,
    })));
    assert!(!is_recoverable_stream_failure(&json!({
        "type": "status",
        "fatal": true,
        "recoverable": true,
    })));
}

#[test]
fn run_id_injection_requires_object_and_overrides_cli_value() {
    assert_eq!(
        attach_run_id(json!({"type": "status", "run_id": 999}), 7).unwrap(),
        json!({"type": "status", "run_id": 7})
    );
    assert_eq!(attach_run_id(json!(["status"]), 7), Err(json!(["status"])));
}

#[test]
fn run_generations_reap_once_and_ignore_late_readers() {
    let dir = unique_temp_dir("echoless-run-generations");
    let state = RunState::default();

    let (first_id, previous) = begin_run(&state);
    assert!(previous.is_none());
    let first_config = dir.join("first.toml");
    install_run(&state, exited_run_child(first_id, &first_config));
    assert!(is_active_run(&state, first_id));

    // A restart hands the running child back so it is stopped before the next spawn.
    let (second_id, previous) = begin_run(&state);
    assert_ne!(second_id, first_id);
    assert!(!is_active_run(&state, first_id));
    let previous = previous.expect("restart must return the running child");
    let stopping = previous.stopping.clone();
    drop(previous);
    assert!(stopping.load(Ordering::SeqCst));
    assert!(!first_config.exists());

    let second_config = dir.join("second.toml");
    install_run(&state, exited_run_child(second_id, &second_config));

    // The first run's reader finishing late must not reap the second run.
    assert!(take_finished_run(&state, first_id).is_none());
    assert!(is_active_run(&state, second_id));

    // The current run is reaped exactly once.
    let finished = take_finished_run(&state, second_id).expect("current run must be reaped");
    assert!(take_finished_run(&state, second_id).is_none());
    assert!(!is_active_run(&state, second_id));
    drop(finished);
    assert!(!second_config.exists());
    assert_eq!(terminate_run(&state), None);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn terminate_run_stops_child_and_rejects_its_late_events() {
    let dir = unique_temp_dir("echoless-terminate-run");
    let config_path = dir.join("run.toml");
    let state = RunState::default();
    let (run_id, _) = begin_run(&state);
    let run = exited_run_child(run_id, &config_path);
    let stopping = run.stopping.clone();
    install_run(&state, run);

    assert_eq!(terminate_run(&state), Some(run_id));

    assert!(stopping.load(Ordering::SeqCst));
    assert!(!config_path.exists());
    assert!(!is_active_run(&state, run_id));
    assert!(take_finished_run(&state, run_id).is_none());
    assert!(run_control_stdin(&state).is_err());

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn control_lines_reach_stdin_and_stop_sends_eof() {
    use std::io::Read;

    use crate::proc::write_run_control;

    let dir = unique_temp_dir("echoless-run-control");
    let config_path = dir.join("run.toml");
    std::fs::write(&config_path, "stub = true").unwrap();
    let state = RunState::default();
    let (run_id, _) = begin_run(&state);
    let child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut run = RunChild::new(run_id, child, config_path);
    let mut stdout = run.child.stdout.take().unwrap();
    install_run(&state, run);

    let line = r#"{"cmd":"start_diagnostics"}"#;
    write_run_control(&run_control_stdin(&state).unwrap(), line).unwrap();
    let started = Instant::now();
    assert_eq!(terminate_run(&state), Some(run_id));

    // `cat` exits on stdin EOF, well before the forced-kill deadline.
    assert!(started.elapsed() < Duration::from_millis(900));
    let mut echoed = String::new();
    stdout.read_to_string(&mut echoed).unwrap();
    assert_eq!(echoed, format!("{line}\n"));

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn child_that_ignores_eof_is_killed_after_bounded_wait() {
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();

    crate::proc::shutdown_child_gracefully(&mut child);

    let status = child
        .try_wait()
        .unwrap()
        .expect("killed child must be reaped");
    assert!(!status.success());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn command_timeout_kills_hung_child() {
    let mut command = slow_child_command();
    let started = Instant::now();
    let err = command_output_with_timeout(
        &mut command,
        Duration::from_millis(80),
        "slow child test",
        |_| false,
    )
    .unwrap_err();

    assert!(err.contains("timed out"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn command_output_drains_pipes_while_waiting_and_filters_stderr() {
    use std::sync::Arc;

    // More stdout than a pipe buffer holds: the child only exits if it is read.
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "echo progress >&2; echo oops >&2; head -c 200000 /dev/zero",
    ]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_lines = seen.clone();
    let out = command_output_with_timeout(
        &mut command,
        Duration::from_secs(10),
        "drain test",
        move |line| {
            seen_lines.lock().unwrap().push(line.to_string());
            line == "progress"
        },
    )
    .unwrap();

    assert!(out.status.success());
    assert_eq!(out.stdout.len(), 200_000);
    assert_eq!(String::from_utf8(out.stderr).unwrap(), "oops\n");
    assert_eq!(*seen.lock().unwrap(), ["progress", "oops"]);
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
    assert_eq!(
        validate_browser_url("HTTPS://GITHUB.COM:443/Haor/Echoless").unwrap(),
        "https://github.com/Haor/Echoless"
    );
    assert!(validate_browser_url("https://github.com./Haor/Echoless").is_ok());
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
        "https://evil.example\\@github.com/Haor/Echoless",
        "https://user:password@github.com/Haor/Echoless",
        "https://github.com@evil.example/Haor/Echoless",
        "https://github.com%2f.evil.example/Haor/Echoless",
        "https://github.com%40evil.example/Haor/Echoless",
        "https://github.com:444/Haor/Echoless",
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
fn transient_config_writer_creates_unique_files() {
    let dir = unique_temp_dir("echoless-transient-config");
    let first = write_transient_config_toml(&dir, "run", "one = true").unwrap();
    let second = write_transient_config_toml(&dir, "run", "two = true").unwrap();

    assert_ne!(first, second);
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
    assert!(err.contains("SHA256 mismatch"), "{err}");

    let wrong_size = LocalVqeModelPin { size: 4, ..good };
    let err = verify_localvqe_model_file(&path, &wrong_size)
        .unwrap_err()
        .to_string();
    assert!(err.contains("size mismatch"), "{err}");

    let _ = std::fs::remove_dir_all(dir);
}
