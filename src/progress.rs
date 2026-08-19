use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::json;

use crate::phases::PhaseId;

/// The client touches its heartbeat at 2Hz. 10s is 20 missed ticks.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);

/// Missing file counts as stale: the client deletes it on a clean stop, and a
/// crashed client never created one.
fn heartbeat_is_stale(path: &Path, timeout: Duration) -> bool {
    match fs::metadata(path).and_then(|m| m.modified()) {
        // Err from elapsed() means mtime is in the future — clock skew, treat as fresh.
        Ok(mtime) => mtime.elapsed().map(|age| age > timeout).unwrap_or(false),
        Err(_) => true,
    }
}

pub struct ProgressReporter {
    path: Option<PathBuf>,
    heartbeat: Option<PathBuf>,
    phase: Option<PhaseId>,
    current: usize,
    total: usize,
}

impl ProgressReporter {
    /// `path: None` disables reporting. `heartbeat: None` disables the liveness check.
    pub fn new(path: Option<PathBuf>, heartbeat: Option<PathBuf>) -> Self {
        ProgressReporter {
            path,
            heartbeat,
            phase: None,
            current: 0,
            total: 0,
        }
    }

    /// Called by whoever owns the work list, once it knows the count.
    pub fn begin(&mut self, phase: PhaseId, total: usize) {
        self.phase = Some(phase);
        self.current = 0;
        self.total = total;
        self.write("running", None);
    }

    pub fn advance(&mut self) {
        self.exit_if_abandoned();
        self.current += 1;
        self.write("running", None);
    }

    /// Deliberately writes nothing before exiting: the output tree stays
    /// unmarked so a recovering client rescrapes rather than trusting it.
    fn exit_if_abandoned(&self) {
        let Some(hb) = &self.heartbeat else { return };
        if heartbeat_is_stale(hb, HEARTBEAT_TIMEOUT) {
            eprintln!("Client heartbeat stale, exiting");
            std::process::exit(0);
        }
    }

    /// Keeps the last real phase name so the terminal state isn't blank.
    pub fn complete(&mut self) {
        self.write("complete", None);
    }

    pub fn fail(&mut self, message: &str) {
        self.write("failed", Some(message));
    }

    fn write(&self, status: &str, message: Option<&str>) {
        let path = match &self.path {
            Some(p) => p,
            None => return,
        };

        let mut payload = json!({
            "phases": PhaseId::ALL.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            "phase": self.phase.map(|p| p.as_str()).unwrap_or(""),
            "current": self.current,
            "total": self.total,
            "status": status,
        });
        if let Some(msg) = message {
            payload["message"] = json!(msg);
        }

        // tmp+rename: the file grew with `phases`, and the client polls at 2Hz.
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, payload.to_string()).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}
