//! Core's tools (D537.6) and the listing that adds the extensions'.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use super::call::Call;
use super::options::Options;
use super::process;
use super::{check, content, extension, session, sessions, Progress, Server};
use crate::layer;

/// Core's tool names: an extension's tool by one of these names is not
/// listed.
pub const CORE: [&str; 20] = [
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
    "eval",
    "break",
    "step",
    "continue",
    "clear",
];

/// What every input tool's description ends with.
const INPUT: &str = "Positions are logical points from the window's top-left corner, title bar included, as `screenshot` shows it. The input goes to the program alone: your own pointer, keyboard and focus stay as they are, and the window isn't brought forward. The window shows an agent cursor where the pointer last went, also in screenshots. Input sent before the program opens its first window waits for that window. Returns once the program has taken the input and drawn again, so a `screenshot` next shows what it did, or once the input made it pause at a breakpoint.";

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

/// The schema of `start`, `run` and `check`: `path`, the options `rawr
/// run` takes beside it ([`Options`]) and `properties` of the tool's own.
fn program_schema(properties: Value) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "path": path_schema(),
            "package": { "type": "string", "description": "A package of the Cargo workspace at `path`, as `cargo run -p`: the program is that package's" },
            "bin": { "type": "string", "description": "A binary of the package, as `cargo run --bin`: the program is that binary" },
            "features": { "type": "array", "items": { "type": "string" }, "description": "Features to enable, as `cargo run --features`; a dependency's as `crate/feature`" },
            "no_default_features": { "type": "boolean", "default": false, "description": "Leave the package's default features off, as `--no-default-features`" },
            "all_features": { "type": "boolean", "default": false, "description": "Enable every feature, as `--all-features`" },
            "env": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Environment variables for this call alone, set over the server's: the build and the program read them, as `CARGO_TARGET_DIR` or a data directory" },
        },
    });
    if let (Some(all), Some(own)) = (schema["properties"].as_object_mut(), properties.as_object()) {
        all.extend(own.clone());
    }
    schema
}

/// An input tool's schema: its own `properties`, which `required` names,
/// and the `path` and `title` every input tool takes.
fn gesture(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "path": path_schema(),
            "title": { "type": "string", "description": "The window's title, when the program has several; defaults to its front window" },
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
            "description": "Check a program without running it: rawr's check and `cargo check`. Returns each error with its absolute file, line and column. A first check of a program with Rust dependencies builds them first. `package`, `bin`, `features`, `no_default_features` and `all_features` choose what is checked as `rawr check` takes them, and `env` sets variables for the check alone.",
            "inputSchema": program_schema(json!({})),
        }),
        json!({
            "name": "run",
            "description": "Run a program to its end, with arguments and stdin, and return its exit status, stdout and stderr. For command-line programs. For a window or a long-running program, use `start`. `package`, `bin`, `features`, `no_default_features` and `all_features` choose what runs as `rawr run` takes them, and `env` sets variables for the build and the program of this run alone.",
            "inputSchema": program_schema(json!({
                "args": arguments,
                "stdin": { "type": "string", "description": "The program's input; it then reads end of input" },
            })),
        }),
        json!({
            "name": "start",
            "description": "Start a program that keeps running, such as a window, and keep it running: a save reloads it, and one that panics or exits with an error runs again on the next save. One that exits with status 0 ends the session, and `start` runs it again. Returns once it runs, with `running`, or `failed` with diagnostics; a first start builds its Rust dependencies first, and tells its progress meanwhile. Starting a program again replaces it, also with another `bin`, other features or another `env`. `package`, `bin`, `features`, `no_default_features` and `all_features` choose what runs as `rawr run` takes them, and `env` sets variables for its builds and its program; every reload keeps them. Programs that draw a terminal UI are refused for now.",
            "inputSchema": program_schema(json!({
                "args": arguments,
                "stdin": { "type": "string", "description": "The program's input; it then reads end of input" },
            })),
        }),
        json!({
            "name": "wait",
            "description": "Wait for what happens next to the started program: `running`, `paused` (at a breakpoint or after a step, with its file and line), `resumed`, `queued` (a save reached a program whose native loop takes it at its next call into script code), `swapped` (a save reloaded it in place, keeping its state), `restarting`, `failed` (with diagnostics; the last good program keeps running) or `ended`. Returns those events and the output since the last call, once either comes: a line the program prints returns it too, so a save may need a second `wait` for its `swapped`. Call it after each save.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "cursor": { "type": "integer", "description": "The last event seen; defaults to the last one returned" },
                    "output_cursor": { "type": "integer", "description": "The last output seen; defaults to the last one returned" },
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
            "description": "Capture the started program's window alone and return the PNG. The program draws the capture itself, so it shows the window as it is while another window covers it, it is off screen or the session is locked, and it needs no Screen Recording permission. The PNG is the window's size in points, title bar included, at its backing scale: the input tools take positions in those points.",
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
                },
            },
        }),
        json!({
            "name": "eval",
            "description": "Evaluate Rust where the started program is paused, as if the code were a block written just before the statement it is paused at: it reads and can change the bindings in scope there, and rustc's rules for that line apply, so a type error or a borrow that conflicts with one the program holds is refused with rustc's error. Returns the value of its last expression as Rust's `{:?}` prints it, read through a borrow so nothing moves; bindings the code makes end with it. With `frame`, it runs in a caller instead, as a block written before the statement making the call the program is in: a binding that call borrows is refused while it runs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "code": { "type": "string", "description": "Rust statements and an optional last expression, such as `items.len() * 2` or `let n = count + 1; n * n`" },
                    "frame": { "type": "integer", "minimum": 0, "default": 0, "description": "Which call it runs in: 0 for the function the program is paused in, 1 for its caller, and on, as `inspect` numbers them" },
                },
                "required": ["code"],
            },
        }),
        json!({
            "name": "break",
            "description": "Set a breakpoint in the started program: on a `line` of a `file`, or on a `function`'s first statement. The program pauses when it reaches it, and `wait` returns the `paused` event with where; `inspect` then reads its values there, and `step` or `continue` resumes it. Returns once the breakpoint is set, with where the program is paused if it already is. A breakpoint stays across saves while its line's text is still in the file, and moves with that text. Code in Rust dependencies has no breakpoints. A pause inside a call from native code, such as a window's event loop, holds that loop: the window doesn't redraw or take input until the program goes on.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "file": { "type": "string", "description": "A source file of the program: absolute, or relative to the server's project root" },
                    "line": { "type": "integer", "minimum": 1, "description": "A line of `file`, from 1" },
                    "function": { "type": "string", "description": "A function of the program, as `tick` or `counter::tick`, in place of `file` and `line`" },
                },
            },
        }),
        json!({
            "name": "step",
            "description": "Run the paused program to its next statement and pause there: `over` stays in this function, `into` goes into a function it calls, and `out` returns from this one. A step over native code, such as a crate's function, runs it whole. Returns where it paused. A step out of a function native code called, such as a window's `update`, pauses at the program's next call from that code.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_schema(),
                    "kind": { "type": "string", "enum": ["over", "into", "out"], "default": "over" },
                },
            },
        }),
        json!({
            "name": "continue",
            "description": "Let the paused program run on. Returns once it runs; when it reaches a breakpoint again, `wait` returns the `paused` event with where.",
            "inputSchema": { "type": "object", "properties": { "path": path_schema() } },
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
         Loop: `new` makes a crate. Edit its files with your own tools. `check` it. `run` a command-line program to its end with args and stdin, or `start` a window or long-running program and `wait` after each save to see it reload or fail. `screenshot` shows its window, and `move`, `click`, `drag`, `scroll`, `key` and `type` send it input, at positions in the screenshot's points. `inspect` reads its values while it runs. `break` pauses it at a line or a function, `eval` runs Rust where it is paused, `step` and `continue` go on from there, and `clear` removes breakpoints. `stop` ends it.\n\
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
    pub(super) fn directory(&self) -> PathBuf {
        let project = if self.root.is_dir() {
            &self.root
        } else {
            self.root.parent().unwrap_or(&self.root)
        };
        rawr_location::rawr_directory_in(&self.target_dir, project).join("mcp")
    }
}

/// `path` as an absolute path, a relative one from `root`; `root` when
/// there is none.
pub(crate) fn absolute(root: &Path, path: Option<&str>) -> PathBuf {
    let path = path.map_or_else(|| root.to_path_buf(), PathBuf::from);
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// Resolve `path`, relative to `root`, once: its program's root and the
/// root's target directory.
pub(crate) fn place(root: &Path, path: Option<&str>) -> Result<Place, String> {
    locate(root, absolute(root, path), &Options::default())
}

/// Resolve `path`, relative to `root`, with a call's `options`: the program
/// they choose there, its root and the target directory the call's
/// environment gives that root.
fn program(root: &Path, path: Option<&str>, options: &Options) -> Result<Place, String> {
    let path = options.program(absolute(root, path))?;
    locate(root, path, options)
}

fn locate(root: &Path, path: PathBuf, options: &Options) -> Result<Place, String> {
    let target = crate::base::discover(Some(&path))
        .map_err(|report| crate::diagnostic::lines(&crate::diagnostic::shape(&report, root)))?;
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
        target_dir: target_directory(&directory, options),
    })
}

/// The Cargo target directory of the project at `directory`, in the
/// server's environment with `options`' variables over it.
#[must_use]
pub fn target_directory(directory: &Path, options: &Options) -> PathBuf {
    let mut environment: HashMap<String, String> = std::env::vars().collect();
    environment.extend(
        options
            .environment()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    let cargo_home = environment
        .get("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .get("HOME")
                .map(|home| PathBuf::from(home).join(".cargo"))
        });
    rawr_location::target_directory(
        directory,
        |key| environment.get(key).map(String::as_str),
        cargo_home.as_deref(),
    )
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
pub fn call(server: &Server, params: &Value, call: &Call) -> Value {
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
        "new" => new(server, text("path"), text("template"), call),
        "check" => Options::new(&arguments).and_then(|options| {
            program(&server.root, text("path"), &options)
                .map(|place| check::check(server, &place, &options, call))
        }),
        "run" => Options::new(&arguments).and_then(|options| {
            program(&server.root, text("path"), &options).map(|place| {
                run(
                    &place,
                    &options,
                    &strings(&arguments, "args"),
                    text("stdin"),
                    call,
                )
            })
        }),
        "start" => Options::new(&arguments).and_then(|options| {
            let place = program(&server.root, text("path"), &options)?;
            start(
                server,
                &place,
                &options,
                &strings(&arguments, "args"),
                text("stdin"),
                call,
                &progress,
            )
        }),
        "wait" => session(server, text("path"), call).map(|session| {
            let output_cursor = arguments.get("output_cursor").and_then(Value::as_u64);
            let after = session.next(
                arguments.get("cursor").and_then(Value::as_u64),
                output_cursor,
                call,
                |stage, percent| progress.send(percent as f64, Some(100.0), stage),
            );
            // A cancelled call gets no answer, so it gives no events: the
            // next call without a cursor still returns them. One a later
            // wait took the place of gives none either.
            if call.is_cancelled() {
                return content::success(json!({}), "cancelled", Vec::new());
            }
            let Some(after) = after else {
                return content::success(
                    json!({ "superseded": true }),
                    "a later `wait` took this one's place",
                    Vec::new(),
                );
            };
            let answer = session.answer(Some(after), output_cursor);
            content::success(answer.structured, answer.summary, Vec::new())
        }),
        "status" => Ok(status(server, text("path"), call)),
        "stop" => stop(server, text("path"), call),
        "inspect" => inspect(
            server,
            text("path"),
            text("name"),
            text("file"),
            arguments.get("frame").and_then(Value::as_u64).unwrap_or(0),
            call,
        ),
        "eval" => evaluate(
            server,
            text("path"),
            text("code").unwrap_or_default(),
            arguments.get("frame").and_then(Value::as_u64).unwrap_or(0),
            call,
        ),
        "break" | "step" | "continue" | "clear" => debug(server, name, &arguments, call),
        "screenshot" => screenshot(server, text("path"), text("title"), call, &progress),
        "move" | "click" | "drag" | "scroll" | "key" | "type" => {
            input(server, name, &arguments, call)
        }
        name => extension_call(server, name, &arguments, call, &progress),
    };
    outcome.unwrap_or_else(content::failure)
}

/// `new`: `rawr new` in a child, so its refusals and templates are the
/// command's own.
fn new(
    server: &Server,
    path: Option<&str>,
    template: Option<&str>,
    call: &Call,
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
    let finished = process::run(&mut command, None, call).map_err(|error| error.to_string())?;
    let (stdout, _) = finished.stdout.finish();
    let (stderr, _) = finished.stderr.finish();
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

/// `run`: `rawr run <path> <feature flags> -- args` to its end, in the
/// call's environment.
fn run(
    place: &Place,
    options: &Options,
    arguments: &[String],
    stdin: Option<&str>,
    call: &Call,
) -> Value {
    let Ok(executable) = std::env::current_exe() else {
        return content::failure("cannot find rawr");
    };
    let mut command = Command::new(executable);
    command.arg("run").arg(&place.path);
    options.apply(&mut command);
    command
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
    let finished = match process::run(&mut command, Some(stdin.unwrap_or_default()), call) {
        Ok(finished) => finished,
        Err(error) => return content::failure(format!("cannot run rawr: {error}")),
    };
    let (stdout, stdout_cut) = finished.stdout.finish();
    let (stderr, stderr_cut) = finished.stderr.finish();
    let status = finished.status.map(process::code);
    let structured = json!({
        "status": status,
        "stdout": stdout,
        "stderr": stderr,
        "output_dropped": stdout_cut || stderr_cut,
        "seconds": finished.elapsed.as_secs_f64(),
    });
    let mut summary = match status {
        Some(status) => format!("exit status {status}\n"),
        None => "cancelled\n".to_owned(),
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
pub(crate) fn terminal_ui(root: &Path) -> Option<&'static str> {
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
    options: &Options,
    arguments: &[String],
    stdin: Option<&str>,
    call: &Call,
    progress: &Progress<'_>,
) -> Result<Value, String> {
    if let Some(name) = terminal_ui(&place.root) {
        return Err(format!(
            "this program uses `{name}`, which draws a terminal UI, and `start` can't host a terminal yet. Run it with `rawr run` in a terminal of your own."
        ));
    }
    let session = sessions::start(
        server,
        &place.root,
        &place.path,
        options,
        arguments,
        stdin,
        call,
    )
    .map_err(|error| format!("cannot start rawr run: {error}"))?;
    session.settle(call, |stage, percent| {
        progress.send(percent as f64, Some(100.0), stage);
    });
    let answer = session.answer(None, None);
    let mut summary = answer.summary;
    if !session.is_started() {
        summary.insert_str(
            0,
            "A `rawr run` started elsewhere serves this program, so `start` attached to it. That run keeps its own arguments, features, environment and input.\n",
        );
    }
    if session.coverage() == Some(session::Coverage::Engine) {
        summary.push_str(session::AT_TERMINAL);
        summary.push('\n');
    }
    Ok(content::success(answer.structured, summary, Vec::new()))
}

/// The session for `path`, the server's or one it attaches to, or why
/// there is none.
fn session(
    server: &Server,
    path: Option<&str>,
    call: &Call,
) -> Result<std::sync::Arc<session::Session>, String> {
    let place = place(&server.root, path)?;
    sessions::get(server, &place.root, call).ok_or_else(|| {
        format!(
            "nothing was started for {}: call `start` first",
            place.root.display()
        )
    })
}

fn status(server: &Server, path: Option<&str>, call: &Call) -> Value {
    let reached = match path {
        Some(_) => session(server, path, call)
            .map(|session| vec![session])
            .unwrap_or_default(),
        None => sessions::all(server, call),
    };
    let list: Vec<Value> = reached
        .iter()
        .map(|session| {
            json!({
                "root": session.root,
                "path": session.path,
                "args": session.arguments,
                "pid": session.pid(),
                "state": session.phase().name(),
                "paused": session.paused(),
                "output": session.coverage().map(session::Coverage::name),
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
                if entry["output"] == session::Coverage::Engine.name() {
                    line.push_str(&format!("\n{}", session::AT_TERMINAL));
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    content::success(json!({ "sessions": list }), summary, Vec::new())
}

fn stop(server: &Server, path: Option<&str>, call: &Call) -> Result<Value, String> {
    let place = place(&server.root, path)?;
    let Some(session) = sessions::stop(server, &place.root, call) else {
        return Ok(content::success(
            json!({ "stopped": false }),
            format!("nothing was running for {}", place.root.display()),
            Vec::new(),
        ));
    };
    if !session.is_started() {
        return Ok(content::success(
            json!({ "stopped": false, "detached": true, "root": session.root, "pid": session.pid() }),
            format!(
                "detached from {}; the `rawr run` started elsewhere keeps running",
                session.root.display()
            ),
            Vec::new(),
        ));
    }
    // The group has its grace to end, then is killed.
    session.wait_exit(call);
    Ok(content::success(
        json!({ "stopped": true, "root": session.root, "pid": session.pid() }),
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
    call: &Call,
) -> Result<Value, String> {
    let session = session(server, path, call)?;
    let file = file.map(|file| absolute(&server.root, Some(file)));
    let request = json!({
        "name": name,
        "file": file,
        "frame": frame,
    });
    let (answer, _) = ask(&session, &request, call)?;
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
            "the program read no values: it ran none of its own code{place}"
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
    if let Some(why) = answer["unread"].as_str() {
        summary.push_str(&format!("no bindings to read: {why}\n"));
    } else if bindings.is_empty() {
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
        "unread": answer["unread"],
        "frames": frames,
    });
    Ok(content::success(structured, summary.trim_end(), Vec::new()))
}

/// `eval`: ask the paused program to evaluate `code` in frame `frame`
/// where it is paused.
fn evaluate(
    server: &Server,
    path: Option<&str>,
    code: &str,
    frame: u64,
    call: &Call,
) -> Result<Value, String> {
    let session = session(server, path, call)?;
    let request = json!({ "op": "eval", "code": code, "frame": frame });
    let (answer, _) = ask(&session, &request, call)?;
    match (answer["value"].as_str(), answer["error"].as_str()) {
        (Some(value), _) => Ok(content::success(
            json!({ "root": session.root, "value": value }),
            value,
            Vec::new(),
        )),
        (None, Some("not paused")) => Err(
            "the program isn't paused, so there is no frame to evaluate in. Set a breakpoint with `break`; the program pauses when it reaches it.".to_owned(),
        ),
        (None, Some(error)) => Err(error.to_owned()),
        (None, None) => Err("the program gave no answer".to_owned()),
    }
}

/// `break`, `step`, `continue` and `clear`: ask the started program over
/// its socket, and say where it paused.
fn debug(server: &Server, tool: &str, arguments: &Value, call: &Call) -> Result<Value, String> {
    let session = session(server, arguments.get("path").and_then(Value::as_str), call)?;
    let file = arguments
        .get("file")
        .and_then(Value::as_str)
        .map(|file| absolute(&server.root, Some(file)));
    let request = json!({
        "op": tool,
        "file": file,
        "line": arguments.get("line"),
        "function": arguments.get("function"),
        "kind": arguments.get("kind"),
    });
    let (answer, seen) = ask(&session, &request, call)?;
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
    session.observe_pause(paused.as_object().map(|_| paused.clone()), seen);
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
    } else if tool == "break" {
        summary.push_str("the program runs; `wait` says when it pauses");
    } else if answer["native"] == true {
        summary.push_str(
            "the step went on into the native code that called the program, where it can't stop; the program pauses at the next statement it runs, which `wait` says",
        );
    } else if tool == "continue" {
        summary.push_str("the program runs on; `wait` says when it pauses again");
    } else {
        summary.push_str("the program runs");
    }
    let mut structured = answer;
    structured["root"] = json!(session.root);
    Ok(content::success(structured, summary.trim_end(), Vec::new()))
}

/// `move`, `click`, `drag`, `scroll`, `key` and `type`: ask the started
/// program over its socket to hand its window the input, and say what it
/// reached.
fn input(server: &Server, tool: &str, arguments: &Value, call: &Call) -> Result<Value, String> {
    let session = session(server, arguments.get("path").and_then(Value::as_str), call)?;
    let mut request = arguments.clone();
    if !request.is_object() {
        request = json!({});
    }
    request["op"] = json!("input");
    request["action"] = json!(tool);
    let (answer, seen) = ask(&session, &request, call)?;
    match answer["error"].as_str() {
        Some("paused") if answer["taken"] == true => {
            let paused = &answer["paused"];
            session.observe_pause(Some(paused.clone()), seen);
            let mut summary = format!(
                "{tool} sent; the program paused at {}:{}",
                paused["file"].as_str().unwrap_or_default(),
                paused["line"]
            );
            if let Some(function) = paused["function"].as_str() {
                summary.push_str(&format!(" in `{function}`"));
            }
            summary.push_str(&format!(
                " ({})\nRead values with `inspect`, and go on with `step` or `continue`.",
                paused["reason"].as_str().unwrap_or_default()
            ));
            let mut structured = answer;
            structured["root"] = json!(session.root);
            return Ok(content::success(structured, summary, Vec::new()));
        }
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

/// One request and its answer from the started program, through its run's
/// session, or why there is none. The session gives a request made while
/// the program starts or restarts to the program that comes of it, not the
/// one it retires (#1964).
fn ask(
    session: &session::Session,
    request: &Value,
    call: &Call,
) -> Result<(Value, session::Seen), String> {
    session.ask(request, call).map_err(|refusal| match refusal {
        session::Refusal::NotRunning => format!(
            "the program started for {} isn't running (state: {})",
            session.root.display(),
            session.phase().name()
        ),
        session::Refusal::Error(error) => error,
        session::Refusal::Cancelled => "cancelled".to_owned(),
    })
}

/// `screenshot`: an extension's own capture first, else the started
/// program's own (#1989): it draws its window through its own renderer
/// and writes the PNG, asked over its socket, so nothing asks the window
/// server for the window's pixels.
fn screenshot(
    server: &Server,
    path: Option<&str>,
    title: Option<&str>,
    call: &Call,
    progress: &Progress<'_>,
) -> Result<Value, String> {
    let place = place(&server.root, path)?;
    let session = sessions::get(server, &place.root, call);
    let installed = layer::installed();
    let mut reasons = Vec::new();
    if let Some(offered) = extension::find(&installed, "screenshot") {
        let asked = content::Call {
            arguments: json!({ "title": title }),
            root: place.root.clone(),
            target_dir: place.target_dir.clone(),
            pid: session.as_ref().map(|session| session.pid()),
        };
        let result = extension::call(&offered.binary, "screenshot", &asked, call, |record| {
            relay(progress, record)
        });
        if result["isError"] != true {
            return Ok(result);
        }
        if let Some(reason) = result["content"][0]["text"].as_str() {
            reasons.push(format!("{}: {reason}", offered.extension));
        }
    }
    let Some(session) = session else {
        reasons.push(format!(
            "nothing was started for {}: call `start`",
            place.root.display()
        ));
        return Err(reasons.join("\n"));
    };
    let file = place.directory().join("screenshot.png");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let request = json!({ "op": "screenshot", "title": title, "file": file });
    let (answer, _) = ask(&session, &request, call)?;
    match answer["error"].as_str() {
        Some("paused") => {
            let paused = &answer["paused"];
            reasons.push(format!(
                "the program is paused at {}:{}, which holds its window's event loop: its window draws once `continue` or `step` lets it go on",
                paused["file"].as_str().unwrap_or_default(),
                paused["line"]
            ));
            return Err(reasons.join("\n"));
        }
        Some(error) => {
            reasons.push(format!(
                "{error} (the program started for {} is {})",
                session.root.display(),
                session.phase().name()
            ));
            return Err(reasons.join("\n"));
        }
        None => {}
    }
    let bytes = std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?;
    let shown = match answer["title"].as_str() {
        Some("") | None => "the untitled window".to_owned(),
        Some(title) => format!("`{title}`"),
    };
    let summary = format!(
        "{shown}: {}\n{} × {} points: `click` and the other input tools take positions in these points, from the top-left corner, title bar included",
        file.display(),
        answer["width"],
        answer["height"]
    );
    Ok(content::success(
        json!({
            "file": file,
            "title": answer["title"],
            "pid": session.pid(),
            "points": { "width": answer["width"], "height": answer["height"] },
            "scale": answer["scale"],
            "windows": answer["windows"],
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
    call: &Call,
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
    let place = place(&server.root, path).or_else(|reason| {
        let directory = absolute(&server.root, path);
        if directory.is_dir() {
            Ok(Place {
                target_dir: target_directory(&directory, &Options::default()),
                root: directory.clone(),
                path: directory,
            })
        } else {
            Err(reason)
        }
    })?;
    let asked = content::Call {
        arguments: arguments.clone(),
        root: place.root.clone(),
        target_dir: place.target_dir.clone(),
        pid: sessions::get(server, &place.root, call).map(|session| session.pid()),
    };
    Ok(extension::call(
        &offered.binary,
        name,
        &asked,
        call,
        |record| relay(progress, record),
    ))
}
