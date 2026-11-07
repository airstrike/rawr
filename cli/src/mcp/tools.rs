//! Core's tools (D537.6) and the listing that adds the extensions'.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::process::{self, Cancel};
use super::{content, extension, session, window, Progress, Server, BOUND};
use crate::layer;

/// Core's tool names: an extension's tool by one of these names is not
/// listed.
pub const CORE: [&str; 19] = [
    "new",
    "check",
    "run",
    "start",
    "wait",
    "status",
    "stop",
    "screenshot",
    "move",
    "click",
    "drag",
    "scroll",
    "key",
    "type",
    "inspect",
    "break",
    "step",
    "continue",
    "clear",
];

/// What every input tool's description ends with.
const INPUT: &str = "Positions are logical points from the window's top-left corner, title bar included, as `screenshot` shows it. The input goes to the program alone: your own pointer, keyboard and focus stay as they are, and the window isn't brought forward. The window shows an agent cursor where the pointer last went, also in screenshots. Returns once the program has taken the input and drawn again, so a `screenshot` next shows what it did.";

/// Crates whose programs draw a terminal UI, which `start` can't host yet.
const TERMINAL_UI: [&str; 6] = [
    "ratatui",
    "crossterm",
    "termion",
    "termwiz",
    "cursive",
    "tui",
];

/// The `path` property every tool takes.
#[must_use]
pub fn path_schema() -> Value {
    json!({
        "type": "string",
        "description": "The crate's directory or a .rs file: absolute, or relative to the server's project root; defaults to that root",
    })
}

fn milliseconds(description: &str, default: u64) -> Value {
    json!({
        "type": "integer",
        "minimum": 0,
        "maximum": BOUND.as_millis(),
        "default": default,
        "description": description,
    })
}

/// An input tool's schema: its own `properties`, which `required` names,
/// and the `path`, `title` and `timeout_ms` every input tool takes.
fn gesture(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "path": path_schema(),
            "title": { "type": "string", "description": "The window's title, when the program has several; defaults to its front window" },
            "timeout_ms": milliseconds("How long to wait for the program to take the input and draw", 5_000),
        },
        "required": required,
    });
    if let (Some(all), Some(own)) = (schema["properties"].as_object_mut(), properties.as_object()) {
        all.extend(own.clone());
    }
    schema
}

fn coordinate(description: &str) -> Value {
    json!({ "type": "number", "description": format!("{description}, in logical points from the window's top-left corner") })
}

fn button() -> Value {
    json!({ "type": "string", "enum": ["left", "right", "middle"], "default": "left" })
}

/// Every tool: core's, then the installed extensions'.
#[must_use]
pub fn list() -> Vec<Value> {
    let installed = layer::installed();
    let templates: Vec<String> = installed
        .iter()
        .flat_map(|binary| binary.manifest.templates.iter().cloned())
        .collect();
    let mut template = json!({
        "type": "string",
        "description": "A template an installed extension offers; omit it for a plain program with no dependencies",
    });
    if !templates.is_empty() {
        template["enum"] = json!(templates);
    }
    let missing: Vec<String> = layer::catalog()
        .into_iter()
        .filter(|known| {
            !known.manifest.templates.is_empty()
                && !installed
                    .iter()
                    .any(|binary| binary.manifest.name == known.manifest.name)
        })
        .map(|known| {
            format!(
                "{} comes with {} (not installed)",
                known.manifest.templates.join(", "),
                known.manifest.name
            )
        })
        .collect();
    let new_description = if missing.is_empty() {
        "Create a Cargo crate whose src/main.rs starts with `// rawr`, ready for `check`, `run` and `start`. Refuses a path inside a Cargo workspace that wouldn't accept it, and says how to fix that.".to_owned()
    } else {
        format!(
            "Create a Cargo crate whose src/main.rs starts with `// rawr`, ready for `check`, `run` and `start`. Refuses a path inside a Cargo workspace that wouldn't accept it, and says how to fix that. Template {}.",
            missing.join("; ")
        )
    };
    let arguments = json!({
        "type": "array",
        "items": { "type": "string" },
        "description": "The program's arguments, as after `--` in `cargo run`",
    });
    let mut tools = vec![
        json!({
            "name": "new",
            "description": new_description,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Where to create the crate: a directory that doesn't exist or is empty" },
                    "template": template,
                },
                "required": ["path"],
            },
        }),
        json!({
            "name": "check",
            "description": "Check a program without running it: rawr's check and `cargo check`. Returns each error with its absolute file, line and column. A first check of a program with Rust dependencies builds them; when it says it is still checking, call it again.",
            "inputSchema": { "type": "object", "properties": { "path": path_schema() } },
        }),
        json!({
            "name": "run",
            "description": "Run a program to its end, with arguments and stdin, and return its exit status, stdout and stderr. For command-line programs. For a window or a long-running program, use `start`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "args": arguments,
                    "stdin": { "type": "string", "description": "The program's input; it then reads end of input" },
                    "timeout_ms": milliseconds("Stop the program after this long", 30_000),
                },
            },
        }),
        json!({
            "name": "start",
            "description": "Start a program that keeps running, such as a window, and keep it running: a save reloads it, and one that ends runs again on the next save. Returns `running`, `failed` with diagnostics, or `building` while a first start builds; then call `wait`. Starting a program again replaces it. Programs that draw a terminal UI are refused for now.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "args": arguments,
                    "stdin": { "type": "string", "description": "The program's input; it then reads end of input" },
                    "wait_ms": milliseconds("How long to wait for it to run before answering", 30_000),
                },
            },
        }),
        json!({
            "name": "wait",
            "description": "Wait for what happens next to the started program: `running`, `paused` (at a breakpoint or after a step, with its file and line), `resumed`, `queued` (a save reached a program whose native loop takes it at its next call into script code), `swapped` (a save reloaded it in place, keeping its state), `restarting`, `failed` (with diagnostics; the last good program keeps running) or `ended`. Returns those events and the output since the last call. Call it after each save.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "cursor": { "type": "integer", "description": "The last event seen; defaults to the last one returned" },
                    "output_cursor": { "type": "integer", "description": "The last output seen; defaults to the last one returned" },
                    "timeout_ms": milliseconds("How long to wait for an event", 30_000),
                },
            },
        }),
        json!({
            "name": "status",
            "description": "The programs this server started: each one's state (`running`, `paused` with where, and so on), without waiting.",
            "inputSchema": { "type": "object", "properties": { "path": path_schema() } },
        }),
        json!({
            "name": "stop",
            "description": "Stop the program `start` started.",
            "inputSchema": { "type": "object", "properties": { "path": path_schema() } },
        }),
        json!({
            "name": "screenshot",
            "description": "Capture the started program's window alone and return the PNG.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "title": { "type": "string", "description": "The window's title, when the program has several" },
                },
            },
        }),
        json!({
            "name": "move",
            "description": format!("Move the pointer in the started program's window to `x`, `y`, as for a hover. {INPUT}"),
            "inputSchema": gesture(json!({ "x": coordinate("x"), "y": coordinate("y") }), &["x", "y"]),
        }),
        json!({
            "name": "click",
            "description": format!("Click in the started program's window at `x`, `y`: the pointer moves there, then `button` goes down and up `count` times, 2 for a double click. {INPUT}"),
            "inputSchema": gesture(
                json!({
                    "x": coordinate("x"),
                    "y": coordinate("y"),
                    "button": button(),
                    "count": { "type": "integer", "minimum": 1, "maximum": 3, "default": 1, "description": "How many clicks: 2 for a double click" },
                }),
                &["x", "y"],
            ),
        }),
        json!({
            "name": "drag",
            "description": format!("Drag in the started program's window: `button` goes down at `x`, `y`, the pointer moves to `to_x`, `to_y` in short steps with it held, and it goes up there. {INPUT}"),
            "inputSchema": gesture(
                json!({
                    "x": coordinate("x where the drag starts"),
                    "y": coordinate("y where the drag starts"),
                    "to_x": coordinate("x where it ends"),
                    "to_y": coordinate("y where it ends"),
                    "button": button(),
                }),
                &["x", "y", "to_x", "to_y"],
            ),
        }),
        json!({
            "name": "scroll",
            "description": format!("Scroll what is under `x`, `y` in the started program's window: positive `dy` toward the end, as a wheel turned toward you; positive `dx` to the right. {INPUT}"),
            "inputSchema": gesture(
                json!({
                    "x": coordinate("x"),
                    "y": coordinate("y"),
                    "dx": { "type": "number", "default": 0, "description": "How far to scroll right; negative scrolls left" },
                    "dy": { "type": "number", "default": 0, "description": "How far to scroll down, toward the end; negative scrolls up" },
                    "unit": { "type": "string", "enum": ["line", "pixel"], "default": "line", "description": "`line` as a mouse wheel's notches, `pixel` as a trackpad's logical points" },
                }),
                &["x", "y"],
            ),
        }),
        json!({
            "name": "key",
            "description": format!("Press a key in the started program's window, with modifiers held: the modifiers go down, the key goes down and up, and the modifiers go up. The program's focused widget takes it. {INPUT}"),
            "inputSchema": gesture(
                json!({
                    "key": { "type": "string", "description": "One character, as `a` or `?`, or a key's name: Enter, Tab, Space, Backspace, Delete, Escape, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown, F1 to F12" },
                    "modifiers": {
                        "type": "array",
                        "items": { "type": "string", "enum": ["shift", "control", "alt", "command"] },
                        "description": "The modifier keys held: `command` is macOS's ⌘",
                    },
                }),
                &["key"],
            ),
        }),
        json!({
            "name": "type",
            "description": format!("Type text in the started program's window, one key press for each character; a newline is Enter. The program's focused widget takes it: `click` a text input first. {INPUT}"),
            "inputSchema": gesture(
                json!({ "text": { "type": "string", "description": "The text to type" } }),
                &["text"],
            ),
        }),
        json!({
            "name": "inspect",
            "description": "Read the started program's values while it runs: the bindings in scope where it is, or one binding by `name`, each as Rust's `{:?}` prints it. The program answers at its next step in its own code; with `name` or `file`, at the next step where that binding is in scope or in that file. The answer lists the calls the program is in; `frame` reads a caller's bindings: 0 is the function it is in, 1 its caller, and on. A program waiting for input or an event answers once it runs again.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "name": { "type": "string", "description": "A binding's name, such as a local or a parameter like `state`" },
                    "file": { "type": "string", "description": "A source file of the program: absolute, or relative to the server's project root" },
                    "frame": { "type": "integer", "minimum": 0, "default": 0, "description": "Which call's bindings: 0 for the function the program is in, 1 for its caller, and on" },
                    "timeout_ms": milliseconds("How long to wait for the program to reach such a step", 2_000),
                },
            },
        }),
        json!({
            "name": "break",
            "description": "Set a breakpoint in the started program: on a `line` of a `file`, or on a `function`'s first statement. The program pauses when it reaches it. `inspect` then reads its values there, and `step` or `continue` resumes it. Returns where the program paused, if it pauses within `timeout_ms`. A breakpoint stays across saves while its line's text is still in the file, and moves with that text. Code in Rust dependencies has no breakpoints. A pause inside a call from native code, such as a window's event loop, holds that loop: the window doesn't redraw or take input until the program goes on.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "file": { "type": "string", "description": "A source file of the program: absolute, or relative to the server's project root" },
                    "line": { "type": "integer", "minimum": 1, "description": "A line of `file`, from 1" },
                    "function": { "type": "string", "description": "A function of the program, as `tick` or `counter::tick`, in place of `file` and `line`" },
                    "timeout_ms": milliseconds("How long to wait for the program to pause", 2_000),
                },
            },
        }),
        json!({
            "name": "step",
            "description": "Run the paused program to its next statement and pause there: `over` stays in this function, `into` goes into a function it calls, and `out` returns from this one. A step over native code, such as a crate's function, runs it whole. Returns where it paused, if it pauses within `timeout_ms`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "kind": { "type": "string", "enum": ["over", "into", "out"], "default": "over" },
                    "timeout_ms": milliseconds("How long to wait for the program to pause", 2_000),
                },
            },
        }),
        json!({
            "name": "continue",
            "description": "Let the paused program run on. Returns where it pauses next, if it reaches a breakpoint within `timeout_ms`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "timeout_ms": milliseconds("How long to wait for the program to pause", 2_000),
                },
            },
        }),
        json!({
            "name": "clear",
            "description": "Remove the started program's breakpoints: all of them, or those on `file`, on `line` of `file` or on `function`. A paused program stays paused until `continue`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "file": { "type": "string", "description": "A source file of the program: absolute, or relative to the server's project root" },
                    "line": { "type": "integer", "minimum": 1, "description": "A line of `file`, from 1" },
                    "function": { "type": "string", "description": "A function a breakpoint was set on" },
                },
            },
        }),
    ];
    tools.extend(
        extension::offered(&installed, &CORE)
            .iter()
            .map(extension::listing),
    );
    tools
}

/// The `initialize` instructions: rawr, the installed extensions, the loop
/// and what to install for a missing extension.
#[must_use]
pub fn instructions(root: &Path) -> String {
    let installed = layer::installed();
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
        .map_or_else(|| "rawr".to_owned(), |path| path.display().to_string());
    let mut text = format!(
        "rawr {} ({executable}) runs Rust programs for dev builds: a Cargo crate whose files start with `// rawr`, or one .rs file. It reloads them when you save.\n\
         Loop: `new` makes a crate. Edit its files with your own tools. `check` it. `run` a command-line program to its end with args and stdin, or `start` a window or long-running program and `wait` after each save to see it reload or fail. `screenshot` shows its window, and `move`, `click`, `drag`, `scroll`, `key` and `type` send it input, at positions in the screenshot's points. `inspect` reads its values while it runs. `break` pauses it at a line or a function, `step` and `continue` go on from there, and `clear` removes breakpoints. `stop` ends it.\n\
         Relative paths start at {}.\n",
        rawr::VERSION,
        root.display()
    );
    for binary in &installed {
        let tools: Vec<String> = extension::offered(std::slice::from_ref(binary), &CORE)
            .into_iter()
            .map(|offered| offered.tool.name)
            .collect();
        text.push_str(&format!(
            "{} ({}) serves programs that use {}",
            binary.manifest.name,
            binary.path.display(),
            crate::base::uses(&binary.manifest.claims)
        ));
        if tools.is_empty() {
            text.push_str(".\n");
        } else {
            text.push_str(&format!(" and adds {}.\n", tools.join(", ")));
        }
    }
    for known in layer::catalog() {
        if installed
            .iter()
            .any(|binary| binary.manifest.name == known.manifest.name)
        {
            continue;
        }
        text.push_str(&format!(
            "{} is not installed. Programs that use {} need it. Install it with:\n{}\n",
            known.manifest.name,
            crate::base::uses(&known.manifest.claims),
            layer::install_lines(&known)
        ));
    }
    text
}

/// Where a call's program is: the path given, its project root and the
/// root's Cargo target directory.
#[derive(Clone, Debug)]
pub struct Place {
    /// The path, absolute.
    pub path: PathBuf,
    /// The crate's root, or the single file.
    pub root: PathBuf,
    /// The Cargo target directory for the root.
    pub target_dir: PathBuf,
}

impl Place {
    /// Where the server keeps its own files for this root.
    fn directory(&self) -> PathBuf {
        self.target_dir.join("rawr").join("mcp")
    }
}

fn absolute(server: &Server, path: Option<&str>) -> PathBuf {
    let path = path.map_or_else(|| server.root.clone(), PathBuf::from);
    let path = if path.is_absolute() {
        path
    } else {
        server.root.join(path)
    };
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// Resolve `path` once: its program's root and the root's target directory.
fn place(server: &Server, path: Option<&str>) -> Result<Place, String> {
    let path = absolute(server, path);
    let target = crate::base::discover(Some(&path)).map_err(|report| {
        crate::diagnostic::lines(&crate::diagnostic::shape(&report, &server.root))
    })?;
    let root = target.project().map_or_else(
        || target.source().path().to_path_buf(),
        |project| project.root().as_path().to_path_buf(),
    );
    let directory = if root.is_dir() {
        root.clone()
    } else {
        root.parent()
            .map_or_else(|| root.clone(), Path::to_path_buf)
    };
    Ok(Place {
        path,
        root,
        target_dir: target_directory(&directory),
    })
}

/// The Cargo target directory of the project at `directory`.
#[must_use]
pub fn target_directory(directory: &Path) -> PathBuf {
    let environment: HashMap<String, String> = std::env::vars().collect();
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    rawr_location::target_directory(
        directory,
        |key| environment.get(key).map(String::as_str),
        cargo_home.as_deref(),
    )
}

fn bounded(arguments: &Value, key: &str, default: u64) -> Duration {
    Duration::from_millis(
        arguments
            .get(key)
            .and_then(Value::as_u64)
            .unwrap_or(default),
    )
    .min(BOUND)
}

fn strings(arguments: &Value, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map_or_else(|| item.to_string(), str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Run one `tools/call`.
pub fn call(server: &Server, params: &Value, cancel: &Cancel) -> Value {
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    let progress = Progress {
        server,
        token: params
            .get("_meta")
            .and_then(|meta| meta.get("progressToken"))
            .cloned(),
    };
    let text = |key: &str| arguments.get(key).and_then(Value::as_str);
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let outcome = match name {
        "new" => new(server, text("path"), text("template"), cancel),
        "check" => place(server, text("path")).map(|place| {
            server
                .checks
                .check(&place.path, &place.root, &place.directory(), BOUND)
        }),
        "run" => place(server, text("path")).map(|place| {
            run(
                &place,
                &strings(&arguments, "args"),
                text("stdin"),
                bounded(&arguments, "timeout_ms", 30_000),
                cancel,
            )
        }),
        "start" => place(server, text("path")).and_then(|place| {
            start(
                server,
                &place,
                &strings(&arguments, "args"),
                text("stdin"),
                bounded(&arguments, "wait_ms", 30_000),
                &progress,
            )
        }),
        "wait" => session(server, text("path")).map(|session| {
            session.next(
                arguments.get("cursor").and_then(Value::as_u64),
                bounded(&arguments, "timeout_ms", 30_000),
                |stage, percent| progress.send(percent as f64, Some(100.0), stage),
            );
            let answer = session.answer(
                arguments.get("cursor").and_then(Value::as_u64),
                arguments.get("output_cursor").and_then(Value::as_u64),
            );
            content::success(answer.structured, answer.summary, Vec::new())
        }),
        "status" => Ok(status(server, text("path"))),
        "stop" => stop(server, text("path")),
        "inspect" => inspect(
            server,
            text("path"),
            text("name"),
            text("file"),
            arguments.get("frame").and_then(Value::as_u64).unwrap_or(0),
            bounded(&arguments, "timeout_ms", 2_000),
        ),
        "break" | "step" | "continue" | "clear" => debug(server, name, &arguments),
        "screenshot" => screenshot(server, text("path"), text("title"), cancel, &progress),
        "move" | "click" | "drag" | "scroll" | "key" | "type" => input(server, name, &arguments),
        name => extension_call(server, name, &arguments, cancel, &progress),
    };
    outcome.unwrap_or_else(content::failure)
}

/// `new`: `rawr new` in a child, so its refusals and templates are the
/// command's own.
fn new(
    server: &Server,
    path: Option<&str>,
    template: Option<&str>,
    cancel: &Cancel,
) -> Result<Value, String> {
    let path = path.ok_or("`new` needs a `path`")?;
    let path = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        server.root.join(path)
    };
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command.arg("new").arg(&path);
    if let Some(template) = template {
        command.args(["--template", template]);
    }
    command.current_dir(&server.root);
    let finished = process::run(&mut command, None, Instant::now() + BOUND, cancel)
        .map_err(|error| error.to_string())?;
    let (stdout, _) = finished.stdout.text();
    let (stderr, _) = finished.stderr.text();
    if !finished.status.is_some_and(|status| status.success()) {
        return Err(format!("{}{}", stdout, stderr).trim_end().to_owned());
    }
    let root = std::fs::canonicalize(&path).unwrap_or(path);
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != "target" {
                    stack.push(path);
                }
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    let next = "Edit src/main.rs, then call `check`, then `run` (a command-line program) or `start` (a window or a long-running program).";
    Ok(content::success(
        json!({ "root": root, "files": files, "template": template, "next": next }),
        format!(
            "Created {}:\n{}\n{next}",
            root.display(),
            files
                .iter()
                .map(|file| format!("  {}", file.display()))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        Vec::new(),
    ))
}

/// `run`: `rawr run <path> -- args` to its end.
fn run(
    place: &Place,
    arguments: &[String],
    stdin: Option<&str>,
    timeout: Duration,
    cancel: &Cancel,
) -> Value {
    let Ok(executable) = std::env::current_exe() else {
        return content::failure("cannot find rawr");
    };
    let mut command = Command::new(executable);
    command
        .arg("run")
        .arg(&place.path)
        .args(if arguments.is_empty() {
            None
        } else {
            Some("--")
        })
        .args(arguments)
        .current_dir(if place.root.is_dir() {
            &place.root
        } else {
            place.root.parent().unwrap_or(&place.root)
        });
    let finished = match process::run(
        &mut command,
        Some(stdin.unwrap_or_default()),
        Instant::now() + timeout,
        cancel,
    ) {
        Ok(finished) => finished,
        Err(error) => return content::failure(format!("cannot run rawr: {error}")),
    };
    let (stdout, stdout_cut) = finished.stdout.text();
    let (stderr, stderr_cut) = finished.stderr.text();
    let status = finished.status.map(process::code);
    let structured = json!({
        "status": status,
        "stdout": stdout,
        "stderr": stderr,
        "timed_out": finished.timed_out,
        "output_dropped": stdout_cut || stderr_cut,
        "seconds": finished.elapsed.as_secs_f64(),
    });
    let mut summary = match (status, finished.timed_out) {
        (_, true) => format!(
            "stopped after {} s, still running. A program with Rust dependencies builds them on its first run: use `start` and `wait` for it.\n",
            timeout.as_secs()
        ),
        (Some(status), _) => format!("exit status {status}\n"),
        (None, _) => "cancelled\n".to_owned(),
    };
    for (stream, output) in [("stdout", &stdout), ("stderr", &stderr)] {
        if !output.is_empty() {
            summary.push_str(&format!("--- {stream} ---\n{output}"));
            if !output.ends_with('\n') {
                summary.push('\n');
            }
        }
    }
    if status == Some(0) {
        content::success(structured, summary.trim_end(), Vec::new())
    } else {
        content::failure_with(structured, summary.trim_end(), Vec::new())
    }
}

/// The terminal-UI crate a crate's manifest depends on, if any.
fn terminal_ui(root: &Path) -> Option<&'static str> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let document = manifest.parse::<toml_edit::DocumentMut>().ok()?;
    let dependencies = document.get("dependencies")?.as_table_like()?;
    TERMINAL_UI
        .into_iter()
        .find(|name| dependencies.contains_key(name))
}

fn start(
    server: &Server,
    place: &Place,
    arguments: &[String],
    stdin: Option<&str>,
    wait: Duration,
    progress: &Progress<'_>,
) -> Result<Value, String> {
    if let Some(name) = terminal_ui(&place.root) {
        return Err(format!(
            "this program uses `{name}`, which draws a terminal UI, and `start` can't host a terminal yet. Run it with `rawr run` in a terminal of your own."
        ));
    }
    let session = server
        .sessions
        .start(
            &place.root,
            &place.path,
            arguments,
            stdin,
            &place.directory(),
        )
        .map_err(|error| format!("cannot start rawr run: {error}"))?;
    session.settle(wait, |stage, percent| {
        progress.send(percent as f64, Some(100.0), stage);
    });
    let answer = session.answer(None, None);
    let mut summary = answer.summary;
    if session.phase().is_pending() {
        summary.push_str("Still building: call `wait` until it runs.\n");
    }
    Ok(content::success(answer.structured, summary, Vec::new()))
}

/// The session for `path`, or why there is none.
fn session(
    server: &Server,
    path: Option<&str>,
) -> Result<std::sync::Arc<session::Session>, String> {
    let place = place(server, path)?;
    server.sessions.get(&place.root).ok_or_else(|| {
        format!(
            "nothing was started for {}: call `start` first",
            place.root.display()
        )
    })
}

fn status(server: &Server, path: Option<&str>) -> Value {
    let sessions = match path {
        Some(_) => session(server, path)
            .map(|session| vec![session])
            .unwrap_or_default(),
        None => server.sessions.all(),
    };
    let list: Vec<Value> = sessions
        .iter()
        .map(|session| {
            json!({
                "root": session.root,
                "path": session.path,
                "args": session.arguments,
                "pid": session.pid,
                "state": session.phase().name(),
                "paused": session.paused(),
            })
        })
        .collect();
    let summary = if list.is_empty() {
        "no programs started".to_owned()
    } else {
        list.iter()
            .map(|entry| {
                let mut line = format!(
                    "{}: {}",
                    entry["root"].as_str().unwrap_or_default(),
                    entry["state"].as_str().unwrap_or_default(),
                );
                if let Some(paused) = entry["paused"].as_object() {
                    line.push_str(&format!(" {}", session::pause_text(paused)));
                }
                line.push_str(&format!(" (pid {})", entry["pid"]));
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    content::success(json!({ "sessions": list }), summary, Vec::new())
}

fn stop(server: &Server, path: Option<&str>) -> Result<Value, String> {
    let place = place(server, path)?;
    let Some(session) = server.sessions.stop(&place.root) else {
        return Ok(content::success(
            json!({ "stopped": false }),
            format!("nothing was running for {}", place.root.display()),
            Vec::new(),
        ));
    };
    // The group has its grace to end.
    let deadline = Instant::now() + process::GRACE + Duration::from_secs(1);
    while is_alive(session.pid) {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(content::success(
        json!({ "stopped": true, "root": session.root, "pid": session.pid }),
        format!("stopped {}", session.root.display()),
        Vec::new(),
    ))
}

/// `inspect`: ask the started program for its values over its socket.
fn inspect(
    server: &Server,
    path: Option<&str>,
    name: Option<&str>,
    file: Option<&str>,
    frame: u64,
    timeout: Duration,
) -> Result<Value, String> {
    let session = session(server, path)?;
    let file = file.map(|file| absolute(server, Some(file)));
    let request = json!({
        "name": name,
        "file": file,
        "frame": frame,
        "timeout_ms": timeout.as_millis() as u64,
    });
    let answer = ask(&session.socket, &request, timeout).map_err(|error| {
        let phase = session.phase();
        if phase.is_live() {
            format!("cannot reach the running program: {error}")
        } else {
            format!(
                "the program started for {} isn't running (state: {})",
                session.root.display(),
                phase.name()
            )
        }
    })?;
    if answer["reached"] != true {
        let paused = &answer["paused"];
        if paused.is_object() {
            let mut missing = Vec::new();
            let depth = answer["depth"].as_u64().unwrap_or(1);
            if frame >= depth {
                missing.push(format!(
                    "the program is {depth} frame{} deep, so there is no frame {frame}",
                    if depth == 1 { "" } else { "s" }
                ));
            }
            if let (true, Some(name)) = (missing.is_empty(), name) {
                missing.push(format!("`{name}` isn't in scope"));
            }
            if let (true, Some(file)) = (frame < depth, &file) {
                missing.push(format!("it isn't in {}", file.display()));
            }
            return Err(format!(
                "the program is paused at {}:{}, where {}. Call `step` or `continue` to go on.",
                paused["file"].as_str().unwrap_or_default(),
                paused["line"],
                missing.join(" or ")
            ));
        }
        let mut place = String::new();
        if frame > 0 {
            place.push_str(&format!(
                " {frame} call{} deep",
                if frame == 1 { "" } else { "s" }
            ));
        }
        if let Some(name) = name {
            place.push_str(&format!(" where `{name}` is in scope"));
        }
        if let Some(file) = &file {
            place.push_str(&format!(" in {}", file.display()));
        }
        return Err(format!(
            "the program ran none of its own code{place} within {} ms, so no values were read. It may be waiting, for example for input or an event; call `inspect` again once it runs.",
            timeout.as_millis()
        ));
    }
    let mut summary = match (answer["file"].as_str(), answer["line"].as_u64()) {
        (Some(file), Some(line)) => format!("{file}:{line}"),
        _ => "at a step outside the program's files".to_owned(),
    };
    if let Some(function) = answer["function"].as_str() {
        summary.push_str(&format!(" in `{function}`"));
    }
    summary.push('\n');
    let bindings = answer["bindings"].as_array().cloned().unwrap_or_default();
    if bindings.is_empty() {
        summary.push_str("no bindings in scope\n");
    }
    for binding in &bindings {
        let name = binding["name"].as_str().unwrap_or_default();
        match binding["value"].as_str() {
            Some(value) => summary.push_str(&format!("{name} = {value}\n")),
            None => summary.push_str(&format!(
                "{name}: {}\n",
                binding["error"].as_str().unwrap_or_default()
            )),
        }
    }
    // The calls the program is in, when there is more than one: `frame`
    // reads a caller's bindings.
    let frames = answer["frames"].as_array().cloned().unwrap_or_default();
    if frames.len() > 1 {
        summary.push_str("frames (read one with `frame`):\n");
        for (index, at) in frames.iter().enumerate() {
            summary.push_str(&format!(
                "  #{index} {} at {}:{}\n",
                at["function"].as_str().unwrap_or("?"),
                at["file"].as_str().unwrap_or("?"),
                at["line"]
            ));
        }
    }
    let structured = json!({
        "root": session.root,
        "frame": answer["frame"],
        "file": answer["file"],
        "line": answer["line"],
        "function": answer["function"],
        "bindings": bindings,
        "frames": frames,
    });
    Ok(content::success(structured, summary.trim_end(), Vec::new()))
}

/// `break`, `step`, `continue` and `clear`: ask the started program over
/// its socket, and say where it paused.
fn debug(server: &Server, tool: &str, arguments: &Value) -> Result<Value, String> {
    let session = session(server, arguments.get("path").and_then(Value::as_str))?;
    let file = arguments
        .get("file")
        .and_then(Value::as_str)
        .map(|file| absolute(server, Some(file)));
    let timeout = bounded(arguments, "timeout_ms", 2_000);
    let request = json!({
        "op": tool,
        "file": file,
        "line": arguments.get("line"),
        "function": arguments.get("function"),
        "kind": arguments.get("kind"),
        "timeout_ms": timeout.as_millis() as u64,
    });
    let answer = ask(&session.socket, &request, timeout).map_err(|error| {
        let phase = session.phase();
        if phase.is_live() {
            format!("cannot reach the running program: {error}")
        } else {
            format!(
                "the program started for {} isn't running (state: {})",
                session.root.display(),
                phase.name()
            )
        }
    })?;
    match answer["error"].as_str() {
        Some("not paused") => {
            return Err(format!(
                "the program isn't paused, so there is nothing to `{tool}` from. Set a breakpoint with `break`; the program pauses when it reaches it."
            ))
        }
        Some(error) => return Err(error.to_owned()),
        None => {}
    }
    let mut summary = match tool {
        "break" => {
            let mut text = match answer["set"]["function"].as_str() {
                Some(function) => {
                    let lines: Vec<String> = answer["lines"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|line| {
                            Some(format!("{}:{}", line["file"].as_str()?, line["line"]))
                        })
                        .collect();
                    if lines.is_empty() {
                        format!("breakpoint on `{function}`")
                    } else {
                        format!("breakpoint on `{function}` at {}", lines.join(", "))
                    }
                }
                None => format!(
                    "breakpoint at {}:{}",
                    answer["set"]["file"].as_str().unwrap_or_default(),
                    answer["set"]["line"]
                ),
            };
            if answer["compiled"] == false {
                text.push_str(" (it applies once the program has compiled)");
            }
            text.push('\n');
            text
        }
        "clear" => {
            let left = answer["left"].as_array().map_or(0, Vec::len);
            format!(
                "removed {} breakpoint{}, {left} left\n",
                answer["cleared"],
                if answer["cleared"] == 1 { "" } else { "s" }
            )
        }
        _ => String::new(),
    };
    let paused = &answer["paused"];
    // `status` and `wait` say so at once, before the program's own event
    // arrives (#535).
    session.observe_pause(paused.as_object().map(|_| paused.clone()));
    if paused.is_object() {
        summary.push_str(&format!(
            "paused at {}:{}",
            paused["file"].as_str().unwrap_or_default(),
            paused["line"]
        ));
        if let Some(function) = paused["function"].as_str() {
            summary.push_str(&format!(" in `{function}`"));
        }
        summary.push_str(&format!(
            " ({})\nRead values with `inspect`, and go on with `step` or `continue`.",
            paused["reason"].as_str().unwrap_or_default()
        ));
    } else if tool == "clear" {
        summary.push_str("the program runs");
    } else {
        summary.push_str(&format!(
            "the program runs and didn't pause within {} ms",
            timeout.as_millis()
        ));
    }
    let mut structured = answer;
    structured["root"] = json!(session.root);
    Ok(content::success(structured, summary.trim_end(), Vec::new()))
}

/// `move`, `click`, `drag`, `scroll`, `key` and `type`: ask the started
/// program over its socket to hand its window the input, and say what it
/// reached.
fn input(server: &Server, tool: &str, arguments: &Value) -> Result<Value, String> {
    let session = session(server, arguments.get("path").and_then(Value::as_str))?;
    let timeout = bounded(arguments, "timeout_ms", 5_000);
    let mut request = arguments.clone();
    if !request.is_object() {
        request = json!({});
    }
    request["op"] = json!("input");
    request["action"] = json!(tool);
    request["timeout_ms"] = json!(timeout.as_millis() as u64);
    let answer = ask(&session.socket, &request, timeout).map_err(|error| {
        let phase = session.phase();
        if phase.is_live() {
            format!("cannot reach the running program: {error}")
        } else {
            format!(
                "the program started for {} isn't running (state: {})",
                session.root.display(),
                phase.name()
            )
        }
    })?;
    match answer["error"].as_str() {
        Some("paused") => {
            let paused = &answer["paused"];
            return Err(format!(
                "the program is paused at {}:{}, which holds its window's event loop: it takes input once `continue` or `step` lets it go on",
                paused["file"].as_str().unwrap_or_default(),
                paused["line"]
            ));
        }
        Some(error) => {
            return Err(format!(
                "{error} (the program started for {} is {})",
                session.root.display(),
                session.phase().name()
            ));
        }
        None => {}
    }
    let window = &answer["window"];
    let mut summary = match answer["cursor"].as_object() {
        Some(cursor) => format!("{tool} at {}, {}", cursor["x"], cursor["y"]),
        None => format!("{tool} sent"),
    };
    summary.push_str(&format!(
        " in {} ({} × {} points)",
        match window["title"].as_str() {
            Some("") | None => "the untitled window".to_owned(),
            Some(title) => format!("`{title}`"),
        },
        window["width"],
        window["height"]
    ));
    if answer["cursor"].is_object() {
        summary.push_str("; the agent cursor is there");
    }
    let mut structured = answer;
    structured["root"] = json!(session.root);
    Ok(content::success(structured, summary, Vec::new()))
}

/// One request and its answer over the program's socket.
#[cfg(unix)]
fn ask(socket: &Path, request: &Value, timeout: Duration) -> std::io::Result<Value> {
    use std::io::{BufRead as _, BufReader, Write as _};
    // A program that just started or restarted serves the socket once it
    // compiled.
    let deadline = Instant::now() + timeout;
    let mut stream = loop {
        match std::os::unix::net::UnixStream::connect(socket) {
            Ok(stream) => break stream,
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    // The program gives up on its side at `timeout`; this side waits a
    // little longer for that answer.
    stream.set_read_timeout(Some(timeout + Duration::from_secs(2)))?;
    writeln!(stream, "{request}")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

#[cfg(not(unix))]
fn ask(_socket: &Path, _request: &Value, _timeout: Duration) -> std::io::Result<Value> {
    Err(std::io::Error::other("reading values needs a Unix socket"))
}

fn is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        i32::try_from(pid)
            .is_ok_and(|pid| nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// `screenshot`: an extension's own capture first (iced's socket needs no
/// permission), else the started program's window by its pids.
fn screenshot(
    server: &Server,
    path: Option<&str>,
    title: Option<&str>,
    cancel: &Cancel,
    progress: &Progress<'_>,
) -> Result<Value, String> {
    let place = place(server, path)?;
    let session = server.sessions.get(&place.root);
    let installed = layer::installed();
    let mut reasons = Vec::new();
    if let Some(offered) = extension::find(&installed, "screenshot") {
        let call = content::Call {
            arguments: json!({ "title": title }),
            root: place.root.clone(),
            target_dir: place.target_dir.clone(),
            pid: session.as_ref().map(|session| session.pid),
        };
        let result = extension::call(
            &offered.binary,
            "screenshot",
            &call,
            Instant::now() + BOUND,
            cancel,
            |record| relay(progress, record),
        );
        if result["isError"] != true {
            return Ok(result);
        }
        if let Some(reason) = result["content"][0]["text"].as_str() {
            reasons.push(format!("{}: {reason}", offered.extension));
        }
    }
    // rawr's own capture asks macOS for the window. Elsewhere only an
    // extension's screenshot tool can capture one.
    if !cfg!(target_os = "macos") {
        reasons
            .push("a window capture without an extension's screenshot tool needs macOS".to_owned());
        return Err(reasons.join("\n"));
    }
    let Some(session) = session else {
        reasons.push(format!(
            "nothing was started for {}: call `start`",
            place.root.display()
        ));
        return Err(reasons.join("\n"));
    };
    let windows = window::windows(&process::tree(session.pid));
    let chosen = title
        .and_then(|title| windows.iter().find(|window| window.title == title))
        .or_else(|| windows.first());
    let Some(chosen) = chosen else {
        reasons.push(format!(
            "the program started for {} shows no window yet (state: {})",
            place.root.display(),
            session.phase().name()
        ));
        return Err(reasons.join("\n"));
    };
    let file = place.directory().join("screenshot.png");
    let bytes = window::capture(chosen, &file)?;
    let points = window::points(&bytes);
    let mut summary = format!(
        "window {} of pid {} ({}): {}",
        chosen.number,
        chosen.pid,
        if chosen.title.is_empty() {
            "untitled"
        } else {
            &chosen.title
        },
        file.display()
    );
    if let Some((width, height)) = points {
        summary.push_str(&format!(
            "\n{width} × {height} points: `click` and the other input tools take positions in these points, from the top-left corner"
        ));
    }
    Ok(content::success(
        json!({
            "file": file,
            "title": chosen.title,
            "pid": chosen.pid,
            "window": chosen.number,
            "points": points.map(|(width, height)| json!({ "width": width, "height": height })),
        }),
        summary,
        vec![content::png(&bytes)],
    ))
}

fn relay(progress: &Progress<'_>, record: &Value) {
    progress.send(
        record["progress"].as_f64().unwrap_or(0.0),
        record["total"].as_f64(),
        record["message"].as_str().unwrap_or_default(),
    );
}

/// A tool an installed extension declares.
fn extension_call(
    server: &Server,
    name: &str,
    arguments: &Value,
    cancel: &Cancel,
    progress: &Progress<'_>,
) -> Result<Value, String> {
    let installed = layer::installed();
    let Some(offered) = extension::offered(&installed, &CORE)
        .into_iter()
        .find(|offered| offered.tool.name == name)
    else {
        return Err(format!("unknown tool `{name}`"));
    };
    // An extension may serve a directory that holds no program yet.
    let path = arguments.get("path").and_then(Value::as_str);
    let place = place(server, path).or_else(|reason| {
        let directory = absolute(server, path);
        if directory.is_dir() {
            Ok(Place {
                target_dir: target_directory(&directory),
                root: directory.clone(),
                path: directory,
            })
        } else {
            Err(reason)
        }
    })?;
    let call = content::Call {
        arguments: arguments.clone(),
        root: place.root.clone(),
        target_dir: place.target_dir.clone(),
        pid: server.sessions.get(&place.root).map(|session| session.pid),
    };
    Ok(extension::call(
        &offered.binary,
        name,
        &call,
        Instant::now() + BOUND,
        cancel,
        |record| relay(progress, record),
    ))
}
