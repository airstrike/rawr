//! One thing rawr says a save did.

use std::fmt;

use crate::json;

/// What a save did to the running program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// This process started again because a save restarted it. A process
    /// hears it first, if at all.
    Restarted { reason: String },
    /// A save compiled; the program takes it at its next call from native
    /// code into script code, and then [`Outcome::Swapped`] follows.
    Queued,
    /// The program runs the saved code now, with its state kept.
    Swapped,
    /// A save failed; the program keeps running the last good code.
    Failed { message: String, at: Option<Span> },
    /// A save can't swap into this process, for `reason`: rawr ends it and
    /// starts the program again.
    Restarting { reason: String },
}

/// Where a failure points: a file and its 1-based line and column.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Restarted { reason } => write!(f, "restarted: {reason}"),
            Self::Queued => f.write_str("queued"),
            Self::Swapped => f.write_str("swapped"),
            Self::Failed {
                message,
                at: Some(at),
            } => write!(f, "failed at {at}: {message}"),
            Self::Failed { message, at: None } => write!(f, "failed: {message}"),
            Self::Restarting { reason } => write!(f, "restarting: {reason}"),
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

impl Outcome {
    /// The outcome one line of the stream gives, or `None` for an event
    /// that is no outcome or a line that is no JSON object.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let object = json::Object::parse(line)?;
        let text = |name| object.text(name).map(str::to_owned);
        Some(match object.text("event")? {
            "restarted" => Self::Restarted {
                reason: text("reason")?,
            },
            "queued" => Self::Queued,
            "swapped" => Self::Swapped,
            "failed" => Self::Failed {
                message: text("message").unwrap_or_default(),
                at: text("file").and_then(|file| {
                    Some(Span {
                        file,
                        line: object.number("line")?,
                        column: object.number("column")?,
                    })
                }),
            },
            "restarting" => Self::Restarting {
                reason: text("reason")?,
            },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Outcome, Span};

    #[test]
    fn each_outcome_parses_from_its_line() {
        assert_eq!(
            Outcome::parse(r#"{"event":"queued"}"#),
            Some(Outcome::Queued)
        );
        assert_eq!(
            Outcome::parse("{\"event\":\"swapped\"}\n"),
            Some(Outcome::Swapped)
        );
        assert_eq!(
            Outcome::parse(r#"{"event":"restarting","reason":"`Scale` changed its fields"}"#),
            Some(Outcome::Restarting {
                reason: "`Scale` changed its fields".to_owned()
            })
        );
        assert_eq!(
            Outcome::parse(r#"{"event":"restarted","reason":"`main` changed"}"#),
            Some(Outcome::Restarted {
                reason: "`main` changed".to_owned()
            })
        );
    }

    #[test]
    fn a_failure_keeps_its_message_and_position_past_nested_diagnostics() {
        let line = r#"{"column":20,"diagnostics":[{"file":"/p/src/main.rs","line":10,"help":null,"nested":{"a":[1,2.5e3,true,false]}}],"event":"failed","file":"/p/src/main.rs","line":10,"message":"cannot find value `x` in this scope","rendered":"error[E0425]: \"x\"\n  --> \\path é 🦀"}"#;
        assert_eq!(
            Outcome::parse(line),
            Some(Outcome::Failed {
                message: "cannot find value `x` in this scope".to_owned(),
                at: Some(Span {
                    file: "/p/src/main.rs".to_owned(),
                    line: 10,
                    column: 20,
                }),
            })
        );
    }

    #[test]
    fn a_failure_without_a_position_has_none() {
        let line = r#"{"event":"failed","message":"the runner stopped","file":null,"line":null,"column":null}"#;
        let outcome = Outcome::parse(line).unwrap();
        assert_eq!(
            outcome,
            Outcome::Failed {
                message: "the runner stopped".to_owned(),
                at: None,
            }
        );
        assert_eq!(outcome.to_string(), "failed: the runner stopped");
    }

    #[test]
    fn rawrs_own_events_and_broken_lines_are_no_outcome() {
        for line in [
            r#"{"event":"running"}"#,
            r#"{"event":"building","stage":"build","percent":40}"#,
            r#"{"event":"ended","status":0}"#,
            r#"{"event":"restarting"}"#,
            r#"{"event":"swapped""#,
            r#"["event","swapped"]"#,
            "",
            "swapped",
        ] {
            assert_eq!(Outcome::parse(line), None, "{line}");
        }
    }

    #[test]
    fn an_outcome_displays_as_one_line() {
        let failed = Outcome::Failed {
            message: "mismatched types".to_owned(),
            at: Some(Span {
                file: "src/main.rs".to_owned(),
                line: 3,
                column: 7,
            }),
        };
        assert_eq!(
            failed.to_string(),
            "failed at src/main.rs:3:7: mismatched types"
        );
        assert_eq!(
            Outcome::Restarted {
                reason: "`main` changed".to_owned()
            }
            .to_string(),
            "restarted: `main` changed"
        );
    }
}
