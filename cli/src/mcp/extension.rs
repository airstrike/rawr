//! Tools that installed extensions declare in their manifests (D537.2),
//! each run per call as `rawr-<name> mcp call <tool>` (see
//! [`super::content`] for the child's side).

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use super::call::{Call, Wake};
use super::content;
use super::process::{self, Tail};
use crate::layer::{Installed, Tool};

/// An extension's tool, as listed.
#[derive(Clone, Debug)]
pub struct Offered {
    /// The binary that runs it.
    pub binary: PathBuf,
    /// The extension's name.
    pub extension: String,
    /// The tool.
    pub tool: Tool,
}

/// The tools `installed` extensions offer: a core name wins over an
/// extension's, and the first extension over a later one.
#[must_use]
pub fn offered(installed: &[Installed], core: &[&str]) -> Vec<Offered> {
    let mut offered: Vec<Offered> = Vec::new();
    for binary in installed {
        for tool in &binary.manifest.tools {
            if core.contains(&tool.name.as_str())
                || offered.iter().any(|known| known.tool.name == tool.name)
            {
                continue;
            }
            offered.push(Offered {
                binary: binary.path.clone(),
                extension: binary.manifest.name.clone(),
                tool: tool.clone(),
            });
        }
    }
    offered
}

/// The installed extension tool named `name`, a core name included (an
/// extension may serve one that core asks it for, as `screenshot`).
#[must_use]
pub fn find(installed: &[Installed], name: &str) -> Option<Offered> {
    installed.iter().find_map(|binary| {
        binary
            .manifest
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .map(|tool| Offered {
                binary: binary.path.clone(),
                extension: binary.manifest.name.clone(),
                tool: tool.clone(),
            })
    })
}

/// The tool's listing: its schema gains the `path` every tool takes.
#[must_use]
pub fn listing(offered: &Offered) -> Value {
    let mut schema = offered.tool.input.clone();
    if !schema.is_object() {
        schema = json!({ "type": "object" });
    }
    if schema.get("properties").is_none() {
        schema["properties"] = json!({});
    }
    if schema["properties"].get("path").is_none() {
        schema["properties"]["path"] = super::tools::path_schema();
    }
    json!({
        "name": offered.tool.name,
        "description": format!("{} (from {})", offered.tool.description, offered.extension),
        "inputSchema": schema,
    })
}

/// Run `asked`, one call of `tool` from `binary`, until it answers, ends or
/// `call` is cancelled. `progress` hears its progress records.
pub fn call(
    binary: &Path,
    tool: &str,
    asked: &content::Call,
    call: &Call,
    mut progress: impl FnMut(&Value),
) -> Value {
    let mut command = Command::new(binary);
    command
        .args(["mcp", "call", tool])
        .current_dir(if asked.root.is_dir() {
            &asked.root
        } else {
            asked.root.parent().unwrap_or(&asked.root)
        })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match process::spawn(&mut command) {
        Ok(child) => child,
        Err(error) => {
            return content::failure(format!("cannot run {}: {error}", binary.display()));
        }
    };
    let group = child.id();
    let input = asked.encode();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = std::thread::Builder::new()
            .name("rawr-mcp-call-input".into())
            .spawn(move || {
                let _ = stdin.write_all(input.as_bytes());
            });
    }
    let stderr = child.stderr.take().map(Tail::read).unwrap_or_default();
    let lines = call.waker();
    match child.stdout.take() {
        Some(stdout) => {
            let _ = std::thread::Builder::new()
                .name("rawr-mcp-call-output".into())
                .spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        let Ok(line) = line else {
                            break;
                        };
                        if let Ok(record) = serde_json::from_str::<Value>(&line)
                            && !lines.wake(Wake::Line {
                                group,
                                record: Some(record),
                            })
                        {
                            return;
                        }
                    }
                    lines.wake(Wake::Line {
                        group,
                        record: None,
                    });
                });
        }
        None => {
            lines.wake(Wake::Line {
                group,
                record: None,
            });
        }
    }
    let mut result = None;
    let mut ended = false;
    loop {
        let heard = call.wait(|wake| match wake {
            Wake::Line {
                group: from,
                record,
            } if from == group => Some(record),
            _ => None,
        });
        match heard {
            Some(Some(record)) => {
                if let Some(answer) = record.get("result") {
                    result = Some(answer.clone());
                    break;
                }
                if record.get("progress").is_some() {
                    progress(&record);
                }
            }
            None => break,
            Some(None) => {
                ended = true;
                break;
            }
        }
    }
    let status = if result.is_some() || ended {
        let status = child.wait().ok();
        process::kill_group(group);
        status
    } else {
        process::end_group(group);
        let _ = child.wait();
        None
    };
    if let Some(result) = result {
        return result;
    }
    let (errors, _) = stderr.finish();
    if !ended {
        return content::failure("cancelled");
    }
    content::failure(format!(
        "{} ended without a result ({}):\n{}",
        binary.display(),
        status.map_or_else(|| "no status".to_owned(), |status| status.to_string()),
        errors.trim_end()
    ))
}
