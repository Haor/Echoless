//! stdout 异步发射器(审计 B-02)。
//!
//! JSONL 状态/命令回执的写出不允许发生在 10ms 音频处理线程上:sidecar
//! 场景 stdout 是管道,GUI 侧读线程一旦短暂卡顿(GC/窗口拖动/重排),
//! 管道缓冲写满后同步 `println!` 会阻塞处理循环 → 输出欠载爆音。
//! 处理线程只做 `try_send`,全局单例 IO 线程负责真正写出;队列满则丢弃
//! 本条(状态行是幂等快照,丢了下一条补;宁丢状态不卡音频),首次丢弃
//! 告警到 stderr 并计数。

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::OnceLock;
use std::thread;

const QUEUE_CAPACITY: usize = 256;

static SENDER: OnceLock<SyncSender<String>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);

pub(super) fn emit_stdout_line(line: String) {
    let sender = SENDER.get_or_init(|| {
        let (tx, rx) = sync_channel::<String>(QUEUE_CAPACITY);
        thread::Builder::new()
            .name("stdout-emitter".into())
            .spawn(move || {
                let stdout = std::io::stdout();
                for line in rx {
                    let mut lock = stdout.lock();
                    // 行缓冲:writeln 的换行触发 flush,保持一行一事件的原子性。
                    let _ = writeln!(lock, "{line}");
                }
            })
            .expect("spawn stdout emitter thread");
        tx
    });
    match sender.try_send(line) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            if DROPPED.fetch_add(1, Ordering::Relaxed) == 0 {
                eprintln!("status emitter queue full; dropping status lines (consumer too slow)");
            }
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_does_not_block_and_survives_burst() {
        // 远超队列容量的突发也不得阻塞调用方(满则丢弃)。
        for i in 0..(QUEUE_CAPACITY * 4) {
            emit_stdout_line(format!("{{\"type\":\"test\",\"seq\":{i}}}"));
        }
    }
}
