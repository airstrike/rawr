//! The adapter's loop. The calling thread reads each request and sends it
//! to the adapter's thread, which takes each request and each answer of
//! the session in turn, and ends once the client disconnected or its input
//! ended and the program it started exited.

use std::io::BufRead;
use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::{json, Value};

use super::{wire, Adapter, Heard, THREAD};

/// Serve the protocol on stdin and stdout until the client disconnects.
/// Relative paths start at `root`.
///
/// # Errors
///
/// When stdin can't be read.
pub fn serve(root: PathBuf) -> std::io::Result<()> {
    serve_on(root, std::io::stdin().lock(), std::io::stdout())
}

/// [`serve`] over any reader and writer.
///
/// # Errors
///
/// When `input` can't be read.
pub fn serve_on(
    root: PathBuf,
    mut input: impl BufRead,
    output: impl std::io::Write + Send + 'static,
) -> std::io::Result<()> {
    let out = wire::Writer::new(output)?;
    let (heard, hearing) = mpsc::channel();
    let telling = heard.clone();
    let adapter = std::thread::Builder::new()
        .name("rawr-dap".into())
        .spawn(move || Adapter::new(root, out, telling).serve(&hearing))?;
    let read = loop {
        match wire::read(&mut input) {
            Ok(Some(request)) => {
                if request["type"] != "request" {
                    continue;
                }
                let last = request["command"] == "disconnect";
                let _ = heard.send(Heard::Request(request));
                // The client holds its end open until the adapter exits.
                if last {
                    break Ok(());
                }
            }
            Ok(None) => {
                let _ = heard.send(Heard::Ended);
                break Ok(());
            }
            Err(error) => {
                let _ = heard.send(Heard::Ended);
                break Err(error);
            }
        }
    };
    let _ = adapter.join();
    read
}

impl Adapter {
    /// Take what the adapter hears until it is over, then write what it
    /// said.
    pub(super) fn serve(mut self, hearing: &mpsc::Receiver<Heard>) {
        for message in hearing {
            match message {
                Heard::Request(request) => self.take(request),
                // The client went away: what the adapter started goes too.
                Heard::Ended => self.end(|adapter| adapter.over = true),
                Heard::Session { id, told } => {
                    if let Some(then) = self.then.remove(&id) {
                        then(&mut self, told);
                    }
                }
            }
            if self.over {
                break;
            }
        }
        self.out.flush();
    }

    /// Take one request: answer it, or start what answers it once the
    /// program has.
    fn take(&mut self, request: Value) {
        let command = request["command"].as_str().unwrap_or_default().to_owned();
        let arguments = &request["arguments"];
        let answer = match command.as_str() {
            "initialize" => Ok(self.initialize(arguments)),
            "launch" => return self.launch(request),
            "attach" => return self.attach(request),
            "configurationDone" => return self.configured(request),
            "setExceptionBreakpoints" => Ok(json!({})),
            "setBreakpoints" | "setFunctionBreakpoints" => return self.take_turn(request),
            "threads" => Ok(json!({ "threads": [{ "id": THREAD, "name": "main" }] })),
            "stackTrace" => return self.stack(request),
            "scopes" => Ok(self.scopes(arguments)),
            "variables" => return self.variables(request),
            "evaluate" => return self.evaluate(request),
            "setVariable" => return self.set_variable(request),
            "setExpression" => return self.set_expression(request),
            "continue" => return self.resume(request),
            "pause" => return self.pause(request),
            // A step is answered at once; the pause it ends in is told as
            // `stopped`.
            "next" => return self.step(&request, "over"),
            "stepIn" => return self.step(&request, "into"),
            "stepOut" => return self.step(&request, "out"),
            "terminate" => {
                return self.end(move |adapter| adapter.out.respond(&request, Ok(json!({}))));
            }
            // `disconnect` ends the launched run, which ends with the
            // adapter in any case, or detaches from one started elsewhere.
            "disconnect" => {
                return self.end(move |adapter| {
                    adapter.out.respond(&request, Ok(json!({})));
                    adapter.over = true;
                });
            }
            command => Err(format!("rawr dap doesn't support `{command}`")),
        };
        self.out.respond(&request, answer);
    }
}
