//! Snippets that start with a frontmatter block: cargo scripts, which the
//! bot runs as `rawr run play.rs` runs one. Their dependencies are built,
//! bindings are generated for the calls the snippet makes, and the snippet
//! runs on the runner built for it.
//!
//! Every dependency a frontmatter names must be an approved crate
//! ([`crate::approved`]). A snippet that names anything else is refused
//! before anything is built. The build is rawr-import's script path: it
//! expands the frontmatter as cargo does, keeps the script's manifest and
//! lockfile in rawr's home, and builds every script in the one build
//! directory there (`scripts/build`), so a crate one snippet compiled is
//! fresh for the next. It runs offline (`CARGO_NET_OFFLINE`), against the
//! approved crates the image vendored, so it fetches nothing.
//!
//! The runner is a universe runner built for the script
//! (`RAWR_UNIVERSE_RUNNER`), which rawr-import writes out and doesn't
//! start. The snippet then runs on it as a snippet runs on the prebuilt
//! runner ([`crate::universe`]): in a fork of its server, reaching no file
//! and no environment, under the bot's time and memory limits. The build
//! has none of those limits: it runs within the container's own bounds.
//!
//! A frontmatter that declares no dependency needs no build. Its snippet
//! runs where every other snippet runs, with the block blanked.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rawr::project::frontmatter;
use toml_edit::{DocumentMut, Item, TableLike};

use crate::approved::{self, Approved, Refusal};
use crate::plain::Output;
use crate::slot::Slot;
use crate::universe::{self, Snippet, Universe};
use crate::{Limits, PlayError, SOURCE_NAME};

/// The variable naming the `rawr-import` a script's build runs;
/// `rawr-import` on the `PATH` when unset.
pub const IMPORT: &str = "PLAY_IMPORT";

/// The variable naming the directory scripts are written to, in their
/// slots; `rawr-play-scripts` in the temporary directory when unset.
pub const SCRIPTS: &str = "PLAY_SCRIPTS";

/// The variables a script's build reads from `rawr-play`'s environment,
/// which the bot passes on to each run: the toolchain's and rawr's homes,
/// and where the approved crates, `rawr-import` and the scripts are.
pub const ENV: [&str; 10] = [
    "PATH",
    "HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "RAWR_HOME",
    "TMPDIR",
    approved::DIRECTORY,
    IMPORT,
    SCRIPTS,
];

/// The variables of [`ENV`] that rawr-import itself reads.
const BUILD_ENV: [&str; 6] = [
    "PATH",
    "HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "TMPDIR",
];

/// The dependency tables of a manifest, and of each of its targets.
const DEPENDENCIES: [&str; 5] = [
    "dependencies",
    "dev-dependencies",
    "dev_dependencies",
    "build-dependencies",
    "build_dependencies",
];

/// A snippet with a frontmatter block that reads as cargo reads one.
#[derive(Debug)]
pub struct Script<'source> {
    source: &'source str,
    /// The block's manifest.
    manifest: DocumentMut,
    /// Where the Rust source starts: after the block, and after the marker
    /// line when the snippet starts with one.
    start: usize,
    /// Whether the snippet marks itself as Rawr code.
    marked: bool,
}

/// What builds a script's runner: rawr-import, rawr's home, where the
/// scripts are written, and the toolchain's variables.
#[derive(Clone, Debug)]
pub struct Builder {
    import: PathBuf,
    home: PathBuf,
    scripts: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl<'source> Script<'source> {
    /// `source` as a cargo script, when it has a frontmatter block: a
    /// script whose block and manifest read as cargo reads them, or cargo's
    /// report on them, at the place in `play.rs` cargo points.
    pub fn new(source: &'source str) -> Option<Result<Self, PlayError>> {
        let lead = match frontmatter::read(source) {
            Ok(lead) => lead,
            Err(malformed) if malformed.opens() => {
                let text = crate::plain::parse_report(source)
                    .map(|text| rawr_snippet::report(&text, source, source))
                    .unwrap_or_else(|| format!("error: {malformed}"));
                return Some(Err(PlayError::Compile(text.trim_end().to_owned())));
            }
            Err(_) => return None,
        };
        let block = lead.frontmatter?;
        let marked = rawr::project::marker::is_marked(source);
        let start = if marked {
            source[lead.source..]
                .find('\n')
                .map_or(source.len(), |end| lead.source + end + 1)
        } else {
            lead.source
        };
        Some(manifest(source, &block).map(|manifest| Self {
            source,
            manifest,
            start,
            marked,
        }))
    }

    /// Whether its frontmatter declares a dependency, which its run builds.
    #[must_use]
    pub fn imports(&self) -> bool {
        let table = self.manifest.as_table();
        let targets = table
            .get("target")
            .and_then(Item::as_table_like)
            .into_iter()
            .flat_map(TableLike::iter)
            .filter_map(|(_, target)| target.as_table_like());
        std::iter::once(table as &dyn TableLike)
            .chain(targets)
            .flat_map(|table| DEPENDENCIES.iter().filter_map(|key| table.get(key)))
            .any(|dependencies| {
                dependencies
                    .as_table_like()
                    .is_some_and(|table| !table.is_empty())
            })
    }

    /// The snippet with its frontmatter block and what comes before it
    /// blanked: a plain snippet, whose lines and columns are the script's.
    #[must_use]
    pub fn blanked(&self) -> String {
        let (lead, rest) = self.source.split_at(self.start);
        let mut blanked = lead
            .chars()
            .map(|character| if character == '\n' { '\n' } else { ' ' })
            .collect::<String>();
        blanked.push_str(rest);
        blanked
    }

    /// Build its runner and run it there, once its dependencies are found
    /// approved: what it printed, and the frame an iced program drew.
    pub fn run(&self, limits: &Limits) -> Result<Output, PlayError> {
        let approved = match Approved::from_env() {
            Some(Ok(approved)) => approved,
            Some(Err(error)) => {
                return Err(PlayError::Compile(format!(
                    "?play could not read its approved crates: {error}"
                )));
            }
            None => {
                let refusal = Refusal::Crate {
                    approved: Vec::new(),
                };
                return Err(PlayError::Refused(refusal.to_string()));
            }
        };
        if let Some(refusal) = approved.refusal(&self.manifest) {
            return Err(PlayError::Refused(refusal.to_string()));
        }
        let snippet = self.snippet(&approved);
        // A parse needs no crate: a syntax error is reported unbuilt.
        if let Some(text) = crate::plain::parse_report(snippet.program()) {
            let report = snippet.report(&text, Path::new(SOURCE_NAME));
            return Err(PlayError::Compile(report.trim_end().to_owned()));
        }
        let builder = Builder::from_env();
        let slot = Slot::take(&builder.scripts).map_err(|error| {
            PlayError::Compile(format!("?play could not write the snippet: {error}"))
        })?;
        let runner = builder.build(&slot, &snippet)?;
        Universe::script(slot.script(), runner, builder.home.clone()).run_snippet(&snippet, limits)
    }

    /// The snippet as its runner compiles it: the frontmatter block with
    /// each approved dependency pinned to its approved features
    /// ([`Approved::pin`]), then the marker unless the snippet has one, then
    /// the rest, wrapped as a plain snippet is. A report counts as the
    /// snippet does once the lines the pinning and the marker added are
    /// taken out.
    fn snippet(&self, approved: &Approved) -> Snippet<'source> {
        let (lead, rest) = self.source.split_at(self.start);
        // The pinned manifest in place of the block's, the fences kept. The
        // block read before, so it reads again.
        let block = frontmatter::read(lead)
            .ok()
            .and_then(|lead| lead.frontmatter)
            .map(|block| block.manifest);
        let pinned = block
            .and_then(|range| {
                let mut manifest = lead[range.clone()].parse::<DocumentMut>().ok()?;
                approved.pin(&mut manifest);
                Some(format!(
                    "{}{manifest}{}",
                    &lead[..range.start],
                    &lead[range.end..]
                ))
            })
            .unwrap_or_else(|| lead.to_owned());
        let mut program = pinned;
        let mut wrapped = lead.to_owned();
        for text in [&mut program, &mut wrapped] {
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
        let lines = |text: &str| text.matches('\n').count();
        let mut count = lines(&program).saturating_sub(lines(&wrapped));
        if !self.marked {
            program.push_str(universe::MARKER);
            count += 1;
        }
        let body = rawr_snippet::plain::program(rest);
        program.push_str(&body);
        let first = lines(&wrapped) + 1;
        wrapped.push_str(&body);
        let added = (count > 0).then_some(universe::Added { first, count });
        Snippet::script(program, wrapped, added, self.source)
    }
}

impl Builder {
    /// The build the environment describes: `PLAY_IMPORT`, rawr's home as
    /// rawr finds it, `PLAY_SCRIPTS`, and the toolchain's variables.
    #[must_use]
    pub fn from_env() -> Self {
        let variable = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let import = variable(IMPORT).map_or_else(|| PathBuf::from("rawr-import"), PathBuf::from);
        // rawr's home, as rawr finds it.
        let home = variable("RAWR_HOME").map(PathBuf::from).unwrap_or_else(|| {
            variable("CARGO_HOME")
                .map(PathBuf::from)
                .or_else(|| variable("HOME").map(|home| Path::new(&home).join(".cargo")))
                .unwrap_or_else(|| PathBuf::from(".cargo"))
                .join("rawr")
        });
        let home = std::path::absolute(&home).unwrap_or(home);
        let scripts = variable(SCRIPTS).map_or_else(
            || std::env::temp_dir().join("rawr-play-scripts"),
            PathBuf::from,
        );
        let env = BUILD_ENV
            .iter()
            .filter_map(|name| Some((OsString::from(name), variable(name)?)))
            .collect();
        Self {
            import,
            home,
            scripts,
            env,
        }
    }

    /// Write `snippet` to `slot`'s script and build its runner there, as
    /// rawr-import builds a universe runner: the runner's path, or cargo's,
    /// rustc's or rawr's report on the script, as `play.rs` shows it.
    fn build(&self, slot: &Slot, snippet: &Snippet<'_>) -> Result<PathBuf, PlayError> {
        let script = slot.script();
        let out = slot.out();
        let unwritable = |error: std::io::Error| {
            PlayError::Compile(format!("?play could not write the snippet: {error}"))
        };
        std::fs::write(&script, snippet.program()).map_err(unwritable)?;
        // A runner left from the slot's last build is never this one's.
        match std::fs::remove_dir_all(&out) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(unwritable(error));
            }
            _ => {}
        }
        let built = Command::new(&self.import)
            .arg("run")
            .arg(&script)
            .env_clear()
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .env("RAWR_HOME", &self.home)
            .env("RAWR_UNIVERSE_RUNNER", "1")
            .env("RAWR_UNIVERSE_OUT", &out)
            // Nothing a snippet names is fetched: the approved crates are
            // the vendored ones the image's Cargo home names.
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_TERM_COLOR", "never")
            .env("NO_COLOR", "1")
            .current_dir(slot.directory())
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                PlayError::Compile(format!(
                    "?play could not start {}: {error}",
                    self.import.display()
                ))
            })?;
        let runner = out.join("runner");
        if built.status.success() && runner.is_file() {
            return Ok(runner);
        }
        let mut text = String::from_utf8_lossy(&built.stderr).into_owned();
        text.push_str(&String::from_utf8_lossy(&built.stdout));
        // rawr-import's progress lines are for whoever watches the build.
        let text = text
            .lines()
            .filter(|line| !line.starts_with("rawr > "))
            .collect::<Vec<_>>()
            .join("\n");
        let shown = snippet.report(&text, &script);
        // Cargo names the script by its path in the slot.
        let slot = format!("{}/", slot.directory().display());
        let shown = shown.replace(&slot, "");
        let shown = shown.trim();
        Err(PlayError::Compile(if shown.is_empty() {
            format!(
                "?play could not build the snippet's dependencies: {} exited with {}",
                self.import.display(),
                built.status
            )
        } else {
            shown.to_owned()
        }))
    }
}

/// Build every set of approved crates once, as a snippet that names those
/// crates builds them, in the build directory every script shares: what the
/// image runs. A snippet's crates have their approved features alone, so
/// each set builds the units a snippet that names it builds, the features
/// cargo unifies across them among them. The sets are as many as `2^n - 1`
/// for `n` approved crates, and they share what doesn't differ.
pub fn warm(approved: &Approved, builder: &Builder) -> Result<(), PlayError> {
    let declarations = approved.declarations().collect::<Vec<_>>();
    for set in 1_usize..(1 << declarations.len()) {
        let named = declarations
            .iter()
            .enumerate()
            .filter(|(index, _)| set & (1 << index) != 0)
            .map(|(_, declaration)| *declaration)
            .collect::<Vec<_>>();
        let source = format!(
            "---\n[dependencies]\n{}\n---\nfn main() {{}}\n",
            named.join("\n")
        );
        let script = match Script::new(&source) {
            Some(script) => script?,
            None => continue,
        };
        let slot = Slot::take(&builder.scripts).map_err(|error| {
            PlayError::Compile(format!("?play could not write the snippet: {error}"))
        })?;
        println!("warming {}", named.join(", "));
        builder.build(&slot, &script.snippet(approved))?;
    }
    Ok(())
}

/// Whether `source` is a cargo script whose frontmatter declares a
/// dependency: its run builds the dependencies first, within the
/// container's bounds, and only the snippet's own run has the memory limit.
#[must_use]
pub fn builds(source: &str) -> bool {
    matches!(Script::new(source), Some(Ok(script)) if script.imports())
}

/// The block's manifest, read as cargo reads it, or its report, at the
/// place in the snippet the manifest's error is.
fn manifest(source: &str, block: &frontmatter::Frontmatter) -> Result<DocumentMut, PlayError> {
    let range = block.manifest.clone();
    // What comes before the manifest blanked, so every span is the
    // snippet's own bytes.
    let mut padded = source.as_bytes()[..range.start]
        .iter()
        .map(|byte| if *byte == b'\n' { '\n' } else { ' ' })
        .collect::<String>();
    padded.push_str(&source[range]);
    padded.parse::<DocumentMut>().map_err(|error| {
        let span = error.span().unwrap_or_else(|| block.open.clone());
        PlayError::Compile(manifest_report(source, error.message(), span))
    })
}

/// A manifest error at the bytes `span` of `source`, rendered as rawr
/// renders a script's, with cargo's message.
fn manifest_report(source: &str, message: &str, span: std::ops::Range<usize>) -> String {
    let position = |byte: usize| {
        let before = &source[..byte.min(source.len())];
        let line = before.matches('\n').count() + 1;
        let column = before
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
            + 1;
        rawr::Position::new(
            u16::try_from(line).unwrap_or(u16::MAX),
            u16::try_from(column).unwrap_or(u16::MAX),
        )
    };
    let span = rawr::Span::new(position(span.start), position(span.end));
    let report = rawr::diagnostic::Source::new(SOURCE_NAME, source)
        .map_err(|_| ())
        .and_then(|evidence| {
            rawr::diagnostic::Report::error(
                rawr::diagnostic::code::ENVIRONMENT_PACKAGE_MANIFEST,
                message.to_owned(),
            )
            .primary(evidence, span, None)
            .map_err(drop)
        });
    match report {
        Ok(report) => rawr::diagnostic::render(&report.finish())
            .trim_end()
            .to_owned(),
        Err(()) => format!("error: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::Script;
    use crate::approved::{Approved, Refusal};
    use crate::PlayError;

    fn script(source: &str) -> Script<'_> {
        Script::new(source).expect("a script").expect("it reads")
    }

    fn approved() -> Approved {
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("approved");
        Approved::read(&directory).unwrap()
    }

    /// A snippet with a frontmatter block is a script, and one without is
    /// not.
    #[test]
    fn a_frontmatter_block_makes_a_snippet_a_script() {
        assert!(Script::new("fn main() {}\n").is_none());
        assert!(Script::new("1 - 2").is_none());
        let imports = script("---\n[dependencies]\ntokio = \"1\"\n---\nfn main() {}\n");
        assert!(imports.imports());
        assert!(super::builds(
            "---\n[dependencies]\ntokio = \"1\"\n---\nfn main() {}\n"
        ));
        let bare = script("---\n[package]\nedition = \"2021\"\n---\nprintln!(\"hi\");\n");
        assert!(!bare.imports());
        assert!(!super::builds("fn main() {}\n"));
        let targeted = script("---\n[target.'cfg(unix)'.dependencies]\ntokio = \"1\"\n---\n");
        assert!(targeted.imports());
    }

    /// libc is no approved crate, and tokio's `fs`, `net` and `process` are
    /// no approved features: both are refused before anything is built,
    /// with the line that names what is approved. A snippet within the
    /// approved features is admitted.
    #[test]
    fn libc_and_tokio_s_io_features_are_refused_and_the_approved_ones_admitted() {
        let approved = approved();
        let refusal = |manifest: &str| approved.refusal(&script(manifest).manifest);
        let Some(Refusal::Crate { approved: names }) =
            refusal("---\n[dependencies]\nlibc = \"0.2.172\"\n---\nfn main() {}\n")
        else {
            panic!("libc is refused");
        };
        assert!(
            names.iter().all(|name| !name.starts_with("libc")),
            "{names:?}"
        );
        for asked in [
            "tokio = { version = \"1\", features = [\"fs\", \"net\", \"process\"] }",
            "tokio = { version = \"1\", features = [\"rt\", \"net\"] }",
            "t = { package = \"tokio\", version = \"1\", features = [\"signal\"] }",
            "tokio = { version = \"1\", optional = true }\n[features]\nio = [\"tokio/fs\"]",
            "ratatui = { version = \"0.30\", default-features = true }",
            "ratatui = { version = \"0.30\", default-features = false, features = [\"crossterm\"] }",
        ] {
            let refused = refusal(&format!("---\n[dependencies]\n{asked}\n---\nfn main() {{}}\n"));
            assert!(matches!(refused, Some(Refusal::Feature { .. })), "{asked}: {refused:?}");
        }
        assert_eq!(
            refusal(
                "---\n[dependencies]\ntokio = { version = \"1\", features = [\"net\"] }\n---\n"
            )
            .unwrap()
            .to_string(),
            "?play builds tokio only with the features `macros`, `rt`, `rt-multi-thread`, `sync` \
             and `time`."
        );
        for asked in [
            "tokio = \"1\"",
            "tokio = { version = \"1.40\", features = [\"rt\", \"time\"] }",
            "ratatui = \"0.30\"",
            "ratatui = { version = \"0.30\", default-features = false, features = [\"std\"] }",
            "iced = { git = \"https://github.com/iced-rs/iced\", rev = \"be88873d38544455e26c7a104eb757da2cbbac6b\", features = [\"canvas\"] }",
        ] {
            let manifest = format!("---\n[dependencies]\n{asked}\n---\nfn main() {{}}\n");
            assert_eq!(refusal(&manifest), None, "{asked}");
        }
    }

    /// An approved dependency builds with its approved features alone,
    /// whatever subset the snippet asked for, so the warm build serves it;
    /// a report still counts the snippet's own lines.
    #[test]
    fn a_dependency_is_pinned_to_its_approved_features() {
        let source = "---\n[dependencies.tokio]\nversion = \"1\"\n---\n1 + 2\n";
        let snippet = script(source).snippet(&approved());
        let manifest = snippet.program().split("---\n").nth(1).unwrap();
        let pinned = manifest.parse::<toml_edit::DocumentMut>().unwrap();
        let features = pinned["dependencies"]["tokio"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|feature| feature.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            features,
            ["macros", "rt", "rt-multi-thread", "sync", "time"]
        );
        let ratatui = script("---\n[dependencies]\nratatui = \"0.30\"\n---\nfn main() {}\n")
            .snippet(&approved());
        assert!(
            ratatui.program().contains("default-features = false"),
            "{}",
            ratatui.program()
        );
        // The pinned table added a line before the marker's: the code's
        // `+` on the snippet's line 5 is the program's line 7.
        let line = snippet
            .program()
            .lines()
            .position(|line| line.contains("1 + 2"))
            .unwrap()
            + 1;
        let rendered = format!(
            "error: oops\n --> /scripts/0/play.rs:{line}:27\n  |\n{line} | fn main() {{ let __v = {{ 1 + 2\n  | {}^\n",
            " ".repeat(26)
        );
        assert_eq!(
            snippet.report(&rendered, "/scripts/0/play.rs".as_ref()),
            "error: oops\n --> play.rs:5:3\n  |\n5 | 1 + 2\n  |   ^\n"
        );
    }

    /// A malformed block or manifest is cargo's error, at the place in
    /// `play.rs` cargo points, before anything is built.
    #[test]
    fn a_bad_frontmatter_is_cargo_s_error() {
        let Some(Err(PlayError::Compile(unclosed))) =
            Script::new("---\n[dependencies]\ntokio = \"1\"\nfn main() {}\n")
        else {
            panic!("an unclosed block is an error");
        };
        assert!(
            unclosed.contains("unclosed frontmatter; expected `---`"),
            "{unclosed}"
        );
        assert!(unclosed.contains(" --> play.rs:"), "{unclosed}");
        let Some(Err(PlayError::Compile(toml))) =
            Script::new("---\n[dependencies\ntokio = \"1\"\n---\nfn main() {}\n")
        else {
            panic!("a manifest that isn't TOML is an error");
        };
        assert!(toml.contains("unclosed table, expected `]`"), "{toml}");
        assert!(toml.contains(" --> play.rs:2:"), "{toml}");
        assert!(toml.contains("2 | [dependencies"), "{toml}");
    }

    /// The runner compiles the block, a marker after it, and the rest
    /// wrapped; a report counts lines as the snippet does. A snippet marked
    /// already keeps its marker where it is.
    #[test]
    fn a_script_is_marked_after_its_block_and_wrapped() {
        let source = "---\n[dependencies]\ntokio = { version = \"1\", features = [\"macros\", \"rt\", \"rt-multi-thread\", \"sync\", \"time\"] }\n---\n1 + 2\n";
        let snippet = script(source).snippet(&approved());
        assert!(
            snippet
                .program()
                .contains("\n---\n// rawr\nfn main() { let __v = { 1 + 2\n"),
            "{}",
            snippet.program()
        );
        let rendered = format!(
            "error: oops\n --> /scripts/0/play.rs:6:27\n  |\n6 | fn main() {{ let __v = {{ 1 + 2\n  | {}^\n",
            " ".repeat(26)
        );
        assert_eq!(
            snippet.report(&rendered, "/scripts/0/play.rs".as_ref()),
            "error: oops\n --> play.rs:5:3\n  |\n5 | 1 + 2\n  |   ^\n"
        );
        let marked = script("---\n[dependencies]\ntokio = \"1\"\n---\n// rawr\nfn main() {}\n");
        assert!(
            marked
                .snippet(&approved())
                .program()
                .ends_with("---\n// rawr\nfn main() {}\n"),
            "{}",
            marked.snippet(&approved()).program()
        );
        let blanked = marked.blanked();
        assert_eq!(blanked.lines().count(), 6);
        assert!(
            blanked.lines().take(5).all(|line| line.trim().is_empty()),
            "{blanked:?}"
        );
        assert!(blanked.ends_with("\nfn main() {}\n"), "{blanked:?}");
    }

    /// A syntax error needs no build: it is reported as `play.rs` shows it.
    #[test]
    fn a_script_s_syntax_error_is_reported_unbuilt() {
        let source =
            "---\n[dependencies]\ntokio = \"1\"\n---\nfn main() {\n    let x = (1 + 2;\n}\n";
        let snippet = script(source).snippet(&approved());
        let text = crate::plain::parse_report(snippet.program()).expect("a syntax error");
        let shown = snippet.report(&text, "play.rs".as_ref());
        assert!(
            shown.contains("Expecting '}' to close a function body"),
            "{shown}"
        );
        assert!(shown.contains(" --> play.rs:7:3\n"), "{shown}");
        assert!(shown.contains("\n7 | }\n"), "{shown}");
    }
}
