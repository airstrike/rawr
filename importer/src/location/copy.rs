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

/// Each error in `stderr` as the program's, with the file under it: once
/// each, as a stand-in's error is its marked type's, and without Cargo's
/// summary. A line of the copy is no line of the program's, so no line is
/// named.
fn errors(stderr: &str) -> Vec<String> {
    let mut errors: Vec<(String, Option<String>)> = Vec::new();
    let mut lines = stderr.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("error") || line.starts_with("error: could not compile") {
            continue;
        }
        let headline = program(line);
        let mut location = None;
        while let Some(next) = lines.next_if(|next| !next.starts_with("error")) {
            if next.trim_start().starts_with("--> ") {
                let file = next.trim_end_matches(|character: char| {
                    character.is_ascii_digit() || character == ':'
                });
                location = Some(program(file));
                break;
            }
        }
        if !errors.iter().any(|(seen, _)| *seen == headline) {
            errors.push((headline, location));
        }
    }
    errors
        .into_iter()
        .map(|(headline, location)| match location {
            Some(location) => format!("{headline}\n{location}"),
            None => headline,
        })
        .collect()
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
}
