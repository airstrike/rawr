//! The server's loop. The calling thread reads each message and sends it
//! to the server's thread, which takes each message, each session's answer
//! and each watcher event in turn, and ends at `exit` or once the client's
//! input ended.

use std::io::BufRead;
use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::{json, Value};

use super::{document, Heard, Server};

/// The error code of a request the server doesn't know.
const UNKNOWN: i64 = -32601;

/// The error code of a request before `initialize`.
const UNINITIALIZED: i64 = -32002;

/// The error code of a request after `shutdown`.
const INVALID: i64 = -32600;

/// Serve the protocol on stdin and stdout until the client exits. `given`
/// names the first program, over the client's root.
///
/// # Errors
///
/// When stdin can't be read.
pub fn serve(given: Option<PathBuf>) -> std::io::Result<()> {
    serve_on(given, std::io::stdin().lock(), std::io::stdout())
}

/// [`serve`] over any reader and writer.
///
/// # Errors
///
/// When `input` can't be read.
pub fn serve_on(
    given: Option<PathBuf>,
    mut input: impl BufRead,
    output: impl std::io::Write + Send + 'static,
) -> std::io::Result<()> {
    let (heard, hearing) = mpsc::channel();
    let telling = heard.clone();
    let server = std::thread::Builder::new()
        .name("rawr-lsp".into())
        .spawn(move || Server::new(Box::new(output), telling, given).serve(&hearing))?;
    let read = loop {
        match crate::dap::wire::read(&mut input) {
            Ok(Some(message)) => {
                let last = message["method"] == "exit";
                let _ = heard.send(Heard::Message(message));
                if last {
                    break Ok(());
                }
            }
            Ok(None) => break Ok(()),
            Err(error) => break Err(error),
        }
    };
    let _ = heard.send(Heard::Ended);
    let _ = server.join();
    read
}

impl Server {
    /// Take what the server hears until it is over.
    pub(super) fn serve(mut self, hearing: &mpsc::Receiver<Heard>) {
        for heard in hearing {
            match heard {
                Heard::Message(message) => self.take(message),
                Heard::Ended => self.over = true,
                Heard::Session { id, told } => {
                    if let Some(then) = self.then.remove(&id) {
                        then(&mut self, told);
                    }
                }
                Heard::Sockets(sockets) => self.sockets(&sockets),
            }
            if self.over {
                break;
            }
        }
        self.detach_all();
    }

    /// Take one message: a request, a notification, or the client's answer
    /// to a request of the server's, which needs nothing.
    fn take(&mut self, mut message: Value) {
        let Some(method) = message["method"].as_str().map(str::to_owned) else {
            return;
        };
        let params = message["params"].take();
        let id = message["id"].take();
        if id.is_null() {
            return self.heed(&method, &params);
        }
        match method.as_str() {
            "initialize" => {
                let result = self.initialize(&params);
                self.respond(&id, result);
            }
            _ if !self.initialized => {
                self.refuse(&id, UNINITIALIZED, "the client hasn't initialized")
            }
            _ if self.shut => self.refuse(&id, INVALID, "the client shut the server down"),
            "shutdown" => {
                self.shut = true;
                self.respond(&id, Value::Null);
            }
            "textDocument/hover" => self.hover(id, &params),
            "textDocument/inlayHint" => self.inlay_hints(&id, &params),
            method => self.refuse(&id, UNKNOWN, &format!("rawr lsp doesn't serve `{method}`")),
        }
    }

    /// The error answer to request `id`.
    fn refuse(&mut self, id: &Value, code: i64, message: &str) {
        self.send(json!({ "id": id, "error": { "code": code, "message": message } }));
    }

    /// One notification.
    fn heed(&mut self, method: &str, params: &Value) {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        match method {
            "exit" => self.over = true,
            "textDocument/didOpen" => {
                let text = params["textDocument"]["text"].as_str().unwrap_or_default();
                let Some(document) = document::Document::new(uri, text.to_owned()) else {
                    return;
                };
                let path = document.path.clone();
                self.documents.insert(uri.to_owned(), document);
                self.program_of(&path);
            }
            "textDocument/didChange" => {
                let text = params["contentChanges"]
                    .as_array()
                    .and_then(|changes| changes.last())
                    .and_then(|change| change["text"].as_str());
                if let (Some(document), Some(text)) = (self.documents.get_mut(uri), text) {
                    document.replace(text.to_owned());
                }
            }
            "textDocument/didSave" => {
                if let Some(path) = self
                    .documents
                    .get(uri)
                    .map(|document| document.path.clone())
                {
                    self.program_of(&path);
                }
            }
            "textDocument/didClose" => {
                self.documents.remove(uri);
            }
            _ => {}
        }
    }

    /// `initialize`: take the client's encodings and refreshes, find the
    /// program at its root, and answer what the server serves.
    fn initialize(&mut self, params: &Value) -> Value {
        self.initialized = true;
        let capabilities = &params["capabilities"];
        self.encoding = document::Encoding::chosen(&capabilities["general"]["positionEncodings"]);
        self.refreshes = capabilities["workspace"]["inlayHint"]["refreshSupport"] == true;
        let root = self
            .given
            .clone()
            .or_else(|| params["rootUri"].as_str().and_then(document::path))
            .or_else(|| {
                params["workspaceFolders"][0]["uri"]
                    .as_str()
                    .and_then(document::path)
            })
            .or_else(|| std::env::current_dir().ok());
        if let Some(root) = root {
            self.program_of(&root);
        }
        json!({
            "capabilities": {
                "positionEncoding": self.encoding.name(),
                // The whole text at each change.
                "textDocumentSync": { "openClose": true, "change": 1, "save": true },
                "hoverProvider": true,
                "inlayHintProvider": true,
            },
            "serverInfo": { "name": "rawr", "version": env!("CARGO_PKG_VERSION") },
        })
    }
}
