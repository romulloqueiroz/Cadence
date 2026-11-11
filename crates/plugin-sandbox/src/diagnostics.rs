//! Failure records are written only by startup supervision/the watchdog, never
//! by the audio callback. The first failure reason is retained atomically.
use crate::shared::PipelineMonitor;
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureReason {
    None = 0,
    InvalidAudio = 1,
    QueueFull = 2,
    Protocol = 3,
    Notification = 4,
    ProcessTimeout = 5,
    CommandTimeout = 6,
    DrainTimeout = 7,
    WorkerExited = 8,
    PipelineTimeout = 9,
    WorkerReported = 10,
    Cancelled = 11,
    Startup = 12,
}
impl FailureReason {
    pub fn from_code(code: u32) -> Self {
        match code {
            0 => Self::None,
            1 => Self::InvalidAudio,
            2 => Self::QueueFull,
            3 => Self::Protocol,
            4 => Self::Notification,
            5 => Self::ProcessTimeout,
            6 => Self::CommandTimeout,
            7 => Self::DrainTimeout,
            8 => Self::WorkerExited,
            9 => Self::PipelineTimeout,
            10 => Self::WorkerReported,
            11 => Self::Cancelled,
            12 => Self::Startup,
            _ => Self::Protocol,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::InvalidAudio => "invalid_audio",
            Self::QueueFull => "queue_overflow",
            Self::Protocol => "protocol_error",
            Self::Notification => "notification_failed",
            Self::ProcessTimeout => "audio_timeout",
            Self::CommandTimeout => "command_timeout",
            Self::DrainTimeout => "drain_timeout",
            Self::WorkerExited => "worker_exited",
            Self::PipelineTimeout => "pipeline_timeout",
            Self::WorkerReported => "worker_reported_error",
            Self::Cancelled => "cancelled",
            Self::Startup => "startup_failed",
        }
    }
}

pub fn default_directory() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CADENCE_WORKER_LOG_DIR") {
        return (!path.is_empty()).then(|| PathBuf::from(path));
    }
    std::env::var_os("HOME").map(|home| {
        #[cfg(target_os = "macos")]
        let suffix = "Library/Logs/Cadence";
        #[cfg(not(target_os = "macos"))]
        let suffix = ".local/state/cadence";
        PathBuf::from(home).join(suffix)
    })
}

pub fn record(
    directory: Option<&Path>,
    plugin: &str,
    pid: u32,
    reason: FailureReason,
    monitor: &PipelineMonitor,
    exit: Option<(i32, i32)>,
) {
    if matches!(reason, FailureReason::None | FailureReason::Cancelled) {
        return;
    }
    let Some(directory) = directory else {
        return;
    };
    static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Ok(_guard) = WRITE_LOCK.lock() else {
        return;
    };
    let parent = std::process::id();
    let published = monitor.published();
    let completed = monitor.completed();
    let data = serde_json::json!({
        "version": 1, "unix_millis": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "parent_pid": parent, "worker_pid": pid, "plugin_id": plugin,
        "reason": reason.name(), "reason_code": reason as u32,
        "published": published, "completed": completed,
        "pending_packets": published.saturating_sub(completed),
        "latency_frames": monitor.latency(), "underruns": monitor.underruns(),
        "missed_frames": monitor.late_frames(),
        "exit_kind": exit.map(|v| v.0), "exit_status": exit.map(|v| v.1),
    });
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(directory)?;
        let path = directory.join(format!("worker-failures-{parent}.jsonl"));
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1_048_576) {
            std::fs::rename(
                &path,
                directory.join(format!("worker-failures-{parent}.previous.jsonl")),
            )?;
        }
        let mut bytes = serde_json::to_vec(&data)?;
        bytes.push(b'\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(&bytes)
    })();
    if let Err(error) = result {
        eprintln!("Cadence worker diagnostic could not be saved: {error}");
    }
}

/// Observe without reaping: the owner retains the child PID until destruction,
/// so the watchdog can never signal a recycled PID.
pub fn exited(pid: u32) -> Option<(i32, i32)> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    // macOS can report a stop event here even with WEXITED. A suspended
    // process is still alive: only terminal child-status codes are exits.
    if result == 0
        && unsafe { info.si_pid() } == pid as i32
        && matches!(
            info.si_code,
            libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
        )
    {
        Some((info.si_code, unsafe { info.si_status() }))
    } else {
        None
    }
}
