// 崩溃取证日志:落盘到 <brand_data_root>/logs/echoless-<UTC时间戳>.log。
//
// 动机(2026-07-09 黑屏 RCA):此前全部诊断信息都是易失的 —— CLI stderr 只转发
// 成 echoless://log 事件、前端错误只进 DevTools console,app 一关什么都不剩,
// 用户报障只能现场连调试器。本模块给「用户把日志文件发过来」这条路。
//
// 约束(防膨胀):
//   - 每次启动一个新文件(定位「哪次运行出的事」天然清晰);
//   - 启动时清理:mtime 超过 KEEP_DAYS 的删掉,再按新旧保留 KEEP_FILES 个;
//   - 单文件 MAX_BYTES 封顶,超限写一行截断标记后本次不再落盘
//     (防 stderr 风暴刷爆磁盘;事件转发不受影响,UI 照常)。
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const KEEP_DAYS: u64 = 7;
const KEEP_FILES: usize = 20;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const CREATE_ATTEMPTS: usize = 1_000;

struct Sink {
    file: File,
    written: u64,
    truncated: bool,
}

static SINK: OnceLock<Mutex<Option<Sink>>> = OnceLock::new();
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 进程启动时调用一次。失败静默(日志是辅助设施,绝不影响主功能)。
pub(crate) fn init(app_version: &str) {
    let (base, _) = echoless_paths::brand_data_root();
    let dir = base.join("logs");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    prune(&dir);
    let Ok((_path, sink)) = create_unique_sink(&dir, &file_stamp(), std::process::id()) else {
        return;
    };
    let _ = LOG_DIR.set(dir);
    let _ = SINK.set(Mutex::new(Some(sink)));
    log(
        "info",
        "app",
        &format!(
            "echoless {app_version} started · os={} arch={}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );
}

/// 追加一行:`2026-07-09 10:15:30Z [level] source: msg`。
pub(crate) fn log(level: &str, source: &str, msg: &str) {
    let Some(lock) = SINK.get() else { return };
    let Ok(mut guard) = lock.lock() else { return };
    let Some(sink) = guard.as_mut() else { return };
    append_line(
        sink,
        &format!("{} [{level}] {source}: {msg}\n", line_stamp()),
        MAX_BYTES,
    );
}

fn append_line(sink: &mut Sink, line: &str, max_bytes: u64) {
    if sink.truncated {
        return;
    }
    let bytes = line.len() as u64;
    if sink.written + bytes > max_bytes {
        let _ = sink.file.write_all(
            format!("{} [warn] log: size cap reached, truncated\n", line_stamp()).as_bytes(),
        );
        sink.truncated = true;
        return;
    }
    if sink.file.write_all(line.as_bytes()).is_ok() {
        sink.written += bytes;
    }
}

fn create_unique_sink(dir: &Path, stamp: &str, pid: u32) -> io::Result<(PathBuf, Sink)> {
    for attempt in 0..CREATE_ATTEMPTS {
        let path = dir.join(format!("echoless-{stamp}-p{pid}-{attempt:03}.log"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                return Ok((
                    path,
                    Sink {
                        file,
                        written: 0,
                        truncated: false,
                    },
                ));
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("failed to reserve a unique log file after {CREATE_ATTEMPTS} attempts"),
    ))
}

/// 清理:先删超龄,再按 mtime 新→旧只保留 KEEP_FILES-1 个(本次启动还要新建一个)。
fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    let mut files: Vec<(PathBuf, SystemTime)> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?;
            if !(name.starts_with("echoless-") && name.ends_with(".log")) {
                return None;
            }
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((p, mtime))
        })
        .collect();
    files.retain(|(p, mtime)| {
        let expired = now
            .duration_since(*mtime)
            .map(|age| age.as_secs() > KEEP_DAYS * 86_400)
            .unwrap_or(false);
        if expired {
            let _ = fs::remove_file(p);
        }
        !expired
    });
    files.sort_by_key(|entry| std::cmp::Reverse(entry.1)); // 新在前
    for (p, _) in files.into_iter().skip(KEEP_FILES.saturating_sub(1)) {
        let _ = fs::remove_file(p);
    }
}

/// 前端错误汇入同一文件(ErrorBoundary / window.onerror / unhandledrejection)。
#[tauri::command]
pub(crate) fn frontend_log(level: String, message: String) {
    let lv = match level.as_str() {
        "error" | "warn" | "info" => level.as_str(),
        _ => "info",
    };
    // 单条截断:前端可能塞整个组件栈,给 8 KB 足够定位且不失控。
    let msg: String = message.chars().take(8192).collect();
    log(lv, "frontend", &msg);
}

// ---- UTC 时间戳(不引 chrono:civil-from-days 算法,够用) ----

fn now_parts() -> (u64, u64, u64, u64, u64, u64, u32) {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = elapsed.as_secs();
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let rem = secs % 86_400;
    (
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        elapsed.subsec_nanos(),
    )
}

fn line_stamp() -> String {
    let (y, mo, d, h, mi, s, _) = now_parts();
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}Z")
}

fn file_stamp() -> String {
    let (y, mo, d, h, mi, s, nanos) = now_parts();
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}-{nanos:09}")
}

/// Howard Hinnant 的 civil_from_days(公历,proleptic)。
fn civil_from_days(z: i64) -> (u64, u64, u64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Barrier};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "echoless-logging-{label}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test log directory");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // 2024-01-01
        assert_eq!(civil_from_days(20_643), (2026, 7, 9)); // 本 RCA 当天
    }

    #[test]
    fn same_stamp_concurrent_creation_reserves_distinct_files() {
        const WORKERS: usize = 8;
        let dir = TestDir::new("concurrent");
        let path = Arc::new(dir.0.clone());
        let barrier = Arc::new(Barrier::new(WORKERS));
        let workers: Vec<_> = (0..WORKERS)
            .map(|_| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let (path, _sink) =
                        create_unique_sink(&path, "20260710-010203-000000000", 1234)
                            .expect("reserve unique log");
                    path
                })
            })
            .collect();

        let paths: HashSet<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker must finish"))
            .collect();

        assert_eq!(paths.len(), WORKERS);
        assert!(paths.iter().all(|path| path.exists()));
    }

    #[test]
    fn colliding_sessions_keep_independent_size_caps() {
        let dir = TestDir::new("caps");
        let (_, mut first) = create_unique_sink(&dir.0, "same", 7).unwrap();
        let (_, mut second) = create_unique_sink(&dir.0, "same", 7).unwrap();

        append_line(&mut first, "1234\n", 5);
        append_line(&mut first, "x\n", 5);
        append_line(&mut second, "z\n", 5);

        assert_eq!(first.written, 5);
        assert!(first.truncated);
        assert_eq!(second.written, 2);
        assert!(!second.truncated);
    }
}
