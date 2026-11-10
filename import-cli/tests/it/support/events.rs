//! A run's `RAWR_MCP_EVENTS` file, read as it grows. A supervised `rawr
//! run` whose program fails its check says `failed` there and waits for
//! the next save, which a test never makes, so a wait reads the file to
//! end on the failure (#1585).

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

/// The events file a run writes, and how much of it a wait has read.
pub(super) struct Events {
    path: PathBuf,
    heard: usize,
}

impl Events {
    pub const fn new(path: PathBuf) -> Self {
        Self { path, heard: 0 }
    }

    /// The file's text now: empty before the run writes it.
    pub fn text(&self) -> String {
        fs::read_to_string(&self.path).unwrap_or_default()
    }

    /// The rendered message of a `failed` event among the whole lines the
    /// run wrote since the last look.
    pub fn failure(&mut self) -> Option<String> {
        let text = self.text();
        let fresh = text.get(self.heard..).unwrap_or(&text);
        let whole = fresh.rfind('\n').map_or(0, |end| end + 1);
        let failure = fresh[..whole]
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| event["event"] == "failed")
            .map(|event| {
                event["rendered"]
                    .as_str()
                    .map_or_else(|| event.to_string(), str::to_owned)
            });
        self.heard = text.len() - fresh.len() + whole;
        failure
    }

    /// Count the whole lines of `text`, which a caller read and took, as
    /// heard: a failure it waited for ends no later wait.
    pub fn hear(&mut self, text: &str) {
        self.heard = text.rfind('\n').map_or(0, |end| end + 1);
    }
}
