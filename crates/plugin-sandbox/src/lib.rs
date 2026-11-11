//! Out-of-process plugin supervisor.
//!
//! The host spawns a worker process that loads a native plugin. Audio, events
//! and state travel through shared memory. This is the isolation boundary: a crashed
//! or runaway plugin cannot take down the host.
//!
//! The realtime bridge uses a preallocated packet ring with one prepared buffer
//! of latency and never waits for the worker. Control and offline/CLI processing
//! use bounded synchronous exchanges. Plugin code runs only in the worker;
//! a separate watchdog terminates stalled workers.

pub mod automation;
pub mod diagnostics;
#[cfg(target_os = "macos")]
pub mod gui;
pub mod host;
pub mod metadata;
mod pipeline;
pub mod shared;
mod telemetry;
pub mod worker;
pub mod workgroup;

pub use host::SandboxPlugin;

use std::fmt;

#[derive(Debug)]
pub enum SandboxError {
    Io(std::io::Error),
    SharedMemory(String),
    Spawn(String),
    Worker(String),
    Protocol(String),
    Cancelled,
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxError::Io(error) => write!(f, "I/O error: {error}"),
            SandboxError::SharedMemory(message) => write!(f, "shared memory: {message}"),
            SandboxError::Spawn(message) => write!(f, "spawn failed: {message}"),
            SandboxError::Worker(message) => write!(f, "worker error: {message}"),
            SandboxError::Protocol(message) => write!(f, "protocol error: {message}"),
            SandboxError::Cancelled => write!(f, "Loading cancelled"),
        }
    }
}

impl std::error::Error for SandboxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SandboxError::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SandboxError {
    fn from(error: std::io::Error) -> Self {
        SandboxError::Io(error)
    }
}
