//! rawr-iced's tools for `rawr mcp` (D537.2). Core lists them from
//! rawr-iced's manifest and runs each call as `rawr-iced mcp call <tool>`,
//! with the call on stdin and the project root core resolved
//! ([`rawr_cli::mcp::content::Call`]). The window tools talk to the live
//! window's agent socket (`rawr_iced::reload::agent`), whoever started it.

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::Command;

use rawr_cli::mcp::content::{self, Call};
use serde_json::{json, Value};

/// Run one tool call: read it from stdin and write its result to stdout.
pub fn call(tool: &str) -> std::io::Result<()> {
    // The server's death ends this call and what it started.
    #[cfg(feature = "native-runner")]
    rawr_cli::events::watch_parent(|| drop(rawr_import::process::retire_owned()));
    let call = Call::read()?;
    let result = run(tool, &call).unwrap_or_else(content::failure);
    content::finish(&result);
    Ok(())
}

fn run(tool: &str, call: &Call) -> Result<Value, String> {
    match tool {
        "list_previews" => list_previews(&call.root),
        "render_preview" => render_preview(&call.root, call.text("name"), call.text("theme")),
        "screenshot" => agent(&call.root, "screenshot\n").map_err(Agent::into_message),
        "click" | "type_text" | "press_key" | "do" | "reset" => {
            let mut request = input(tool, &call.arguments)?;
            if call.arguments.get("screenshot").and_then(Value::as_bool) == Some(true) {
                request.push_str("screenshot\n");
            }
            agent(&call.root, &request).map_err(Agent::into_message)
        }
        tool => Err(format!("rawr-iced has no tool `{tool}`")),
    }
}

/// The project's previews as written, read from its parsed sources.
fn previews(root: &Path) -> Result<Vec<rawr::preview::Listed>, String> {
    let target = rawr::project::Target::discover(root).map_err(|error| error.to_string())?;
    let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target)
        .map_err(|error| error.to_string())?;
    Ok(graph.previews())
}

fn list_previews(root: &Path) -> Result<Value, String> {
    let previews = previews(root)?;
    let listed: Vec<Value> = previews
        .iter()
        .map(|preview| {
            json!({
                "group": preview.group,
                "name": preview.name,
                "subject": preview.subject,
                "width": preview.size.0,
                "height": preview.size.1,
                "themes": preview.themes,
                "snapshot": preview.snapshot,
            })
        })
        .collect();
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
    let summary = if lines.is_empty() {
        "no #[preview]s".to_owned()
    } else {
        lines.join("\n")
    };
    Ok(content::success(
        json!({ "previews": listed }),
        summary,
        Vec::new(),
    ))
}

/// Render the preview `name` alone with one theme through `rawr snapshot
/// --check` (its selection renders only that preview into the disposable
/// actual tree; baselines are never compared or written) and return the PNG.
fn render_preview(root: &Path, name: Option<&str>, theme: Option<&str>) -> Result<Value, String> {
    let name = name.ok_or("`name` is required")?;
    let previews = previews(root)?;
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
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(executable)
        .args(["snapshot".as_ref(), "--check".as_ref(), root.as_os_str()])
        .env(
            rawr_iced::snapshot::SELECTION,
            format!("{}/{}\t{theme}", listed.group, listed.name),
        )
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(log);
    }
    // The snapshot store keeps its rendered tree under the project root.
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
    Ok(content::success(
        json!({
            "preview": format!("{}/{}", listed.group, listed.name),
            "theme": theme,
            "width": width.parse::<u64>().ok(),
            "height": height.parse::<u64>().ok(),
            "file": file,
        }),
        format!(
            "{}/{} ({theme}) {width}x{height}: {}",
            listed.group,
            listed.name,
            file.display()
        ),
        vec![content::png(&bytes)],
    ))
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
/// into a result: its lines, a screenshot image, and diagnostics as data.
fn agent(root: &Path, request: &str) -> Result<Value, Agent> {
    let socket = rawr_iced::reload::agent::socket(root);
    let mut stream = std::os::unix::net::UnixStream::connect(&socket).map_err(|error| {
        Agent::Closed(format!(
            "no rawr-iced window listens for {} ({}: {error}); call `start` first",
            root.display(),
            socket.display()
        ))
    })?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
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
            ));
        }
    };
    let mut images = Vec::new();
    let mut text = Vec::new();
    let mut file = None;
    let mut diagnostics = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[..] {
            ["screenshot", path] => match std::fs::read(path) {
                Ok(bytes) => {
                    images.push(content::png(&bytes));
                    text.push(format!("screenshot: {path}"));
                    file = Some(path.to_owned());
                }
                Err(error) => text.push(format!("cannot read {path}: {error}")),
            },
            ["diagnostic", file, line, column, code, message] => diagnostics.push(json!({
                "source": "rawr",
                "file": file,
                "line": line.parse::<u64>().ok(),
                "column": column.parse::<u64>().ok(),
                "code": code,
                "severity": "error",
                "message": message,
            })),
            _ if !line.is_empty() => text.push(line.to_owned()),
            _ => {}
        }
    }
    let summary = if text.is_empty() {
        "done".to_owned()
    } else {
        text.join("\n")
    };
    let structured = json!({
        "ok": ok,
        "screenshot": file,
        "diagnostics": diagnostics,
        "lines": text,
    });
    if ok {
        Ok(content::success(structured, summary, images))
    } else {
        Err(Agent::Failed(summary))
    }
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

#[cfg(test)]
mod tests {
    use serde_json::json;

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
    fn manifest_fields_decode() {
        assert_eq!(
            super::decode("rawrgallery%3A%3A%4Bit%3A%3Aview%23dial"),
            "rawrgallery::Kit::view#dial"
        );
    }

    #[test]
    fn the_manifest_declares_the_tools_this_binary_runs() {
        let manifest = rawr_cli::layer::Manifest::parse(crate::layer::MANIFEST).unwrap();
        let names: Vec<&str> = manifest
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "list_previews",
                "render_preview",
                "click",
                "type_text",
                "press_key",
                "do",
                "reset",
                "screenshot"
            ]
        );
        for tool in &manifest.tools {
            assert_eq!(tool.input["type"], "object", "{}", tool.name);
        }
    }
}
