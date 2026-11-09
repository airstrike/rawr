//! Rawr's copy of a program's crate, as a message quotes it.
//!
//! A program with marked files builds from a copy rawr lays out in its
//! directory: `<target>/rawr/crate/`, `<target>/rawr/members/<path>/crate/`
//! for a workspace member, and `bin/` beside either for a package's native
//! binary that runs from its library. In the copy, a marked type native
//! code holds as a trait object has a stand-in named `__Rawr<Name>`. What
//! rustc or Cargo says of the copy names the program's own files and
//! types once [`program`] maps it.

/// The prefix of a generated stand-in's name.
const STAND_IN: &str = "__Rawr";

/// `text` with each path into rawr's copy as the program's own path, as
/// Cargo writes it (relative to the workspace root), and each stand-in's
/// name as the marked type's.
#[must_use]
pub fn program(text: &str) -> String {
    names(&paths(text))
}

fn paths(text: &str) -> String {
    let mut mapped = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let end = rest.find(is_boundary).unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        match program_path(word) {
            Some(path) => mapped.push_str(&path),
            None => mapped.push_str(word),
        }
        let Some(boundary) = after.chars().next() else {
            return mapped;
        };
        mapped.push(boundary);
        rest = &after[boundary.len_utf8()..];
    }
}

fn is_boundary(character: char) -> bool {
    character.is_whitespace() || matches!(character, '`' | '\'' | '"' | '(' | ')')
}

/// A path into the copy as the program's, or `None` for another path.
fn program_path(path: &str) -> Option<String> {
    path.match_indices("/rawr/")
        .find_map(|(at, marker)| copied(&path[at + marker.len()..]))
}

/// The program's path of `rest`, a path below rawr's directory, when it is
/// in the copy.
fn copied(rest: &str) -> Option<String> {
    let (member, rest) = match rest.strip_prefix("members/") {
        Some(member) => {
            let split = member.find("/crate/").or_else(|| member.find("/bin/"))?;
            (&member[..=split], &member[split + 1..])
        }
        None => ("", rest),
    };
    let file = rest
        .strip_prefix("crate/")
        .or_else(|| rest.strip_prefix("bin/"))?;
    Some(format!("{member}{file}"))
}

fn names(text: &str) -> String {
    let mut mapped = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(STAND_IN) {
        let after = &rest[at + STAND_IN.len()..];
        let starts_word = rest[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_alphanumeric() && before != '_');
        let names_type = after.chars().next().is_some_and(char::is_uppercase);
        mapped.push_str(&rest[..at]);
        if !(starts_word && names_type) {
            mapped.push_str(STAND_IN);
        }
        rest = after;
    }
    mapped.push_str(rest);
    mapped
}

/// A Cargo command on rawr's copy of a program that failed with rustc's or
/// Cargo's errors: what it said, which `cargo check` of the program says
/// too unless rawr's copy is wrong.
#[derive(Debug)]
pub struct Failure {
    stderr: Box<str>,
}

impl Failure {
    /// The failure `error` is, or `error` back for any other.
    pub fn of(error: crate::Error) -> Result<Self, crate::Error> {
        match said(&error) {
            Some(stderr) => Ok(Self {
                stderr: stderr.into(),
            }),
            None => Err(error),
        }
    }
}

/// What Cargo said when `error` is its failure with errors, also when a
/// cached build product carries it.
fn said(error: &crate::Error) -> Option<&str> {
    match error {
        crate::Error::Process {
            program: "cargo",
            stderr,
            ..
        } if !errors(stderr).is_empty() => Some(stderr),
        crate::Error::Node { source, .. } => match source.as_ref() {
            crate::node::Error::Importer(error) => said(error),
            _ => None,
        },
        _ => None,
    }
}

/// One error rustc gave for the copy: its headline, the first place it
/// names, and the trampoline any place it names is in.
struct Said {
    headline: String,
    location: Option<String>,
    trampoline: Option<Trampoline>,
}

impl std::fmt::Display for Said {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.headline)?;
        match (&self.trampoline, &self.location) {
            (Some(trampoline), _) => write!(formatter, "\n{trampoline}"),
            (None, Some(location)) => write!(formatter, "\n{location}"),
            (None, None) => Ok(()),
        }
    }
}

/// Each error in `stderr` as the program's, with the file under it: once
/// each, as a stand-in's error is its marked type's, and without Cargo's
/// summary. A line of the copy is no line of the program's, so no line is
/// named, but for an error in a trampoline, which names the marked
/// function's signature.
fn errors(stderr: &str) -> Vec<Said> {
    let mut errors: Vec<Said> = Vec::new();
    let mut lines = stderr.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("error") || line.starts_with("error: could not compile") {
            continue;
        }
        let headline = program(line);
        let mut location = None;
        let mut trampoline = None;
        while let Some(next) = lines.next_if(|next| !next.starts_with("error")) {
            let Some(place) = place(next) else {
                continue;
            };
            if location.is_none() && next.trim_start().starts_with("-->") {
                let file = place.trim_end_matches(|character: char| {
                    character.is_ascii_digit() || character == ':'
                });
                location = Some(format!("  --> {}", program(file)));
            }
            if trampoline.is_none() {
                trampoline = Trampoline::at(place);
            }
        }
        if !errors.iter().any(|seen| seen.headline == headline) {
            errors.push(Said {
                headline,
                location,
                trampoline,
            });
        }
    }
    errors
}

/// The place a `--> path:line:column` or `::: path:line:column` line of
/// rustc's names.
fn place(line: &str) -> Option<&str> {
    let line = line.trim_start();
    line.strip_prefix("-->")
        .or_else(|| line.strip_prefix(":::"))
        .map(str::trim)
}

impl Failure {
    /// The trampoline the first error that is in one is in, as rustc's
    /// errors name the marked function's signature.
    #[must_use]
    pub fn trampoline(&self) -> Option<Trampoline> {
        errors(&self.stderr)
            .into_iter()
            .find_map(|said| said.trampoline)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "rawr could not build the program; unless `cargo check` fails too, this is a bug in rawr",
        )?;
        for error in errors(&self.stderr) {
            write!(formatter, "\n{error}")?;
        }
        Ok(())
    }
}

/// The comment a marked file's generated copy starts with, before the
/// marked file's path in backticks.
const GENERATED: &str = "// Generated by Rawr from `";

/// Where an error in a function rawr generated for native code to call a
/// marked function, its trampoline, is in the program: the marked
/// function's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trampoline {
    /// The marked function's path from the crate root, as the interpreter
    /// finds it: `workspace::inbox::State::mark_all_read`.
    item: Box<str>,
    /// The marked file.
    file: Box<std::path::Path>,
    /// Its text when the error was read.
    text: Box<str>,
    /// The signature's line and column in it, from 1.
    line: usize,
    column: usize,
    /// The generated file and the line of the error in it.
    generated: Box<str>,
}

impl Trampoline {
    /// The trampoline `place` is in, a path into rawr's copy and its line
    /// and column as rustc writes them, read from the copy and the marked
    /// file on disk: `None` for a place in no trampoline.
    fn at(place: &str) -> Option<Self> {
        let mut parts = place.rsplitn(3, ':');
        let _column = parts.next()?;
        let line = parts.next()?.parse::<usize>().ok()?;
        let path = parts.next()?;
        program_path(path)?;
        let copy = std::fs::read_to_string(path).ok()?;
        let file = copy
            .lines()
            .next()?
            .strip_prefix(GENERATED)?
            .split('`')
            .next()?;
        let lines = copy.lines().collect::<Vec<_>>();
        let start = (0..line.min(lines.len()))
            .rev()
            .find(|at| signature(lines[*at]).is_some())?;
        let name = signature(lines[start])?;
        let item = lines[start + 1..]
            .iter()
            .take_while(|line| signature(line).is_none())
            .find_map(|line| called(line))?;
        let text = std::fs::read_to_string(file).ok()?;
        let (at, column) = declared(&text, item, name)?;
        Some(Self {
            item: item.into(),
            file: std::path::Path::new(file).into(),
            text: text.into(),
            line: at,
            column,
            generated: format!("{path}:{line}").into(),
        })
    }

    /// The marked file.
    #[must_use]
    pub fn file(&self) -> &std::path::Path {
        &self.file
    }

    /// The marked file's text the signature's place is in.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The signature's line and column, from 1.
    #[must_use]
    pub const fn place(&self) -> (usize, usize) {
        (self.line, self.column)
    }

    /// The signature's length on its line, in characters.
    #[must_use]
    pub fn width(&self) -> usize {
        self.text
            .lines()
            .nth(self.line - 1)
            .map_or(1, |line| line.trim().chars().count())
    }

    /// What rustc's error is in, said as a note.
    #[must_use]
    pub fn note(&self) -> String {
        format!(
            "in the code rawr generated for native code to call `{}`, at {}",
            self.item, self.generated
        )
    }
}

impl std::fmt::Display for Trampoline {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "  --> {}:{}:{}\n   = note: {}",
            self.file.display(),
            self.line,
            self.column,
            self.note()
        )
    }
}

/// The name of the function `line` starts to declare: `None` for any other
/// line.
fn signature(line: &str) -> Option<&str> {
    let mut words = line.split_whitespace();
    let rest = loop {
        match words.next()? {
            "fn" => break words.next()?,
            word if word.starts_with("pub") || matches!(word, "const" | "unsafe" | "async") => {}
            _ => return None,
        }
    };
    let end = rest
        .find(|character: char| !(character.is_alphanumeric() || character == '_'))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// The marked function a trampoline's `line` calls, when it starts the
/// call.
fn called(line: &str) -> Option<&str> {
    let (_, after) = line.split_once("__rawr_island::Call::new(\"")?;
    after.split('"').next()
}

/// The line and column, from 1, of the declaration of `name` in `text`
/// that `item`, its path, names: in an `impl` of its owner when the path
/// has one.
fn declared(text: &str, item: &str, name: &str) -> Option<(usize, usize)> {
    let owner = item.rsplit("::").nth(1);
    let mut within = None;
    let mut first = None;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("impl") {
            within = Some(trimmed);
        }
        if signature(line) != Some(name) {
            continue;
        }
        let at = (index + 1, line.len() - trimmed.len() + 1);
        let owned = within.zip(owner).is_some_and(|(header, owner)| {
            header
                .split(|character: char| !(character.is_alphanumeric() || character == '_'))
                .any(|word| word == owner)
        });
        if owned {
            return Some(at);
        }
        first.get_or_insert(at);
    }
    first
}

impl std::error::Error for Failure {}

#[cfg(test)]
mod tests {
    const STDERR: &str = "    Checking app v0.1.0 (/p/app/target/rawr/crate)\n\
warning: unused macro definition: `give`\n   --> /p/app/target/rawr/crate/src/main.rs:265:18\n\n\
error[E0277]: `BadgePage` doesn't implement `Debug`\n  --> /p/app/target/rawr/crate/src/badge.rs:22:15\n   |\n\
22 | impl Page for BadgePage {\n\n\
error[E0277]: `__RawrBadgePage` doesn't implement `Debug`\n  --> /p/app/target/rawr/crate/src/badge.rs:35:15\n   |\n\
35 | impl Page for crate::badge::__RawrBadgePage {\n\n\
error: could not compile `app` (lib) due to 2 previous errors; 1 warning emitted\n";

    #[test]
    fn a_copy_path_is_the_program_s() {
        assert_eq!(
            super::program("  --> /p/app/target/rawr/crate/src/badge.rs:22:15"),
            "  --> src/badge.rs:22:15"
        );
        assert_eq!(
            super::program("::: /p/ws/target/rawr/members/examples/app/crate/src/a.rs:1:2"),
            "::: examples/app/src/a.rs:1:2"
        );
        assert_eq!(
            super::program("(/p/app/target/rawr/bin/src/main.rs)"),
            "(src/main.rs)"
        );
        assert_eq!(
            super::program("/p/app/target/rawr/crate/src/rawr/a.rs"),
            "src/rawr/a.rs"
        );
        assert_eq!(
            super::program("/home/x/.cargo/rawr/hosts/1/src/lib.rs"),
            "/home/x/.cargo/rawr/hosts/1/src/lib.rs"
        );
    }

    #[test]
    fn a_stand_in_is_the_marked_type() {
        assert_eq!(
            super::program("`crate::badge::__RawrBadgePage` and __rawr_island"),
            "`crate::badge::BadgePage` and __rawr_island"
        );
    }

    #[test]
    fn a_failure_names_each_error_once_at_the_program_s_path() {
        let error = crate::Error::Process {
            program: "cargo",
            status: "exit status: 101".into(),
            stderr: STDERR.into(),
        };
        let error = crate::Error::Node {
            operation: "authoritative Cargo build product",
            source: Box::new(crate::node::Error::Importer(Box::new(error))),
        };
        let failure = super::Failure::of(error).unwrap().to_string();
        assert_eq!(
            failure,
            "rawr could not build the program; unless `cargo check` fails too, this is a bug in rawr\n\
             error[E0277]: `BadgePage` doesn't implement `Debug`\n  --> src/badge.rs"
        );
        let spawn = crate::Error::Process {
            program: "cargo",
            status: "failed to spawn".into(),
            stderr: "No such file or directory (os error 2)".into(),
        };
        assert!(super::Failure::of(spawn).is_err());
    }

    /// A marked file and rawr's copy of it, in which the trampoline of
    /// `State::mark_all_read` fails as rustc failed inboard's.
    const MARKED: &str = "// rawr\npub struct Id(String);\n\npub struct Other;\n\nimpl Other {\n    \
        pub fn mark_all_read(&mut self) {}\n}\n\npub struct State {\n    ids: Vec<Id>,\n}\n\n\
        impl State {\n    pub fn mark_all_read(&mut self, ids: impl IntoIterator<Item = Id>) {\n        \
        self.ids.extend(ids);\n    }\n}\n";

    fn copy(marked: &std::path::Path) -> String {
        format!(
            "// Generated by Rawr from `{}`, which starts with `// rawr`: what native code\n\
             // sees of it. Each function hands its arguments to the interpreter.\n\
             impl State {{\n    \
             pub fn mark_all_read(&mut self, __a0: impl IntoIterator<Item = Id>) {{\n        \
             let mut __call = crate::__rawr_island::Call::new(\"workspace::inbox::State::mark_all_read\", true);\n        \
             __call.unit();\n        \
             crate::__rawr_island::give!(__call, __a0);\n    \
             }}\n}}\n",
            marked.display()
        )
    }

    #[test]
    fn an_error_in_a_trampoline_names_the_marked_function_s_signature() {
        let root =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
                .join(format!("rawr-trampoline-{}", std::process::id()));
        let marked = root.join("src/workspace/inbox.rs");
        let copied = root.join("target/rawr/crate/src/workspace/inbox.rs");
        std::fs::create_dir_all(copied.parent().unwrap()).unwrap();
        std::fs::create_dir_all(marked.parent().unwrap()).unwrap();
        std::fs::write(&marked, MARKED).unwrap();
        std::fs::write(&copied, copy(&marked)).unwrap();
        let main = root.join("target/rawr/crate/src/main.rs");
        let stderr = format!(
            "error[E0310]: the parameter type `impl IntoIterator<Item = Id>` may not live long enough\n    \
             --> {main}:554:13\n     |\n 554 |             (&hand(&value)).hand(&mut $call, value, traits);\n     |\n    \
             ::: {copied}:7:9\n     |\n   7 |         crate::__rawr_island::give!(__call, __a0);\n     |\n\
             help: consider adding an explicit lifetime bound\n    -->  {copied}:4:71\n\n\
             error: could not compile `app` (lib) due to 1 previous error\n",
            main = main.display(),
            copied = copied.display(),
        );
        let error = crate::Error::Process {
            program: "cargo",
            status: "exit status: 101".into(),
            stderr: stderr.into(),
        };
        let failure = super::Failure::of(error).unwrap();
        let trampoline = failure.trampoline().unwrap();
        assert_eq!(trampoline.place(), (15, 5));
        assert_eq!(trampoline.file(), marked);
        assert_eq!(
            failure.to_string(),
            format!(
                "rawr could not build the program; unless `cargo check` fails too, this is a bug in rawr\n\
                 error[E0310]: the parameter type `impl IntoIterator<Item = Id>` may not live long enough\n  \
                 --> {}:15:5\n   \
                 = note: in the code rawr generated for native code to call \
                 `workspace::inbox::State::mark_all_read`, at {}:7",
                marked.display(),
                copied.display()
            )
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
