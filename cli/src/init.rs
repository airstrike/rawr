//! `rawr init [--claude] [--codex] [path]`: register `rawr mcp` in a
//! project's agent config (D537.7).
//!
//! The entry runs this `rawr` by its absolute, canonical path, so a stale
//! `rawr` elsewhere on `PATH` is never started, with `args = ["mcp"]` and
//! the project root as its working directory. Codex gets longer tool and
//! startup timeouts. An existing file is kept as `<file>.bak` before it is
//! rewritten; an old `rawr` entry is replaced and every other setting
//! stays. With no flag, the project's `.claude` or `.codex` directory says
//! which agent it uses.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;
use toml_edit::{value, DocumentMut, Item, Table};

/// Claude Code's project-scoped MCP config, relative to the project root.
pub const CLAUDE_CONFIG: &str = ".mcp.json";
/// Codex's project-scoped config, relative to the project root.
pub const CODEX_CONFIG: &str = ".codex/config.toml";

/// How long Codex may wait for one tool call, in seconds. Each of rawr's
/// calls returns within about 45 s.
pub const CODEX_TOOL_TIMEOUT: i64 = 120;
/// How long Codex may wait for the server to start, in seconds.
pub const CODEX_STARTUP_TIMEOUT: i64 = 30;

/// The server entry: the command, its arguments and its working directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The absolute, canonical path of `rawr`.
    pub command: PathBuf,
    /// The project root.
    pub root: PathBuf,
}

impl Entry {
    /// The entry for this `rawr`, serving the project at `root`.
    pub fn current(root: &Path) -> io::Result<Self> {
        Ok(Self {
            command: fs::canonicalize(std::env::current_exe()?)?,
            root: fs::canonicalize(root)?,
        })
    }
}

/// One config file written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Written {
    /// The config file.
    pub path: PathBuf,
    /// The copy of what it held before, when it existed.
    pub backup: Option<PathBuf>,
}

impl fmt::Display for Written {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Wrote {}", self.path.display())?;
        if let Some(backup) = &self.backup {
            write!(formatter, " (the old file is {})", backup.display())?;
        }
        Ok(())
    }
}

/// A config file that could not be read, understood or written.
#[derive(Debug)]
pub struct Error {
    path: PathBuf,
    reason: String,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.reason)
    }
}

impl std::error::Error for Error {}

fn error(path: &Path, reason: impl ToString) -> Error {
    Error {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    }
}

/// The agents a project uses, from its `.claude` and `.codex` directories:
/// `(claude, codex)`.
#[must_use]
pub fn detect(root: &Path) -> (bool, bool) {
    (root.join(".claude").is_dir(), root.join(".codex").is_dir())
}

/// Set `mcpServers.rawr` in `<root>/.mcp.json`, keeping every other key.
pub fn claude(entry: &Entry) -> Result<Written, Error> {
    let path = entry.root.join(CLAUDE_CONFIG);
    let old = read(&path)?;
    let mut config = match &old {
        Some(text) => serde_json::from_str(text).map_err(|e| error(&path, e))?,
        None => json!({}),
    };
    let Some(object) = config.as_object_mut() else {
        return Err(error(&path, "expected a JSON object"));
    };
    let servers = object.entry("mcpServers").or_insert_with(|| json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err(error(&path, "`mcpServers` is not an object"));
    };
    servers.insert(
        "rawr".to_owned(),
        json!({
            "command": entry.command,
            "args": ["mcp"],
            "cwd": entry.root,
        }),
    );
    let mut text = serde_json::to_string_pretty(&config).map_err(|e| error(&path, e))?;
    text.push('\n');
    write(&path, old.as_deref(), &text)
}

/// Set `[mcp_servers.rawr]` in `<root>/.codex/config.toml`, keeping every
/// other table, with timeouts that fit a cold build behind `start`.
pub fn codex(entry: &Entry) -> Result<Written, Error> {
    let path = entry.root.join(CODEX_CONFIG);
    let old = read(&path)?;
    let mut document = old
        .as_deref()
        .unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|e| error(&path, e))?;
    if !document.contains_key("mcp_servers") {
        let mut servers = Table::new();
        servers.set_implicit(true);
        document.insert("mcp_servers", Item::Table(servers));
    }
    let Some(servers) = document["mcp_servers"].as_table_mut() else {
        return Err(error(&path, "`mcp_servers` is not a table"));
    };
    let mut table = Table::new();
    table.insert("command", value(entry.command.to_string_lossy().as_ref()));
    let mut args = toml_edit::Array::new();
    args.push("mcp");
    table.insert("args", value(args));
    table.insert("cwd", value(entry.root.to_string_lossy().as_ref()));
    table.insert("tool_timeout_sec", value(CODEX_TOOL_TIMEOUT));
    table.insert("startup_timeout_sec", value(CODEX_STARTUP_TIMEOUT));
    servers.insert("rawr", Item::Table(table));
    write(&path, old.as_deref(), &document.to_string())
}

fn read(path: &Path) -> Result<Option<String>, Error> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(path, e)),
    }
}

/// Write `text` to `path`, first copying what it held to `<path>.bak`.
fn write(path: &Path, old: Option<&str>, text: &str) -> Result<Written, Error> {
    let backup = match old {
        Some(old) => {
            let mut backup = path.as_os_str().to_owned();
            backup.push(".bak");
            let backup = PathBuf::from(backup);
            fs::write(&backup, old).map_err(|e| error(&backup, e))?;
            Some(backup)
        }
        None => None,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| error(parent, e))?;
    }
    fs::write(path, text).map_err(|e| error(path, e))?;
    Ok(Written {
        path: path.to_path_buf(),
        backup,
    })
}

#[cfg(test)]
mod tests {
    use super::{claude, codex, detect, Entry, CLAUDE_CONFIG, CODEX_CONFIG};
    use std::fs;
    use std::path::PathBuf;

    fn sandbox(label: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/rawr-cli-init")
            .join(format!("{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn entry(root: &std::path::Path, command: &str) -> Entry {
        Entry {
            command: PathBuf::from(command),
            root: root.to_path_buf(),
        }
    }

    #[test]
    fn claude_writes_then_refreshes_the_entry_keeping_other_servers_and_a_backup() {
        let root = sandbox("claude");
        let path = root.join(CLAUDE_CONFIG);
        fs::write(
            &path,
            r#"{"mcpServers": {"other": {"command": "other"}, "rawr": {"command": "rawr"}}, "extra": 1}"#,
        )
        .unwrap();
        let first = claude(&entry(&root, "/old/rawr")).unwrap();
        assert_eq!(first.backup, Some(root.join(".mcp.json.bak")));
        let second = claude(&entry(&root, "/new/rawr")).unwrap();
        assert!(second.backup.is_some());
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["other"]["command"], "other");
        assert_eq!(config["extra"], 1);
        assert_eq!(
            config["mcpServers"]["rawr"],
            serde_json::json!({"command": "/new/rawr", "args": ["mcp"], "cwd": root})
        );
        let backup: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(".mcp.json.bak")).unwrap()).unwrap();
        assert_eq!(backup["mcpServers"]["rawr"]["command"], "/old/rawr");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_writes_one_table_with_timeouts_and_refreshes_it() {
        let root = sandbox("codex");
        let path = root.join(CODEX_CONFIG);
        let fresh = codex(&entry(&root, "/old/rawr")).unwrap();
        assert_eq!(fresh.backup, None);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("[mcp_servers.rawr]\ncommand = \"/old/rawr\"\nargs = [\"mcp\"]\n"),
            "{text}"
        );
        assert!(text.contains("tool_timeout_sec = 120\nstartup_timeout_sec = 30\n"));

        fs::write(
            &path,
            "model = \"x\"\n\n[mcp_servers.other]\ncommand = \"other\"\n\n[mcp_servers.rawr]\ncommand = \"rawr\"\nargs = [\"mcp\"]\n",
        )
        .unwrap();
        let refreshed = codex(&entry(&root, "/new/rawr")).unwrap();
        assert_eq!(refreshed.backup, Some(root.join(".codex/config.toml.bak")));
        let text = fs::read_to_string(&path).unwrap();
        let document = text.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(document["model"].as_str(), Some("x"));
        assert_eq!(
            document["mcp_servers"]["other"]["command"].as_str(),
            Some("other")
        );
        assert_eq!(
            document["mcp_servers"]["rawr"]["command"].as_str(),
            Some("/new/rawr")
        );
        assert_eq!(text.matches("[mcp_servers.rawr]").count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_agent_is_detected_from_its_directory() {
        let root = sandbox("detect");
        assert_eq!(detect(&root), (false, false));
        fs::create_dir(root.join(".codex")).unwrap();
        assert_eq!(detect(&root), (false, true));
        fs::remove_dir_all(root).unwrap();
    }
}
