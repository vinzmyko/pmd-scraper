use std::{
    fs,
    path::PathBuf,
};

use serde_json::json;

use crate::phases::PhaseId;

pub struct ProgressReporter {
    path: Option<PathBuf>,
    phase: Option<PhaseId>,
    current: usize,
    total: usize,
}

impl ProgressReporter {
    /// `None` disables reporting entirely; every method becomes a no-op.
    pub fn new(path: Option<PathBuf>) -> Self {
        ProgressReporter {
            path,
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
        self.current += 1;
        self.write("running", None);
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
