//! `rawr init --claude` / `--codex`: register `rawr mcp` in a project's
//! agent config, adding the `rawr` server only when it is absent.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Claude Code's project-scoped MCP config, relative to the project root.
pub const CLAUDE_CONFIG: &str = ".mcp.json";
/// Codex's project-scoped config, relative to the project root.
pub const CODEX_CONFIG: &str = ".codex/config.toml";

const CODEX_TABLE: &str = "[mcp_servers.rawr]\ncommand = \"rawr\"\nargs = [\"mcp\"]\n";

/// What one registration did to its config file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The `rawr` server was added (the file was created or merged).
    Written(PathBuf),
    /// The file already named a `rawr` server; it was left untouched.
    Registered(PathBuf),
}

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Written(path) => write!(formatter, "Wrote {}", path.display()),
            Self::Registered(path) => {
                write!(formatter, "{} already registers rawr", path.display())
            }
        }
    }
}

/// A config file that could not be read, understood, or written.
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

/// Add `"rawr": {"command": "rawr", "args": ["mcp"]}` to `<root>/.mcp.json`'s
/// `mcpServers`, keeping every other server.
pub fn claude(root: &Path) -> Result<Outcome, Error> {
    let path = root.join(CLAUDE_CONFIG);
    let mut config = match read(&path)? {
        Some(text) => serde_json::from_str(&text).map_err(|e| error(&path, e))?,
        None => serde_json::json!({}),
    };
    let Some(config) = config.as_object_mut() else {
        return Err(error(&path, "expected a JSON object"));
    };
    let servers = config
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err(error(&path, "`mcpServers` is not an object"));
    };
    if servers.contains_key("rawr") {
        return Ok(Outcome::Registered(path));
    }
    servers.insert(
        "rawr".to_owned(),
        serde_json::json!({"command": "rawr", "args": ["mcp"]}),
    );
    let mut text = serde_json::to_string_pretty(config).map_err(|e| error(&path, e))?;
    text.push('\n');
    fs::write(&path, text).map_err(|e| error(&path, e))?;
    Ok(Outcome::Written(path))
}

/// Append a `[mcp_servers.rawr]` table to `<root>/.codex/config.toml`, the
/// project layer Codex reads once the project is trusted.
pub fn codex(root: &Path) -> Result<Outcome, Error> {
    let path = root.join(CODEX_CONFIG);
    let mut text = read(&path)?.unwrap_or_default();
    let registered = text.lines().any(|line| {
        let line = line.trim();
        line.starts_with("[mcp_servers.rawr]") || line.starts_with("[mcp_servers.\"rawr\"]")
    });
    if registered {
        return Ok(Outcome::Registered(path));
    }
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
    }
    text.push_str(CODEX_TABLE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| error(parent, e))?;
    }
    fs::write(&path, text).map_err(|e| error(&path, e))?;
    Ok(Outcome::Written(path))
}

fn read(path: &Path) -> Result<Option<String>, Error> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::{CLAUDE_CONFIG, CODEX_CONFIG, Outcome, claude, codex};
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

    #[test]
    fn claude_writes_merges_and_then_leaves_the_file_alone() {
        let root = sandbox("claude-fresh");
        let path = root.join(CLAUDE_CONFIG);
        assert_eq!(claude(&root).unwrap(), Outcome::Written(path.clone()));
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config,
            serde_json::json!({"mcpServers": {"rawr": {"command": "rawr", "args": ["mcp"]}}})
        );
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(claude(&root).unwrap(), Outcome::Registered(path.clone()));
        assert_eq!(fs::read_to_string(&path).unwrap(), written);

        let root = sandbox("claude-merge");
        let path = root.join(CLAUDE_CONFIG);
        fs::write(
            &path,
            r#"{"mcpServers": {"other": {"command": "other"}}, "extra": 1}"#,
        )
        .unwrap();
        assert_eq!(claude(&root).unwrap(), Outcome::Written(path.clone()));
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["other"]["command"], "other");
        assert_eq!(config["mcpServers"]["rawr"]["args"][0], "mcp");
        assert_eq!(config["extra"], 1);
        assert_eq!(claude(&root).unwrap(), Outcome::Registered(path));
    }

    #[test]
    fn codex_appends_once_preserving_other_tables() {
        let root = sandbox("codex-fresh");
        let path = root.join(CODEX_CONFIG);
        assert_eq!(codex(&root).unwrap(), Outcome::Written(path.clone()));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[mcp_servers.rawr]\ncommand = \"rawr\"\nargs = [\"mcp\"]\n"
        );
        assert_eq!(codex(&root).unwrap(), Outcome::Registered(path));

        let root = sandbox("codex-merge");
        let path = root.join(CODEX_CONFIG);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[mcp_servers.other]\ncommand = \"other\"").unwrap();
        assert_eq!(codex(&root).unwrap(), Outcome::Written(path.clone()));
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "[mcp_servers.other]\ncommand = \"other\"\n\n[mcp_servers.rawr]\ncommand = \"rawr\"\nargs = [\"mcp\"]\n"
        );
        assert_eq!(codex(&root).unwrap(), Outcome::Registered(path.clone()));
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
}
