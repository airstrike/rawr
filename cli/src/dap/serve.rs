//! The adapter's loop: read each request, answer it on a thread of its
//! own, so a request that waits for the program (a breakpoint set while it
//! builds, a step) never holds the others; end at `disconnect` or the end
//! of input.

use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{wire, Adapter, THREAD};

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
    let adapter = Arc::new(Adapter::new(root, wire::Writer::new(output)));
    while let Some(request) = wire::read(&mut input)? {
        if request["type"] != "request" {
            continue;
        }
        if request["command"] == "disconnect" {
            adapter.disconnect(&request);
            return Ok(());
        }
        let answering = Arc::clone(&adapter);
        std::thread::Builder::new()
            .name("rawr-dap-request".into())
            .spawn(move || answering.take(&request))?;
    }
    // The client went away: what the adapter started goes too.
    adapter.end();
    Ok(())
}

impl Adapter {
    /// Answer one request.
    fn take(self: &Arc<Self>, request: &Value) {
        let arguments = &request["arguments"];
        let answer = match request["command"].as_str().unwrap_or_default() {
            "initialize" => Ok(self.initialize(arguments)),
            "launch" => self.launch(arguments),
            "attach" => self.attach(arguments),
            "configurationDone" | "setExceptionBreakpoints" => Ok(json!({})),
            "setBreakpoints" => self.set_lines(arguments),
            "setFunctionBreakpoints" => self.set_functions(arguments),
            "threads" => Ok(json!({ "threads": [{ "id": THREAD, "name": "main" }] })),
            "stackTrace" => self.stack(arguments),
            "scopes" => Ok(self.scopes(arguments)),
            "variables" => self.variables(arguments),
            "evaluate" => self.evaluate(arguments),
            "continue" => self.resume(),
            "pause" => self.pause(),
            // A step is answered at once; the pause it ends in is told as
            // `stopped`.
            "next" => return self.step(request, "over"),
            "stepIn" => return self.step(request, "into"),
            "stepOut" => return self.step(request, "out"),
            "terminate" => {
                self.end();
                Ok(json!({}))
            }
            command => Err(format!("rawr dap doesn't support `{command}`")),
        };
        self.out.respond(request, answer);
    }
}
