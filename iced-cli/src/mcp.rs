//! `rawr mcp`: a Model Context Protocol server on stdio for coding agents.
//!
//! JSON-RPC 2.0, one message per line. Each tool runs this same `rawr`
//! binary as a child process, so a build's output never reaches the
//! protocol's stdout. Add it to Claude Code with
//! `claude mcp add rawr -- rawr mcp` from the project directory.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const PROTOCOL: &str = "2025-06-18";

/// Serve MCP requests from stdin until it closes.
pub fn serve() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => answer(&message),
            Err(error) => Some(failure(Value::Null, -32700, &error.to_string())),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// The reply to one message; notifications get none.
fn answer(message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match message.get("method").and_then(Value::as_str) {
        Some("initialize") => json!({
            "protocolVersion": params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "rawr", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Rawr previews, checks and screenshots for the Rawr/iced project in `path` \
                (default: the server's working directory). Saving a source file reloads a running \
                `rawr run`/`rawr preview` by itself; `wait_for_reload` returns when it has. `click`, \
                `type_text`, `press_key`, `do` and `reset` drive that window (start it with \
                `rawr run` first); each can return a screenshot.",
        }),
        Some("ping") => json!({}),
        Some("tools/list") => json!({ "tools": tools() }),
        Some("tools/call") => call(&params),
        Some(method) => return Some(failure(id, -32601, &format!("unknown method `{method}`"))),
        None => return Some(failure(id, -32600, "missing method")),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tools() -> Value {
    let path = json!({
        "type": "string",
        "description": "Crate directory or .rs file; defaults to the working directory",
    });
    let shot = json!({
        "type": "boolean",
        "description": "Return a screenshot of the window after the input",
    });
    json!([
        {
            "name": "list_previews",
            "description": "List the project's #[preview]s (each `bind` set is one) as `group/name [subject] size themes`, read from the sources without rendering.",
            "inputSchema": { "type": "object", "properties": { "path": path } },
        },
        {
            "name": "render_preview",
            "description": "Render one preview (only that one) headlessly to a PNG, as `rawr snapshot` does, and return the image.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "name": { "type": "string", "description": "Preview name, group/name, or subject#name" },
                    "theme": { "type": "string", "description": "A declared theme, or `native`; defaults to the first declared one" },
                },
                "required": ["name"],
            },
        },
        {
            "name": "check",
            "description": "Run `rawr check` and return its diagnostics.",
            "inputSchema": { "type": "object", "properties": { "path": path } },
        },
        {
            "name": "screenshot",
            "description": "Capture the window of a running `rawr run` or `rawr preview` and return the PNG.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "title": { "type": "string", "description": "Window title to prefer, when several are open (macOS window capture)" },
                },
            },
        },
        {
            "name": "click",
            "description": "Click in the running window: the widget showing `text` (exact, else contained, any case; a text input's value or placeholder counts), the widget with `id`, or the logical point `x`,`y` (from the top left, footer included).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "text": { "type": "string" },
                    "id": { "type": "string" },
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "screenshot": shot,
                },
            },
        },
        {
            "name": "type_text",
            "description": "Type text into the running window's focused widget (click a text input first, or use `do`).",
            "inputSchema": {
                "type": "object",
                "properties": { "path": path, "text": { "type": "string" }, "screenshot": shot },
                "required": ["text"],
            },
        },
        {
            "name": "press_key",
            "description": "Press and release a key in the running window: Enter, Tab, Escape, Backspace, Delete, Space, Left/Right/Up/Down, Home, End, PageUp, PageDown or one character, optionally after cmd+, ctrl+, shift+, alt+.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": path, "key": { "type": "string" }, "screenshot": shot },
                "required": ["key"],
            },
        },
        {
            "name": "do",
            "description": "Run a sequence of inputs in the running window, each seeing the state the last left: `actions` of {\"action\": \"click\", text|id|x,y}, {\"action\": \"type\", \"text\"}, {\"action\": \"key\", \"key\"}.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "actions": { "type": "array", "items": { "type": "object" } },
                    "screenshot": shot,
                },
                "required": ["actions"],
            },
        },
        {
            "name": "reset",
            "description": "Boot the running window's application or preview afresh (fresh state).",
            "inputSchema": { "type": "object", "properties": { "path": path, "screenshot": shot } },
        },
        {
            "name": "wait_for_reload",
            "description": "Block until the running window's next reload commits (after a source edit) and say so, or return its errors as diagnostics with file and line.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path,
                    "timeout_ms": { "type": "number", "description": "Give up after this long; default 120000" },
                },
            },
        },
    ])
}

fn call(params: &Value) -> Value {
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    let text = |key: &str| arguments.get(key).and_then(Value::as_str);
    let path = text("path").map(PathBuf::from);
    let outcome = match params.get("name").and_then(Value::as_str) {
        Some("list_previews") => previews(path.as_deref()).map(|previews| {
            let lines: Vec<String> = previews
                .iter()
                .map(|preview| {
                    format!(
                        "{}/{} [{}] {}x{} themes: {}{}",
                        preview.group,
                        preview.name,
                        preview.subject,
                        preview.size.0,
                        preview.size.1,
                        if preview.themes.is_empty() {
                            "native".to_owned()
                        } else {
                            preview.themes.join(", ")
                        },
                        if preview.snapshot {
                            ""
                        } else {
                            " (no snapshot)"
                        },
                    )
                })
                .collect();
            vec![content_text(lines.join("\n"))]
        }),
        Some("render_preview") => render_preview(path.as_deref(), text("name"), text("theme")),
        Some("check") => {
            let output = rawr(&["check".as_ref(), target(path.as_deref()).as_os_str()]);
            match output {
                Ok((true, log)) => Ok(vec![content_text(log)]),
                Ok((false, log)) => Err(log),
                Err(error) => Err(error),
            }
        }
        Some("screenshot") => match agent(path.as_deref(), "screenshot", None) {
            Err(Agent::Closed(_)) => screenshot(text("title")),
            outcome => outcome.map_err(Agent::into_message),
        },
        Some(tool @ ("click" | "type_text" | "press_key" | "do" | "reset")) => {
            input(tool, &arguments).and_then(|mut request| {
                if arguments.get("screenshot").and_then(Value::as_bool) == Some(true) {
                    request.push_str("screenshot\n");
                }
                agent(path.as_deref(), &request, None).map_err(Agent::into_message)
            })
        }
        Some("wait_for_reload") => wait_for_reload(
            path.as_deref(),
            arguments
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(120_000),
        ),
        Some(name) => Err(format!("unknown tool `{name}`")),
        None => Err("missing tool name".to_owned()),
    };
    match outcome {
        Ok(content) => json!({ "content": content, "isError": false }),
        Err(message) => json!({ "content": [content_text(message)], "isError": true }),
    }
}

fn content_text(text: String) -> Value {
    json!({ "type": "text", "text": text })
}

fn content_png(bytes: &[u8]) -> Value {
    json!({ "type": "image", "data": base64(bytes), "mimeType": "image/png" })
}

fn target(path: Option<&Path>) -> PathBuf {
    path.map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Run this `rawr` with `arguments`; whether it succeeded, and its output.
fn rawr(arguments: &[&std::ffi::OsStr]) -> Result<(bool, String), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(executable)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((output.status.success(), log))
}

/// The project's previews as written, read from its parsed sources.
fn previews(path: Option<&Path>) -> Result<Vec<rawr::preview::Listed>, String> {
    let target =
        rawr::project::Target::discover(target(path)).map_err(|error| error.to_string())?;
    let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target)
        .map_err(|error| error.to_string())?;
    Ok(graph.previews())
}

/// Render the preview `name` alone with one theme through `rawr snapshot
/// --check` (its selection renders only that preview into the disposable
/// actual tree; baselines are never compared or written) and return the PNG.
fn render_preview(
    path: Option<&Path>,
    name: Option<&str>,
    theme: Option<&str>,
) -> Result<Vec<Value>, String> {
    let name = name.ok_or("`name` is required")?;
    let previews = previews(path)?;
    let listed = previews.iter().find(|preview| {
        name.eq_ignore_ascii_case(&preview.name)
            || name.eq_ignore_ascii_case(&format!("{}/{}", preview.group, preview.name))
            || name.rsplit_once('#').is_some_and(|(subject, variant)| {
                subject == preview.subject && variant.eq_ignore_ascii_case(&preview.name)
            })
    });
    let Some(listed) = listed else {
        let names: Vec<String> = previews
            .iter()
            .map(|preview| format!("{}/{}", preview.group, preview.name))
            .collect();
        return Err(format!(
            "no preview `{name}`; previews: {}",
            names.join(", ")
        ));
    };
    // The first declared theme unless one is named, as the live host shows.
    let theme = theme
        .or_else(|| listed.themes.first().map(String::as_str))
        .unwrap_or("native");
    let target = target(path);
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(executable)
        .args(["snapshot".as_ref(), "--check".as_ref(), target.as_os_str()])
        .env(
            rawr_iced::snapshot::SELECTION,
            format!("{}/{}\t{theme}", listed.group, listed.name),
        )
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(log);
    }
    let root = crate::preview::root(Some(&target)).map_err(|error| error.to_string())?;
    let actual = root.join("target").join("rawr").join("snapshots");
    let manifest = std::fs::read_to_string(actual.join("manifest.rawr-snapshot"))
        .map_err(|error| format!("no preview rendered: {error}\n{log}"))?;
    let (file, width, height) = manifest
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let ["artifact", _, _, _, _, width, height, .., file] = fields[..] else {
                return None;
            };
            Some((
                actual.join(decode(file)),
                width.to_owned(),
                height.to_owned(),
            ))
        })
        .ok_or_else(|| format!("no preview rendered\n{log}"))?;
    let bytes = std::fs::read(&file).map_err(|error| error.to_string())?;
    Ok(vec![
        content_png(&bytes),
        content_text(format!(
            "{}/{} ({theme}) {width}x{height}: {}",
            listed.group,
            listed.name,
            file.display()
        )),
    ])
}

/// An input tool's arguments as agent socket commands (see
/// `rawr_iced::reload::agent`).
fn input(tool: &str, arguments: &Value) -> Result<String, String> {
    let line = |action: &str, step: &Value| -> Result<String, String> {
        let text = |key: &str| step.get(key).and_then(Value::as_str);
        let single = |value: &str| value.replace(['\n', '\r'], " ");
        match action {
            "click" => match (
                text("text"),
                text("id"),
                step.get("x").and_then(Value::as_f64),
                step.get("y").and_then(Value::as_f64),
            ) {
                (Some(text), _, _, _) => Ok(format!("click-text {}\n", single(text))),
                (None, Some(id), _, _) => Ok(format!("click-id {}\n", single(id))),
                (None, None, Some(x), Some(y)) => Ok(format!("click {x} {y}\n")),
                _ => Err("`click` needs `text`, `id`, or `x` and `y`".to_owned()),
            },
            "type" | "type_text" => text("text")
                .map(|text| {
                    text.lines()
                        .map(|line| format!("type {line}\nkey enter\n"))
                        .collect::<String>()
                })
                .map(|typed| {
                    // Each line but the last ends with Enter.
                    typed
                        .strip_suffix("key enter\n")
                        .unwrap_or(&typed)
                        .to_owned()
                })
                .ok_or_else(|| "`type` needs `text`".to_owned()),
            "key" | "press_key" => text("key")
                .map(|key| format!("key {}\n", single(key)))
                .ok_or_else(|| "`key` needs `key`".to_owned()),
            "reset" => Ok("reset\n".to_owned()),
            other => Err(format!("unknown action `{other}`")),
        }
    };
    match tool {
        "do" => arguments
            .get("actions")
            .and_then(Value::as_array)
            .ok_or_else(|| "`do` needs `actions`".to_owned())?
            .iter()
            .map(|step| {
                line(
                    step.get("action")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    step,
                )
            })
            .collect(),
        tool => line(tool, arguments),
    }
}

/// Why the agent socket gave no answer.
enum Agent {
    /// No window listens, or it closed before answering.
    Closed(String),
    /// The window answered with an error.
    Failed(String),
}

impl Agent {
    fn into_message(self) -> String {
        match self {
            Self::Closed(message) | Self::Failed(message) => message,
        }
    }
}

/// Send `request` to the project's running window and turn its answer
/// into tool content: text, a screenshot image, and diagnostics as JSON.
fn agent(
    path: Option<&Path>,
    request: &str,
    timeout: Option<std::time::Duration>,
) -> Result<Vec<Value>, Agent> {
    use std::io::Read;

    let root = crate::preview::root(Some(&target(path)))
        .map_err(|error| Agent::Closed(error.to_string()))?;
    let socket = rawr_iced::reload::agent::socket(&root);
    let mut stream = std::os::unix::net::UnixStream::connect(&socket).map_err(|error| {
        Agent::Closed(format!(
            "no running `rawr run`/`rawr preview` window for {} ({}: {error}); start one first",
            root.display(),
            socket.display()
        ))
    })?;
    stream
        .set_read_timeout(Some(timeout.unwrap_or(std::time::Duration::from_secs(30))))
        .map_err(|error| Agent::Closed(error.to_string()))?;
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|error| Agent::Closed(error.to_string()))?;
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                Agent::Failed("timed out waiting for the window".to_owned())
            }
            _ => Agent::Closed(format!("the window did not answer: {error}")),
        })?;
    let mut lines = answer.lines();
    let ok = match lines.next() {
        Some("ok") => true,
        Some("error") => false,
        _ => {
            return Err(Agent::Closed(
                "the window closed before answering".to_owned(),
            ))
        }
    };
    let mut content = Vec::new();
    let mut text = Vec::new();
    let mut diagnostics = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[..] {
            ["screenshot", file] => match std::fs::read(file) {
                Ok(bytes) => {
                    content.push(content_png(&bytes));
                    text.push(format!("screenshot: {file}"));
                }
                Err(error) => text.push(format!("cannot read {file}: {error}")),
            },
            ["diagnostic", file, line, column, code, message] => diagnostics.push(json!({
                "file": file,
                "line": line.parse::<u64>().unwrap_or(0),
                "column": column.parse::<u64>().unwrap_or(0),
                "code": code,
                "message": message,
            })),
            _ => text.push(line.to_owned()),
        }
    }
    if !diagnostics.is_empty() {
        content.push(content_text(
            json!({ "diagnostics": diagnostics }).to_string(),
        ));
    }
    content.push(content_text(text.join("\n")));
    if ok {
        Ok(content)
    } else {
        Err(Agent::Failed(
            content
                .iter()
                .filter_map(|item| item["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    }
}

/// Wait for the window's next reload. A `.rs` edit replaces a `[rust]`
/// project's runner, closing this window: then the new one answering is
/// the reload.
fn wait_for_reload(path: Option<&Path>, timeout_ms: u64) -> Result<Vec<Value>, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let remaining = || deadline.saturating_duration_since(std::time::Instant::now());
    match agent(path, "wait\n", Some(remaining())) {
        Err(Agent::Closed(message)) if message.contains("before answering") => {
            while !remaining().is_zero() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if agent(path, "\n", Some(remaining())).is_ok() {
                    return Ok(vec![content_text("reloaded into a new runner".to_owned())]);
                }
            }
            Err("the window closed and no new one opened in time".to_owned())
        }
        outcome => outcome.map_err(Agent::into_message),
    }
}

/// The front window of a running `rawr` or its generated runner.
#[cfg(target_os = "macos")]
fn screenshot(title: Option<&str>) -> Result<Vec<Value>, String> {
    use core_foundation::base::CFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{
        create_description_from_array, create_window_list, kCGNullWindowID,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
    };

    let identifiers = create_window_list(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )
    .ok_or("cannot list windows")?;
    let windows = create_description_from_array(identifiers).ok_or("cannot list windows")?;
    let field = |window: &CFDictionary<CFString, CFType>, key: &str| {
        window
            .find(CFString::new(key))
            .map(|value| (*value).clone())
    };
    let mut found = Vec::new();
    for window in windows.iter() {
        let text = |key| {
            field(&window, key)
                .and_then(|value| value.downcast::<CFString>())
                .map(|value| value.to_string())
                .unwrap_or_default()
        };
        let number = |key| {
            field(&window, key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|value| value.to_i64())
        };
        let owner = text("kCGWindowOwnerName");
        if number("kCGWindowLayer") == Some(0)
            && matches!(
                owner.as_str(),
                "rawr" | "rawr-iced" | "rawr-generated-runner"
            )
            && let Some(number) = number("kCGWindowNumber")
        {
            found.push((number, text("kCGWindowName")));
        }
    }
    let (number, name) = title
        .and_then(|title| found.iter().find(|(_, name)| name == title))
        .or_else(|| found.first())
        .cloned()
        .ok_or("no `rawr run` or `rawr preview` window is open")?;
    let directory = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join("target")
        .join("rawr")
        .join("mcp");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("screenshot.png");
    let status = Command::new("screencapture")
        .args(["-x", "-o", &format!("-l{number}")])
        .arg(&path)
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("screencapture failed: {status}"));
    }
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    Ok(vec![
        content_png(&bytes),
        content_text(format!("{name} (window {number}): {}", path.display())),
    ])
}

#[cfg(not(target_os = "macos"))]
fn screenshot(_title: Option<&str>) -> Result<Vec<Value>, String> {
    Err("screenshots need macOS; press Cmd+Shift+S (or Shot) in the window instead".to_owned())
}

/// Undo the manifest's percent-encoding.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                index += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |triple, (index, byte)| {
                triple | u32::from(*byte) << (16 - 8 * index)
            });
        for index in 0..4 {
            if index <= chunk.len() {
                encoded.push(char::from(
                    ALPHABET[(triple >> (18 - 6 * index) & 63) as usize],
                ));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    #[test]
    fn answers_initialize_tool_listing_and_unknown_methods() {
        let initialize = super::answer(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-03-26" },
        }))
        .unwrap();
        assert_eq!(initialize["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(initialize["result"]["serverInfo"]["name"], "rawr");
        assert!(
            super::answer(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .is_none()
        );

        let listed =
            super::answer(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(
            names,
            [
                "list_previews",
                "render_preview",
                "check",
                "screenshot",
                "click",
                "type_text",
                "press_key",
                "do",
                "reset",
                "wait_for_reload"
            ]
        );

        let unknown =
            super::answer(&json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" })).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        let called = super::answer(&json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "nope" },
        }))
        .unwrap();
        assert_eq!(called["result"]["isError"], Value::Bool(true));
    }

    #[test]
    fn input_tools_become_agent_commands() {
        let input = |tool, arguments| super::input(tool, &arguments);
        assert_eq!(
            input("click", json!({ "text": "Paper" })).unwrap(),
            "click-text Paper\n"
        );
        assert_eq!(
            input("click", json!({ "x": 10, "y": 20.5 })).unwrap(),
            "click 10 20.5\n"
        );
        assert!(input("click", json!({})).is_err());
        assert_eq!(
            input("type_text", json!({ "text": "a\nb" })).unwrap(),
            "type a\nkey enter\ntype b\n"
        );
        assert_eq!(
            input(
                "do",
                json!({ "actions": [
                    { "action": "click", "id": "name" },
                    { "action": "key", "key": "cmd+a" },
                ] })
            )
            .unwrap(),
            "click-id name\nkey cmd+a\n"
        );
        assert_eq!(input("reset", json!({})).unwrap(), "reset\n");
    }

    #[test]
    fn manifest_fields_decode_and_png_bytes_encode() {
        assert_eq!(
            super::decode("rawrgallery%3A%3A%4Bit%3A%3Aview%23dial"),
            "rawrgallery::Kit::view#dial"
        );
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
    }
}
