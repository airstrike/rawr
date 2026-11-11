//! A run's `RAWR_MCP_EVENTS` file, read as it grows. A supervised `rawr
//! run` whose program fails its check says `failed` there, and one whose
//! program ends says `ended`; either then waits for the next save, which a
//! test never makes, so a wait reads the file to end on them (#1585).

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

/// What a run's events say that ends a wait for anything else.
pub(super) enum News {
    /// The rendered message of a `failed` event: the run waits for a save.
    Failed(String),
    /// An `ended` event: the program is gone, and the run waits for a save.
    Ended(String),
}

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

    /// What the run said since the last look, among the whole lines it
    /// wrote: a `failed` event first, then an `ended` one.
    pub fn news(&mut self) -> Option<News> {
        let text = self.text();
        let fresh = text.get(self.heard..).unwrap_or(&text);
        let whole = fresh.rfind('\n').map_or(0, |end| end + 1);
        let events = fresh[..whole]
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Vec<_>>();
        self.heard = text.len() - fresh.len() + whole;
        let failed = events.iter().find(|event| event["event"] == "failed");
        let ended = events.iter().find(|event| event["event"] == "ended");
        match (failed, ended) {
            (Some(failed), _) => Some(News::Failed(
                failed["rendered"]
                    .as_str()
                    .map_or_else(|| failed.to_string(), str::to_owned),
            )),
            (None, Some(ended)) => Some(News::Ended(ended.to_string())),
            (None, None) => None,
        }
    }

    /// Count the whole lines of `text`, which a caller read and took, as
    /// heard: a failure it waited for ends no later wait.
    pub fn hear(&mut self, text: &str) {
        self.heard = text.rfind('\n').map_or(0, |end| end + 1);
    }
}
