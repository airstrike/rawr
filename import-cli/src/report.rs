//! Diagnostics of the runner's owner.

use rawr::diagnostic;

/// The report of a package that did not load or check, or one saying its
/// exact evidence could not be kept.
#[must_use]
pub fn package(error: &rawr::package::Error, fallback: diagnostic::Source) -> diagnostic::Report {
    error.report().unwrap_or_else(|report_error| {
        report(
            diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            format!("cannot retain exact package diagnostic evidence: {report_error}"),
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
