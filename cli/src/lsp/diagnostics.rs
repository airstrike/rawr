//! `textDocument/publishDiagnostics`: why the last save didn't swap into
//! the running program, as rawr's own diagnostics, until a save does.
//! rustc's diagnostics among them are left to rust-analyzer, which shows
//! them already.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::{document, Server};

impl Server {
    /// Show `failure`, a failed save's diagnostics or null, for link
    /// `serial` in place of what it showed.
    pub(super) fn diagnose(&mut self, serial: u64, failure: Value) {
        let Some(link) = self.link(serial) else {
            return;
        };
        if link.failure == failure {
            return;
        }
        let gone = std::mem::take(&mut link.published);
        link.failure = failure.clone();
        let mut files: BTreeMap<PathBuf, Vec<Value>> = BTreeMap::new();
        for diagnostic in failure.as_array().into_iter().flatten() {
            if diagnostic["source"] == "rustc" {
                continue;
            }
            let Some(file) = diagnostic["file"].as_str() else {
                continue;
            };
            let file = std::fs::canonicalize(file).unwrap_or_else(|_| PathBuf::from(file));
            let shown = self.shown(&file, diagnostic);
            files.entry(file).or_default().push(shown);
        }
        let mut published = Vec::new();
        for (file, diagnostics) in files {
            let uri = self.uri_of(&file);
            self.notify(
                "textDocument/publishDiagnostics",
                json!({ "uri": uri, "diagnostics": diagnostics }),
            );
            published.push(uri);
        }
        for uri in gone.into_iter().filter(|uri| !published.contains(uri)) {
            self.notify(
                "textDocument/publishDiagnostics",
                json!({ "uri": uri, "diagnostics": [] }),
            );
        }
        if let Some(link) = self.link(serial) {
            link.published = published;
        }
    }

    /// `diagnostic`, one of rawr's in `file`, as the protocol shows it.
    fn shown(&self, file: &std::path::Path, diagnostic: &Value) -> Value {
        let text = self.text_of(file).unwrap_or_default();
        let number = |key: &str| {
            diagnostic[key]
                .as_u64()
                .and_then(|number| usize::try_from(number).ok())
        };
        let line = number("line").unwrap_or(1);
        let column = number("column").unwrap_or(1);
        let end_line = number("end_line").unwrap_or(line);
        let end_column = number("end_column").unwrap_or(column);
        let at = |line: usize, column: usize| {
            json!({
                "line": line.saturating_sub(1),
                "character": self.encoding.character(document::line(&text, line), column),
            })
        };
        let severity = match diagnostic["severity"].as_str().unwrap_or_default() {
            "warning" => 2,
            "note" => 3,
            "help" => 4,
            _ => 1,
        };
        let mut message = diagnostic["message"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if let Some(help) = diagnostic["help"].as_str() {
            message.push('\n');
            message.push_str(help);
        }
        let mut shown = json!({
            "range": { "start": at(line, column), "end": at(end_line, end_column) },
            "severity": severity,
            "source": "rawr",
            "message": message,
        });
        if let Some(code) = diagnostic["code"].as_str() {
            shown["code"] = json!(code);
        }
        shown
    }

    /// The URI the client names `file` by: its own while it has the file
    /// open.
    fn uri_of(&self, file: &std::path::Path) -> String {
        self.documents
            .iter()
            .find(|(_, document)| document.path == file)
            .map_or_else(|| document::uri(file), |(uri, _)| uri.clone())
    }
}
