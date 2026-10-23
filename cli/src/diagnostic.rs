//! Concise human rendering for canonical diagnostic reports.

use rawr::diagnostic::Report;

/// Render one canonical report for terminal output.
#[must_use]
pub fn render(report: &Report) -> String {
    rawr::diagnostic::render(report)
}

#[cfg(test)]
mod tests {
    use rawr::diagnostic::{code, Report, Source};
    use rawr::{Position, Span};

    #[test]
    fn cli_uses_the_core_canonical_report_rendering() {
        let source = Source::new("src/main.rs", "alpha\n").unwrap();
        let report = Report::error(code::PARSE_SYNTAX, "invalid source")
            .primary(
                source,
                Span::new(Position::new(1, 1), Position::new(1, 6)),
                Some("parse failed".into()),
            )
            .unwrap()
            .finish();

        assert_eq!(super::render(&report), rawr::diagnostic::render(&report));
    }
}
