//! Rawr's copy of a program's crate, as a message quotes it.
//!
//! A program with marked files builds from a copy rawr lays out in its
//! directory: `<target>/rawr/crate/`, `<target>/rawr/members/<path>/crate/`
//! for a workspace member (each under `<target>/rawr/projects/<name>-<key>/`
//! when the target directory is shared), and `bin/` beside either for a
//! package's native binary that runs from its library. In the copy, a marked type native
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
    // A shared target directory keeps each workspace's under
    // `projects/<name>-<key>/`.
    let rest = match rest.strip_prefix(rawr_location::PROJECTS) {
        Some(keyed) => keyed.strip_prefix('/')?.split_once('/')?.1,
        None => rest,
    };
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

/// One error rustc or Cargo gave for the copy: its headline, the first
/// place it names, the generated code any place it names is in, and the
/// causes Cargo gave under it.
struct Said {
    headline: String,
    location: Option<String>,
    generated: Option<Generated>,
    causes: Vec<String>,
}

impl std::fmt::Display for Said {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.headline)?;
        match (&self.generated, &self.location) {
            (Some(generated), _) => write!(formatter, "\n{generated}")?,
            (None, Some(location)) => write!(formatter, "\n{location}")?,
            (None, None) => {}
        }
        for cause in &self.causes {
            write!(formatter, "\n\nCaused by:\n{cause}")?;
        }
        Ok(())
    }
}

/// The note a failed runner build's error gets where its place in an
/// adapter rawr generated is mapped to the program's call that needed the
/// adapter, before the adapter's own place.
pub(crate) const CALL_SITE: &str = "in the code rawr generated for this call, at ";

/// Each error in `stderr` as the program's, with the file under it and
/// the causes Cargo gave: once each, as a stand-in's error is its marked
/// type's, and without Cargo's summary. A line of the copy is no line of
/// the program's, so no line is named, but for an error in code rawr
/// generated for a marked item, which names the item's declaration, and
/// for one in an adapter, which names the program's call.
fn errors(stderr: &str) -> Vec<Said> {
    let mut errors: Vec<Said> = Vec::new();
    let mut lines = stderr.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("error") || line.starts_with("error: could not compile") {
            continue;
        }
        let headline = program(line);
        let mut location = None;
        let mut generated = None;
        let mut called = None;
        let mut causes: Vec<String> = Vec::new();
        while let Some(next) = lines.next_if(|next| !next.starts_with("error")) {
            if next == "Caused by:" {
                causes.push(String::new());
                continue;
            }
            if let Some(cause) = causes.last_mut() {
                if next.starts_with(char::is_whitespace) && !next.trim().is_empty() {
                    if !cause.is_empty() {
                        cause.push('\n');
                    }
                    cause.push_str(&program(next));
                }
                continue;
            }
            if let Some(adapter) = next
                .trim_start()
                .strip_prefix("= note: ")
                .and_then(|note| note.strip_prefix(CALL_SITE))
            {
                if generated.is_none() {
                    generated = called.and_then(|place| Generated::called_at(place, adapter));
                }
                continue;
            }
            let Some(place) = place(next) else {
                continue;
            };
            if location.is_none() && next.trim_start().starts_with("-->") {
                let file = place.trim_end_matches(|character: char| {
                    character.is_ascii_digit() || character == ':'
                });
                // A place in the copy names no line of the program's; any
                // other place is where rustc says.
                let shown = if program_path(file).is_some() {
                    program(file)
                } else {
                    place.to_owned()
                };
                location = Some(format!("  --> {shown}"));
                called = Some(place);
            }
            if generated.is_none() {
                generated = Generated::at(place);
            }
        }
        causes.retain(|cause| !cause.is_empty());
        if !errors.iter().any(|seen| seen.headline == headline) {
            errors.push(Said {
                headline,
                location,
                generated,
                causes,
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
    /// The generated code the first error that is in some is in, as
    /// rustc's errors name the marked item's declaration.
    #[must_use]
    pub fn generated(&self) -> Option<Generated> {
        errors(&self.stderr)
            .into_iter()
            .find_map(|said| said.generated)
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

/// Where an error in code rawr generated for a marked item is in the
/// program: the marked function's signature for its trampoline, the
/// function rawr generated for native code to call it, and the marked
/// type's name for an impl that carries the type between native code and
/// the interpreter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generated {
    /// What rawr generated the code for.
    purpose: Purpose,
    /// The marked file.
    file: Box<std::path::Path>,
    /// Its text when the error was read.
    text: Box<str>,
    /// The line and column in it, from 1, of what rustc's error is at in
    /// the declaration, as the trait `impl IntoView` names, or of the
    /// declaration, and its width in characters.
    line: usize,
    column: usize,
    width: usize,
    /// The generated file and the line of the error in it.
    generated: Box<str>,
}

/// What rawr generated the code an error is in for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Purpose {
    /// A trampoline: native code calls the marked function at this path
    /// from the crate root, as the interpreter finds it
    /// (`screen::list::State::clear`).
    Call(Box<str>),
    /// An impl of the island's trait (`Put`, `Take`, `Parts` or `Mirror`)
    /// for the marked type, as the copy writes it.
    Crossing { traits: Box<str>, ty: Box<str> },
    /// An adapter of a runner, which the program's call at the place
    /// needed.
    Adapter,
}

/// The traits of the island's support whose impls for a marked type carry
/// its values between native code and the interpreter.
const CROSSING: [&str; 4] = ["Put", "Take", "Parts", "Mirror"];

/// A place in a marked file's generated copy: the copy's lines, and the
/// place's line and column there, from 1.
struct Copied<'c> {
    lines: Vec<&'c str>,
    line: usize,
    column: usize,
}

impl Generated {
    /// The generated code `place` is in, a path into rawr's copy and its
    /// line and column as rustc writes them, read from the copy and the
    /// marked file on disk: `None` for a place in no code rawr generated
    /// for a marked item.
    fn at(place: &str) -> Option<Self> {
        let mut parts = place.rsplitn(3, ':');
        let column = parts.next()?.parse::<usize>().ok()?;
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
        let copied = Copied {
            lines: copy.lines().collect(),
            line,
            column,
        };
        let text = std::fs::read_to_string(file).ok()?;
        let (purpose, (at, column, width)) = copied
            .trampoline(&text)
            .or_else(|| copied.crossing(&text))?;
        Some(Self {
            purpose,
            file: std::path::Path::new(file).into(),
            text: text.into(),
            line: at,
            column,
            width,
            generated: format!("{path}:{line}").into(),
        })
    }

    /// The marked file.
    #[must_use]
    pub fn file(&self) -> &std::path::Path {
        &self.file
    }

    /// The marked file's text the declaration's place is in.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The line and column, from 1, of what the error is at in the
    /// declaration.
    #[must_use]
    pub const fn place(&self) -> (usize, usize) {
        (self.line, self.column)
    }

    /// The width of what the error is at, in characters.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// What rustc's error is in, said as a note.
    #[must_use]
    pub fn note(&self) -> String {
        match &self.purpose {
            Purpose::Call(item) => format!(
                "in the code rawr generated for native code to call `{item}`, at {}",
                self.generated
            ),
            Purpose::Crossing { traits, ty } => format!(
                "in the `{traits}` impl rawr generated to carry `{ty}` between native code \
                 and the interpreter, at {}",
                self.generated
            ),
            Purpose::Adapter => format!("{CALL_SITE}{}", self.generated),
        }
    }

    /// The program's call at `place`, `file:line:column` as rustc writes
    /// it, that needed the adapter at `adapter`, read from the program's
    /// file on disk.
    fn called_at(place: &str, adapter: &str) -> Option<Self> {
        let mut parts = place.rsplitn(3, ':');
        let column = parts.next()?.parse::<usize>().ok()?;
        let line = parts.next()?.parse::<usize>().ok()?;
        let file = parts.next()?;
        let text = std::fs::read_to_string(file).ok()?;
        let written = text.lines().nth(line.checked_sub(1)?)?;
        let at = written.char_indices().nth(column.checked_sub(1)?)?.0;
        let width = words(written)
            .find(|(start, word)| (*start..start + word.len()).contains(&at))
            .map_or(1, |(_, word)| word.chars().count());
        Some(Self {
            purpose: Purpose::Adapter,
            file: std::path::Path::new(file).into(),
            text: text.into(),
            line,
            column,
            width,
            generated: adapter.into(),
        })
    }
}

impl Copied<'_> {
    /// The marked function whose trampoline the place is in, and the place
    /// in `text`, the marked file, of the word rustc's error is at in its
    /// signature, or of the signature.
    fn trampoline(&self, text: &str) -> Option<(Purpose, (usize, usize, usize))> {
        let lines = &self.lines;
        let start = (0..self.line.min(lines.len()))
            .rev()
            .find(|at| signature(lines[*at]).is_some())?;
        let name = signature(lines[start])?;
        let item = lines[start + 1..]
            .iter()
            .take_while(|line| signature(line).is_none())
            .find_map(|line| called(line))?;
        let (at, start_column) = declared(text, item, name)?;
        // The word rustc's error is at, as the trait an `impl Trait`
        // names, is at the same word of the marked signature, which
        // differs from the copy's only in its parameters' names.
        let place = lines
            .get(start..self.line)
            .and_then(|signature| named(signature, self.column))
            .and_then(|(word, seen)| found(text, at, word, seen))
            .unwrap_or_else(|| {
                let width = text
                    .lines()
                    .nth(at - 1)
                    .map_or(1, |line| line.trim().chars().count());
                (at, start_column, width)
            });
        Some((Purpose::Call(item.into()), place))
    }

    /// The crossing impl the place is in, and the place in `text`, the
    /// marked file, of its type's name where the type is declared.
    fn crossing(&self, text: &str) -> Option<(Purpose, (usize, usize, usize))> {
        let header = self.lines[..self.line.min(self.lines.len())]
            .iter()
            .rev()
            .find(|line| line.starts_with("impl"))?;
        let (traits, ty) = crossed(header)?;
        let name = ty.split('<').next()?.trim();
        let (line, column) = declared_type(text, name)?;
        let purpose = Purpose::Crossing {
            traits: traits.into(),
            ty: ty.into(),
        };
        Some((purpose, (line, column, name.chars().count())))
    }
}

impl std::fmt::Display for Generated {
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

/// The island's trait and the type an impl `header` of the copy is of,
/// when it is a crossing impl: `impl<T> crate::__rawr_island::Put for
/// Ball<T> {` is `Put` for `Ball<T>`.
fn crossed(header: &str) -> Option<(&str, &str)> {
    let (_, after) = header.split_once("__rawr_island::")?;
    let (traits, ty) = after.split_once(" for ")?;
    if !CROSSING.contains(&traits) {
        return None;
    }
    Some((traits, ty.trim_end().trim_end_matches('{').trim_end()))
}

/// The line and column, from 1, of the name `name` where `text` declares a
/// struct, enum or union of it.
fn declared_type(text: &str, name: &str) -> Option<(usize, usize)> {
    text.lines().enumerate().find_map(|(index, line)| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("pub") && !trimmed.starts_with(['s', 'e', 'u']) {
            return None;
        }
        let mut seen = words(line);
        while let Some((_, word)) = seen.next() {
            if !matches!(word, "struct" | "enum" | "union") {
                continue;
            }
            let (start, declared) = seen.next()?;
            if declared != name {
                return None;
            }
            return Some((index + 1, line[..start].chars().count() + 1));
        }
        None
    })
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
/// call or says why native code can't call it yet.
fn called(line: &str) -> Option<&str> {
    let (_, after) = line
        .split_once("__rawr_island::call(\"")
        .or_else(|| line.split_once("__rawr_island::uncalled(\""))
        .or_else(|| line.split_once("native code can't call `"))?;
    after.split(['"', '`']).next()
}

fn is_identifier(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Each identifier of `text`, with its byte offset.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.char_indices()
        .filter(move |(at, character)| {
            is_identifier(*character)
                && text[..*at]
                    .chars()
                    .next_back()
                    .is_none_or(|before| !is_identifier(before))
        })
        .map(move |(at, _)| {
            let end = text[at..]
                .find(|character: char| !is_identifier(character))
                .map_or(text.len(), |end| at + end);
            (at, &text[at..end])
        })
}

/// The identifier at `column`, from 1, of `signature`'s last line, and
/// how many times the signature names it before: `None` when none is
/// there, it is a parameter the copy renamed (`__a0`), or the place is in
/// the body, after the first `{`.
fn named<'s>(signature: &[&'s str], column: usize) -> Option<(&'s str, usize)> {
    let (last, before) = signature.split_last()?;
    let at = last.char_indices().nth(column.checked_sub(1)?)?.0;
    if before.iter().any(|line| line.contains('{')) || last[..at].contains('{') {
        return None;
    }
    let (target, word) =
        words(last).find(|(start, word)| (*start..start + word.len()).contains(&at))?;
    if word.starts_with("__") {
        return None;
    }
    let earlier = before
        .iter()
        .flat_map(|line| words(line))
        .chain(words(last).take_while(|(start, _)| *start < target))
        .filter(|(_, seen)| *seen == word)
        .count();
    Some((word, earlier))
}

/// The line and column, from 1, and the width of the identifier `word` the
/// signature in `text` starting on `line` names after `seen` others: its
/// lines up to the body's `{`.
fn found(text: &str, line: usize, word: &str, seen: usize) -> Option<(usize, usize, usize)> {
    let mut left = seen;
    for (index, written) in text.lines().enumerate().skip(line - 1) {
        let signature = written.split('{').next().unwrap_or(written);
        for (start, named) in words(signature) {
            if named != word {
                continue;
            }
            if left == 0 {
                let column = written[..start].chars().count() + 1;
                return Some((index + 1, column, word.chars().count()));
            }
            left -= 1;
        }
        if written.contains('{') {
            return None;
        }
    }
    None
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
        // A target directory several workspaces share.
        assert_eq!(
            super::program("--> /p/_target/rawr/projects/app-00ab/crate/src/a.rs:1:2"),
            "--> src/a.rs:1:2"
        );
        assert_eq!(
            super::program("/p/_target/rawr/projects/ws-00ab/members/tools/one/bin/src/main.rs"),
            "tools/one/src/main.rs"
        );
        assert_eq!(
            super::program("/p/_target/rawr/projects/app-00ab/store/x.rs"),
            "/p/_target/rawr/projects/app-00ab/store/x.rs"
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
             let mut __call = crate::__rawr_island::call(\"workspace::inbox::State::mark_all_read\", true);\n        \
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
        let trampoline = failure.generated().unwrap();
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

    /// An error in the signature of a function native code can't call
    /// yet, whose copy panics, names the word it is at in the marked
    /// signature, as rustc's E0405 named inboard's `impl IntoContent`.
    #[test]
    fn an_error_in_a_refused_signature_names_its_word_in_the_marked_one() {
        let root =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
                .join(format!("rawr-refused-{}", std::process::id()));
        let marked = root.join("src/workspace/home.rs");
        let copied = root.join("target/rawr/crate/src/workspace/home.rs");
        std::fs::create_dir_all(copied.parent().unwrap()).unwrap();
        std::fs::create_dir_all(marked.parent().unwrap()).unwrap();
        let signature = "    pub fn lines(mut self, lines: impl IntoLines) -> Self {";
        std::fs::write(
            &marked,
            format!(
                "// rawr\npub struct Panel;\n\nimpl Panel {{\n{signature}\n        self\n    }}\n}}\n"
            ),
        )
        .unwrap();
        let generated = "    pub fn lines(mut self, __a0: impl IntoLines) -> Self {";
        std::fs::write(
            &copied,
            format!(
                "// Generated by Rawr from `{}`, which starts with `// rawr`: what native code\n\
                 // sees of it. Each function hands its arguments to the interpreter.\n\
                 impl Panel {{\n{generated}\n        \
                 ::core::panic!(\"rawr: native code can't call `workspace::home::Panel::lines` \
                 yet: parameter `lines`: `impl IntoLines` can't cross yet\")\n    }}\n}}\n",
                marked.display()
            ),
        )
        .unwrap();
        let column = generated.find("IntoLines").unwrap() + 1;
        let stderr = format!(
            "error[E0405]: cannot find trait `IntoLines` in this scope\n --> {}:4:{column}\n  |\n\n\
             error: could not compile `app` (bin \"app\") due to 1 previous error\n",
            copied.display()
        );
        let error = crate::Error::Process {
            program: "cargo",
            status: "exit status: 101".into(),
            stderr: stderr.into(),
        };
        let failure = super::Failure::of(error).unwrap();
        let trampoline = failure.generated().unwrap();
        assert_eq!(
            trampoline.place(),
            (5, signature.find("IntoLines").unwrap() + 1)
        );
        assert_eq!(trampoline.width(), "IntoLines".len());
        assert_eq!(trampoline.file(), marked);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Cargo's failure to resolve a dependency keeps the causes it gave
    /// under it, so a network failure reads as one.
    #[test]
    fn a_cargo_failure_keeps_its_causes() {
        let stderr =
            "error: failed to get `ahash` as a dependency of package `app v0.1.0 (/p/app)`\n\n\
Caused by:\n  failed to load source for dependency `ahash`\n\n\
Caused by:\n  Unable to update registry `crates-io`\n\n\
Caused by:\n  [6] Couldn't resolve host name (Could not resolve host: index.crates.io)\n";
        let error = crate::Error::Process {
            program: "cargo",
            status: "exit status: 101".into(),
            stderr: stderr.into(),
        };
        assert_eq!(
            super::Failure::of(error).unwrap().to_string(),
            "rawr could not build the program; unless `cargo check` fails too, this is a bug in rawr\n\
             error: failed to get `ahash` as a dependency of package `app v0.1.0 (/p/app)`\n\n\
             Caused by:\n  failed to load source for dependency `ahash`\n\n\
             Caused by:\n  Unable to update registry `crates-io`\n\n\
             Caused by:\n  [6] Couldn't resolve host name (Could not resolve host: index.crates.io)"
        );
    }

    /// An error in a marked type's generated `Take` impl names the type
    /// where the marked file declares it.
    #[test]
    fn an_error_in_a_crossing_impl_names_the_marked_type() {
        let root =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
                .join(format!("rawr-crossing-{}", std::process::id()));
        let marked = root.join("src/physics.rs");
        let copied = root.join("target/rawr/crate/src/physics.rs");
        std::fs::create_dir_all(copied.parent().unwrap()).unwrap();
        std::fs::create_dir_all(marked.parent().unwrap()).unwrap();
        std::fs::write(
            &marked,
            "// rawr\nuse std::rc::Rc;\n\npub struct Ball<T> {\n    pub weight: Rc<T>,\n}\n",
        )
        .unwrap();
        std::fs::write(
            &copied,
            format!(
                "// Generated by Rawr from `{}`, which starts with `// rawr`: what native code\n\
                 // sees of it. Each function hands its arguments to the interpreter.\n\
                 pub struct Ball<T> {{\n    pub weight: Rc<T>,\n}}\n\
                 impl<T> crate::__rawr_island::Take for Ball<T> {{\n    #[inline]\n    \
                 fn take(input: &mut crate::__rawr_island::In<'_>) -> Self {{\n        \
                 input.structure(\"physics::Ball\", 1);\n        \
                 Ball {{\n            weight: crate::__rawr_island::Take::take(input),\n        \
                 }}\n    }}\n}}\n",
                marked.display()
            ),
        )
        .unwrap();
        let stderr = format!(
            "error[E0277]: the trait bound `Rc<T>: Take` is not satisfied\n  --> {}:11:21\n   |\n\n\
             error: could not compile `app` (lib) due to 1 previous error\n",
            copied.display()
        );
        let error = crate::Error::Process {
            program: "cargo",
            status: "exit status: 101".into(),
            stderr: stderr.into(),
        };
        let failure = super::Failure::of(error).unwrap();
        let generated = failure.generated().unwrap();
        assert_eq!(generated.place(), (4, 12));
        assert_eq!(generated.width(), "Ball".len());
        assert_eq!(generated.file(), marked);
        assert_eq!(
            generated.note(),
            format!(
                "in the `Take` impl rawr generated to carry `Ball<T>` between native code and \
                 the interpreter, at {}:11",
                copied.display()
            )
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A runner's error mapped from an adapter to the program's call
    /// names the call, and the adapter in its note.
    #[test]
    fn an_error_mapped_from_an_adapter_names_the_program_s_call() {
        let root =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
                .join(format!("rawr-adapter-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let program = root.join("home.rs");
        std::fs::write(
            &program,
            "// rawr\nfn view() {\n    row().spacing(\"zero\");\n}\n",
        )
        .unwrap();
        let adapter = "src/__rawr_program/rawr_adapters_iced_8.rs:4:50";
        let stderr = format!(
            "error[E0277]: the trait bound `Pixels: From<&str>` is not satisfied\n   \
             --> {}:3:11\n    = note: {}{adapter}\n    |\n\n\
             error: could not compile `rawr-generated-runner` (bin) due to 1 previous error\n",
            program.display(),
            super::CALL_SITE
        );
        let error = crate::Error::Process {
            program: "cargo",
            status: "exit status: 101".into(),
            stderr: stderr.into(),
        };
        let failure = super::Failure::of(error).unwrap();
        let generated = failure.generated().unwrap();
        assert_eq!(generated.place(), (3, 11));
        assert_eq!(generated.width(), "spacing".len());
        assert_eq!(generated.file(), program);
        assert_eq!(
            failure.to_string(),
            format!(
                "rawr could not build the program; unless `cargo check` fails too, this is a bug in rawr\n\
                 error[E0277]: the trait bound `Pixels: From<&str>` is not satisfied\n  \
                 --> {}:3:11\n   \
                 = note: in the code rawr generated for this call, at {adapter}",
                program.display()
            )
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
