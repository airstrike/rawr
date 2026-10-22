//! The binaries that serve programs `rawr` hands off: executables named
//! `rawr-<name>` next to `rawr` or on `PATH`, found as Cargo finds its
//! subcommands. Each says what it serves when run with [`FLAG`]: a small TOML
//! manifest of the crates and `rawr.toml` tables that bring a program to it,
//! and the commands it adds to `rawr`. `rawr` keeps each binary's manifest,
//! keyed by its path, size and modification time, so a binary is asked once
//! per install.
//!
//! `rawr` knows nothing of any one binary. [`CATALOG`] lists the binaries
//! that exist, as data, so a program or command needing one that is not
//! installed can say how to install it.

use std::ffi::OsString;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rawr::layer::{Claims, Facts};
use rawr::diagnostic;
use toml_edit::{DocumentMut, Item, Table};

/// The argument that asks a binary for its manifest.
pub const FLAG: &str = "--rawr-manifest";

/// The manifest format this `rawr` reads.
pub const FORMAT: i64 = 1;

/// The binaries that exist, with how to install each.
pub const CATALOG: &str = include_str!("catalog.toml");

/// What a binary serves, from its manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    /// Its name: `rawr-<name>` is the executable.
    pub name: String,
    /// What brings a program to it.
    pub claims: Claims,
    /// The commands it adds to `rawr`.
    pub commands: Vec<Provided>,
}

/// A command a binary adds to `rawr`: a first word, and a flag that must
/// follow it when the command shares its word with one `rawr` runs itself
/// (`check --as-rust`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provided {
    /// The command's first word.
    pub word: String,
    /// A flag the arguments must hold.
    pub flag: Option<String>,
}

impl std::fmt::Display for Provided {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.flag {
            Some(flag) => write!(formatter, "rawr {} {flag}", self.word),
            None => write!(formatter, "rawr {}", self.word),
        }
    }
}

/// An installed binary and its manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Installed {
    /// The executable.
    pub path: PathBuf,
    /// What it serves.
    pub manifest: Manifest,
}

/// A binary of [`CATALOG`]: its manifest and how to install it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Known {
    /// What it serves.
    pub manifest: Manifest,
    /// The commands that install it, one per line.
    pub install: Vec<String>,
}

impl Manifest {
    /// Read a manifest. `None` when it is not one of [`FORMAT`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let document = text.parse::<DocumentMut>().ok()?;
        Self::from_table(document.as_table())
    }

    fn from_table(table: &Table) -> Option<Self> {
        if table.get("format")?.as_integer()? != FORMAT {
            return None;
        }
        let strings = |key: &str| -> Option<Vec<String>> {
            match table.get(key) {
                None => Some(Vec::new()),
                Some(item) => item
                    .as_array()?
                    .iter()
                    .map(|value| value.as_str().map(str::to_owned))
                    .collect(),
            }
        };
        Some(Self {
            name: table.get("name")?.as_str()?.to_owned(),
            claims: Claims {
                crates: strings("crates")?,
                sections: strings("sections")?,
                prelude: strings("prelude")?,
            },
            commands: strings("commands")?
                .iter()
                .map(|command| {
                    let mut words = command.split_whitespace();
                    Provided {
                        word: words.next().unwrap_or_default().to_owned(),
                        flag: words.next().map(str::to_owned),
                    }
                })
                .collect(),
        })
    }

    /// The command `arguments` spell, if this binary provides it.
    #[must_use]
    pub fn provided(&self, arguments: &[OsString]) -> Option<&Provided> {
        let word = arguments.first()?;
        self.commands.iter().find(|command| {
            *word == *command.word
                && command.flag.as_ref().is_none_or(|flag| {
                    arguments[1..].iter().any(|argument| argument == flag.as_str())
                })
        })
    }

    /// Whether this binary provides the command `arguments` spell.
    #[must_use]
    pub fn provides(&self, arguments: &[OsString]) -> bool {
        self.provided(arguments).is_some()
    }

    /// Whether it provides `word` at all.
    #[must_use]
    pub fn provides_word(&self, word: &str) -> bool {
        self.commands.iter().any(|command| command.word == word)
    }
}

/// The binaries of [`CATALOG`].
#[must_use]
pub fn catalog() -> Vec<Known> {
    let document = CATALOG
        .parse::<DocumentMut>()
        .expect("the catalog is TOML");
    document
        .get("binary")
        .and_then(Item::as_array_of_tables)
        .expect("the catalog lists [[binary]] tables")
        .iter()
        .map(|table| Known {
            manifest: Manifest::from_table(table).expect("each catalog entry is a manifest"),
            install: table
                .get("install")
                .and_then(Item::as_array)
                .expect("each catalog entry says how to install it")
                .iter()
                .filter_map(|line| line.as_str().map(str::to_owned))
                .collect(),
        })
        .collect()
}

/// The binary among `installed` a program with `facts` goes to, if any.
/// Two binaries claiming one program is a diagnostic.
pub fn claimed<'a>(
    installed: &'a [Installed],
    facts: &Facts,
) -> Result<Option<&'a Installed>, diagnostic::Report> {
    let mut claimed = installed
        .iter()
        .filter(|binary| binary.manifest.claims.claim(facts));
    match (claimed.next(), claimed.next()) {
        (None, _) => Ok(None),
        (Some(binary), None) => Ok(Some(binary)),
        (Some(first), Some(second)) => Err(diagnostic::Report::error(
            diagnostic::code::FRAMEWORK_FACADE,
            format!(
                "this program uses {} and {}; {} serves the first and {} the second",
                first.manifest.claims.reason(facts).unwrap_or_default(),
                second.manifest.claims.reason(facts).unwrap_or_default(),
                first.manifest.name,
                second.manifest.name,
            ),
        )
        .help("a program can use only one of them")
        .finish()),
    }
}

/// The binaries installed next to the running `rawr` and on `PATH`, the
/// first of each name winning.
#[must_use]
pub fn installed() -> Vec<Installed> {
    let mut directories = Vec::new();
    if let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|executable| fs::canonicalize(executable).ok())
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
    {
        directories.push(directory);
    }
    if let Some(path) = std::env::var_os("PATH") {
        directories.extend(std::env::split_paths(&path));
    }
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    for directory in directories {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(stem) = name
                .strip_suffix(std::env::consts::EXE_SUFFIX)
                .and_then(|stem| stem.strip_prefix("rawr-"))
            else {
                continue;
            };
            if stem.is_empty()
                || stem.contains('.')
                || candidates.iter().any(|(known, _)| known == stem)
                || !is_executable(&entry.path())
            {
                continue;
            }
            candidates.push((stem.to_owned(), entry.path()));
        }
    }
    let mut store = Store::open();
    let installed = candidates
        .into_iter()
        .filter_map(|(_, path)| {
            store
                .manifest(&path)
                .map(|manifest| Installed { path, manifest })
        })
        .collect();
    store.close();
    installed
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The manifests `rawr` has asked for, in `<rawr home>/manifests.toml`:
/// one table per binary path holding its size, modification time and
/// manifest, empty for a binary that is not one `rawr` hands programs to.
struct Store {
    path: Option<PathBuf>,
    document: DocumentMut,
    changed: bool,
}

impl Store {
    fn open() -> Self {
        let path = home().map(|home| home.join("manifests.toml"));
        let document = path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| text.parse::<DocumentMut>().ok())
            .unwrap_or_default();
        Self {
            path,
            document,
            changed: false,
        }
    }

    fn manifest(&mut self, binary: &Path) -> Option<Manifest> {
        let metadata = fs::metadata(binary).ok()?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |time| time.as_nanos());
        let identity = format!("{}:{modified}", metadata.len());
        let key = binary.to_string_lossy().into_owned();
        let kept = self
            .document
            .get(&key)
            .and_then(Item::as_table)
            .filter(|entry| entry.get("identity").and_then(Item::as_str) == Some(identity.as_str()))
            .and_then(|entry| entry.get("manifest").and_then(Item::as_str).map(str::to_owned));
        let text = match kept {
            Some(text) => text,
            None => {
                let text = ask(binary)?.unwrap_or_default();
                let mut entry = Table::new();
                entry.insert("identity", toml_edit::value(identity));
                entry.insert("manifest", toml_edit::value(text.clone()));
                self.document.insert(&key, Item::Table(entry));
                self.changed = true;
                text
            }
        };
        Manifest::parse(&text)
    }

    fn close(self) {
        let (true, Some(path)) = (self.changed, self.path) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let partial = path.with_extension(format!("{}.partial", std::process::id()));
        if fs::write(&partial, self.document.to_string()).is_ok() {
            let _ = fs::rename(&partial, &path);
        }
    }
}

/// Rawr's home: `$RAWR_HOME`, else `$CARGO_HOME/rawr`, else `~/.cargo/rawr`.
fn home() -> Option<PathBuf> {
    std::env::var_os("RAWR_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CARGO_HOME").map(|home| Path::new(&home).join("rawr")))
        .or_else(|| {
            std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo").join("rawr"))
        })
}

/// Run `binary` with [`FLAG`] and return what it printed if it exited
/// successfully with a manifest of [`FORMAT`], else `Some(None)`. `None`
/// when it could not be started or did not finish, so nothing is kept
/// about it.
fn ask(binary: &Path) -> Option<Option<String>> {
    let mut attempts = 0;
    let mut child = loop {
        match Command::new(binary)
            .arg(FLAG)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => break child,
            // A binary being written (or held open by a process another
            // thread is starting) cannot run yet.
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 20 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    };
    // macOS assesses a new executable on its first run, which can take
    // seconds; a binary still running after this is asked again next time.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return Some(None),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    let mut text = String::new();
    if child
        .stdout
        .take()
        .is_none_or(|mut stdout| stdout.read_to_string(&mut text).is_err())
    {
        return Some(None);
    }
    Some(Manifest::parse(&text).map(|_| text))
}

/// The install commands as one indented block.
#[must_use]
pub fn install_lines(known: &Known) -> String {
    known
        .install
        .iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{claimed, Installed, Manifest};
    use std::ffi::OsString;

    fn installed(name: &str, crates: &[&str]) -> Installed {
        let crates = crates
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        Installed {
            path: format!("/bin/rawr-{name}").into(),
            manifest: Manifest::parse(&format!(
                "format = 1\nname = \"rawr-{name}\"\ncrates = [{crates}]\ncommands = [\"go\", \"check --deep\"]\n"
            ))
            .unwrap(),
        }
    }

    fn facts(source: &str) -> rawr::layer::Facts {
        let path = std::env::temp_dir().join(format!(
            "rawr-claimed-{}-{}.rawr",
            std::process::id(),
            source.len()
        ));
        std::fs::write(&path, source).unwrap();
        let target = rawr::project::Target::discover(&path).unwrap();
        let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target).unwrap();
        std::fs::remove_file(path).unwrap();
        rawr::layer::Facts::of(&graph)
    }

    #[test]
    fn a_program_goes_to_the_one_binary_claiming_it() {
        let both = [installed("one", &["alpha"]), installed("two", &["beta"])];
        let plain = facts("fn main() { println!(\"hi\"); }\n");
        assert_eq!(claimed(&both, &plain).unwrap(), None);
        let alpha = facts("fn main() { let x = alpha::thing(); }\n");
        assert_eq!(claimed(&both, &alpha).unwrap().unwrap().manifest.name, "rawr-one");
    }

    #[test]
    fn two_binaries_claiming_one_program_is_a_diagnostic() {
        let both = [installed("one", &["alpha"]), installed("two", &["beta"])];
        let mixed = facts("use alpha::a;\nfn main() { let x = beta::thing(); }\n");
        let report = claimed(&both, &mixed).unwrap_err();
        let rendered = rawr::diagnostic::render(&report);
        assert!(
            rendered.contains("this program uses `alpha` and `beta`; rawr-one serves the first and rawr-two the second"),
            "{rendered}"
        );
    }

    #[test]
    fn a_command_is_provided_by_its_word_and_flag() {
        let binary = installed("one", &[]);
        let arguments = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(binary.manifest.provides(&arguments(&["go", "x"])));
        assert!(binary.manifest.provides(&arguments(&["check", "--deep", "x"])));
        assert!(!binary.manifest.provides(&arguments(&["check", "x"])));
        assert!(!binary.manifest.provides(&arguments(&["stop"])));
        assert_eq!(Manifest::parse("format = 2\nname = \"x\"\n"), None);
    }

    #[test]
    fn the_catalog_reads() {
        for known in super::catalog() {
            assert!(!known.install.is_empty(), "{}", known.manifest.name);
        }
    }
}
