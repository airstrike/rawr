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
pub const CORE: [&str; 8] = [
    "new",
    "check",
    "run",
    "start",
    "wait",
    "status",
    "stop",
    "screenshot",
];

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
            "description": "Wait for what happens next to the started program: `running`, `swapped` (a save reloaded it in place, keeping its state), `restarting`, `failed` (with diagnostics; the last good program keeps running) or `ended`. Returns those events and the output since the last call. Call it after each save.",
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
            "description": "The programs this server started: each one's state, without waiting.",
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
         Loop: `new` makes a crate. Edit its files with your own tools. `check` it. `run` a command-line program to its end with args and stdin, or `start` a window or long-running program and `wait` after each save to see it reload or fail. `screenshot` shows its window. `stop` ends it.\n\
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
        "screenshot" => screenshot(server, text("path"), text("title"), cancel, &progress),
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
            })
        })
        .collect();
    let summary = if list.is_empty() {
        "no programs started".to_owned()
    } else {
        list.iter()
            .map(|entry| {
                format!(
                    "{}: {} (pid {})",
                    entry["root"].as_str().unwrap_or_default(),
                    entry["state"].as_str().unwrap_or_default(),
                    entry["pid"]
                )
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
    Ok(content::success(
        json!({ "file": file, "title": chosen.title, "pid": chosen.pid, "window": chosen.number }),
        format!(
            "window {} of pid {} ({}): {}",
            chosen.number,
            chosen.pid,
            if chosen.title.is_empty() {
                "untitled"
            } else {
                &chosen.title
            },
            file.display()
        ),
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
