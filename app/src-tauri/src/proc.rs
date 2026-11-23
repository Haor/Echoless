use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::bin_resolve::{echoless_command, suppress_child_console};

pub(crate) const STREAM_TAIL_LIMIT_BYTES: usize = 4096;
pub(crate) const JSON_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const VALIDATE_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const PROBE_DELAY_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const NVAFX_INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
// download-install fetches about 1 GB; 10 minutes is too short below ~14 Mbps.
pub(crate) const NVAFX_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub(crate) const MODEL_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const CHILD_GRACEFUL_SHUTDOWN_POLL_ATTEMPTS: usize = 40;
const CHILD_FORCED_SHUTDOWN_POLL_ATTEMPTS: usize = 8;
const CHILD_SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(25);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(20);
pub(crate) type RunId = u64;

/// A running `echoless run` child. Dropping it stops the process: stdin is
/// closed first (EOF asks the CLI to shut down), then the process gets a
/// bounded grace period before it is killed, and its config file is removed.
pub(crate) struct RunChild {
    pub(crate) run_id: RunId,
    pub(crate) child: Child,
    /// Set when the GUI stops the run, so the stdout reader can tell an
    /// intentional stop from a crash.
    pub(crate) stopping: Arc<AtomicBool>,
    stdin: Option<Arc<Mutex<ChildStdin>>>,
    config_path: PathBuf,
}

impl RunChild {
    pub(crate) fn new(run_id: RunId, mut child: Child, config_path: PathBuf) -> Self {
        let stdin = child.stdin.take().map(|stdin| Arc::new(Mutex::new(stdin)));
        Self {
            run_id,
            child,
            stopping: Arc::default(),
            stdin,
            config_path,
        }
    }
}

impl Drop for RunChild {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // A control write in flight keeps its own clone of stdin. EOF then
        // arrives when that write ends, or the kill below breaks the pipe.
        drop(self.stdin.take());
        shutdown_child_gracefully(&mut self.child);
        crate::platform::cleanup_run_config(&self.config_path);
    }
}

/// Closes stdin, waits up to about 1 s for the child to exit, then kills it
/// and waits up to about 0.2 s more. Never blocks without a bound.
pub(crate) fn shutdown_child_gracefully(child: &mut Child) {
    drop(child.stdin.take());
    if !wait_for_exit(child, CHILD_GRACEFUL_SHUTDOWN_POLL_ATTEMPTS) {
        let _ = child.kill();
        let _ = wait_for_exit(child, CHILD_FORCED_SHUTDOWN_POLL_ATTEMPTS);
    }
}

fn wait_for_exit(child: &mut Child, attempts: usize) -> bool {
    for _ in 0..attempts {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(CHILD_SHUTDOWN_POLL_INTERVAL),
            Err(_) => return false,
        }
    }
    false
}

/// The current `echoless run` child. Start, stop, reader finalization and app
/// exit all run on the Tauri main thread, so they never interleave; the lock
/// only guards against the reader threads that check `is_active_run`.
struct RunStateInner {
    child: Option<RunChild>,
    next_run_id: RunId,
}

pub(crate) struct RunState(Mutex<RunStateInner>);

impl Default for RunState {
    fn default() -> Self {
        Self(Mutex::new(RunStateInner {
            child: None,
            next_run_id: 1,
        }))
    }
}

fn run_state_guard(state: &RunState) -> MutexGuard<'_, RunStateInner> {
    state.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Allocates the next run id and takes the previous child. The caller must
/// drop the returned child before it spawns the new one, so the old process
/// releases its audio devices first.
pub(crate) fn begin_run(state: &RunState) -> (RunId, Option<RunChild>) {
    let mut guard = run_state_guard(state);
    let run_id = guard.next_run_id;
    guard.next_run_id += 1;
    (run_id, guard.child.take())
}

pub(crate) fn install_run(state: &RunState, run: RunChild) {
    run_state_guard(state).child = Some(run);
}

/// Readers use this to drop status, log and exit events from older runs.
pub(crate) fn is_active_run(state: &RunState, run_id: RunId) -> bool {
    run_state_guard(state)
        .child
        .as_ref()
        .is_some_and(|run| run.run_id == run_id)
}

/// Takes the child only if it still belongs to `run_id`. A reader of a run
/// that was already stopped or replaced gets `None` and must not touch the
/// newer run.
pub(crate) fn take_finished_run(state: &RunState, run_id: RunId) -> Option<RunChild> {
    let mut guard = run_state_guard(state);
    if guard.child.as_ref()?.run_id != run_id {
        return None;
    }
    guard.child.take()
}

/// Stops the current run, if any, and returns its id. The child is dropped
/// outside the lock.
pub(crate) fn terminate_run(state: &RunState) -> Option<RunId> {
    let run = run_state_guard(state).child.take();
    run.map(|run| run.run_id)
}

pub(crate) fn run_control_stdin(state: &RunState) -> Result<Arc<Mutex<ChildStdin>>, String> {
    run_state_guard(state)
        .child
        .as_ref()
        .and_then(|run| run.stdin.clone())
        .ok_or_else(|| "not running".to_string())
}

pub(crate) fn write_run_control(stdin: &Mutex<ChildStdin>, line: &str) -> Result<(), String> {
    let mut stdin = stdin.lock().unwrap_or_else(PoisonError::into_inner);
    stdin
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|()| stdin.flush())
        .map_err(|err| format!("write run control failed: {err}"))
}

/// Runs a one-shot command and returns its output.
///
/// Stdout and stderr are read while the child runs, so a full pipe cannot
/// stall it. Each stderr line goes to `on_stderr_line`; lines for which it
/// returns `false` are kept in `Output::stderr`, limited to the last
/// `STREAM_TAIL_LIMIT_BYTES`. On timeout the whole process tree is killed.
pub(crate) fn command_output_with_timeout(
    command: &mut Command,
    timeout: Duration,
    label: &str,
    mut on_stderr_line: impl FnMut(&str) -> bool + Send + 'static,
) -> Result<Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    suppress_child_console(command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn {label} failed: {e}"))?;

    let stdout = child.stdout.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_end(&mut bytes);
        }
        bytes
    });
    let stderr = child.stderr.take();
    let stderr_reader = std::thread::spawn(move || {
        let mut tail = String::new();
        let Some(stderr) = stderr else { return tail };
        for line in BufReader::new(stderr).split(b'\n') {
            let Ok(line) = line else { break };
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches('\r');
            if !on_stderr_line(line) {
                push_tail_line(&mut tail, line, STREAM_TAIL_LIMIT_BYTES);
            }
        }
        tail
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(COMMAND_POLL_INTERVAL),
            Ok(None) => {
                kill_child_tree(&mut child);
                let _ = child.wait();
                break None;
            }
            Err(e) => {
                kill_child_tree(&mut child);
                return Err(format!("wait {label} failed: {e}"));
            }
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let Some(status) = status else {
        return Err(format!(
            "{label} timed out after {}s; stderr: {}",
            timeout.as_secs(),
            stderr.trim()
        ));
    };
    Ok(Output {
        status,
        stdout,
        stderr: stderr.into_bytes(),
    })
}

#[cfg(windows)]
fn kill_child_tree(child: &mut Child) {
    let pid = child.id().to_string();
    let mut taskkill = Command::new("taskkill");
    taskkill.args(["/PID", &pid, "/T", "/F"]);
    suppress_child_console(&mut taskkill);
    let _ = taskkill.status();
    let _ = child.kill();
}

#[cfg(not(windows))]
fn kill_child_tree(child: &mut Child) {
    let _ = child.kill();
}

pub(crate) fn command_status_error(label: &str, out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let detail = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    // The message is shown in the UI; keep long output from flooding it.
    let detail: String = if detail.chars().count() > 240 {
        let head: String = detail.chars().take(240).collect();
        format!("{head}…")
    } else {
        detail.to_string()
    };
    format!(
        "{label} failed with status {}; output: {detail}",
        out.status
    )
}

pub(crate) fn parse_json_output(label: &str, out: Output) -> Result<Value, String> {
    if !out.status.success() {
        return Err(command_status_error(label, &out));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("parse json failed: {e}; raw: {stdout}"))
}

#[derive(Debug, PartialEq)]
pub(crate) enum JsonlLineEvent {
    Empty,
    Json(Value),
    Unparsed(String),
}

pub(crate) fn parse_jsonl_line_event(line: &str) -> JsonlLineEvent {
    if line.trim().is_empty() {
        return JsonlLineEvent::Empty;
    }
    match serde_json::from_str::<Value>(line) {
        Ok(value) => JsonlLineEvent::Json(value),
        Err(_) => JsonlLineEvent::Unparsed(line.to_string()),
    }
}

pub(crate) fn push_tail_line(tail: &mut String, line: &str, limit_bytes: usize) {
    tail.push_str(line);
    tail.push('\n');
    if tail.len() <= limit_bytes {
        return;
    }
    let cut_at_least = tail.len() - limit_bytes;
    let cut = tail
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= cut_at_least)
        .unwrap_or(tail.len());
    tail.drain(..cut);
}

/// Runs a one-shot JSON subcommand (devices / processors / config validate).
pub(crate) fn run_json_blocking(
    app: Option<&tauri::AppHandle>,
    args: &[&str],
    timeout: Duration,
    label: &str,
) -> Result<Value, String> {
    let mut command = echoless_command(app)?;
    command.args(args);
    let out = command_output_with_timeout(&mut command, timeout, label, |_| false)?;
    parse_json_output(label, out)
}

pub(crate) async fn run_json_async(
    app: tauri::AppHandle,
    args: Vec<String>,
    timeout: Duration,
    label: &'static str,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        run_json_blocking(Some(&app), &arg_refs, timeout, label)
    })
    .await
    .map_err(|e| format!("{label} task join failed: {e}"))?
}
