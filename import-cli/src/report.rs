//! Diagnostics of the runner's owner.

use std::path::Path;

use rawr::diagnostic;

/// The report of a package that did not load or check, or one saying its
/// exact evidence could not be kept.
#[must_use]
pub fn package(error: &rawr::package::Error, fallback: diagnostic::Source) -> diagnostic::Report {
    error.report().unwrap_or_else(|report_error| {
        report(
            diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            format!("cannot report the package's error: {report_error}"),
            Some(fallback),
        )
    })
}

/// An error report with `code` and `message`, against `source` when given.
#[must_use]
pub fn report(
    code: diagnostic::code::Code,
    message: impl std::fmt::Display,
    source: Option<diagnostic::Source>,
) -> diagnostic::Report {
    let builder = diagnostic::Report::error(code, message.to_string());
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}

/// What the runner's owner was doing for a program when its machinery
/// failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Task {
    /// Building the program's runner: its Rust dependencies and bindings.
    Build,
    /// Watching the program's files for saves.
    Watch,
}

impl Task {
    /// The headline of a failure while doing this for the program whose
    /// entry file is `file`, in plain words.
    #[must_use]
    pub fn headline(self, file: Option<&str>) -> String {
        match (self, file) {
            (Self::Build, Some(file)) => {
                format!(
                    "rawr could not build the Rust dependencies of `{}`",
                    shown(file)
                )
            }
            (Self::Build, None) => "rawr could not build the program's Rust dependencies".into(),
            (Self::Watch, Some(file)) => {
                format!(
                    "rawr could not watch the files of `{}` for saves",
                    shown(file)
                )
            }
            (Self::Watch, None) => "rawr could not watch the program's files for saves".into(),
        }
    }
}

/// A failure of rawr's machinery, as rustc reports a failure of its own:
/// what rawr was doing and for which file in the headline, against
/// `source` when given, with the failure itself in a note.
#[must_use]
pub fn failure(
    task: Task,
    error: impl std::fmt::Display,
    source: Option<diagnostic::Source>,
) -> diagnostic::Report {
    let headline = task.headline(source.as_ref().map(diagnostic::Source::identity));
    let builder = diagnostic::Report::error(diagnostic::code::FRAMEWORK_NATIVE, headline)
        .note(error.to_string());
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}

/// `file` as the user names it: from the current directory when it is
/// under it, as rustc and cargo show a path.
fn shown(file: &str) -> String {
    let path = Path::new(file);
    std::env::current_dir()
        .ok()
        .and_then(|directory| path.strip_prefix(directory).ok().map(Path::to_path_buf))
        .filter(|relative| !relative.as_os_str().is_empty())
        .map_or_else(
            || file.to_owned(),
            |relative| relative.display().to_string(),
        )
}

#[cfg(test)]
mod tests {
    use super::{failure, Task};
    use rawr::diagnostic;

    /// An internal failure reads as rustc's own: an `error:` headline that
    /// says what rawr was doing for which file, and the detail in a note.
    #[test]
    fn an_internal_failure_reads_as_rustc_s_own() {
        let source = diagnostic::Source::new("/project/src/main.rs", "fn main() {}\n").unwrap();
        let report = failure(
            Task::Build,
            "node store path has an unsupported shape: /home/.cargo/rawr/store/x",
            Some(source),
        );
        assert_eq!(
            diagnostic::render(&report),
            concat!(
                "error: rawr could not build the Rust dependencies of `/project/src/main.rs`\n",
                " --> /project/src/main.rs\n",
                "  = note: node store path has an unsupported shape: /home/.cargo/rawr/store/x\n",
            )
        );
        assert_eq!(report.code(), diagnostic::code::FRAMEWORK_NATIVE);
    }
}
