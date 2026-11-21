//! The hover: the expression under the pointer and what the paused
//! program's hover read said of it, which runs none of the program's code.

use std::ops::Range;

use serde_json::Value;

/// What the pointer is over and what the hover shows there, or waits for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    /// The line, from 0, and the characters on it the hover covers: the
    /// expression read, or the character asked about until the answer
    /// comes.
    line: usize,
    range: Range<usize>,
    /// Which ask this is: an answer to an earlier one is of a place the
    /// pointer left.
    asked: u64,
    said: Said,
}

/// What the program said of the expression under the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Said {
    /// The answer has yet to come.
    Waiting,
    /// Its value, as `{:?}` prints it.
    Value { expression: String, value: String },
    /// Nothing there reads, or it is code a hover doesn't run: the hover
    /// shows nothing.
    Nothing,
}

impl Hover {
    /// The hover asked as `asked` about character `column` of line `line`,
    /// both from 0.
    #[must_use]
    pub const fn asked(line: usize, column: usize, asked: u64) -> Self {
        Self {
            line,
            range: column..column + 1,
            asked,
            said: Said::Waiting,
        }
    }

    /// Whether the hover covers character `column` of line `line`: a
    /// pointer moved there shows the same hover.
    #[must_use]
    pub fn covers(&self, line: usize, column: usize) -> bool {
        self.line == line && self.range.contains(&column)
    }

    #[must_use]
    pub const fn said(&self) -> &Said {
        &self.said
    }

    /// Take `answer`, the hover read's answer to ask `asked`, when it is
    /// this hover's: its `range` is where the expression read is written,
    /// from 1.
    pub fn answered(&mut self, asked: u64, answer: &Value) {
        if asked != self.asked {
            return;
        }
        let column = |key: &str| {
            answer["range"][key]
                .as_u64()
                .and_then(|column| usize::try_from(column).ok())
                .map(|column| column.saturating_sub(1))
        };
        if let (Some(start), Some(end)) = (column("start"), column("end"))
            && answer["range"]["line"].as_u64() == u64::try_from(self.line + 1).ok()
            && start < end
        {
            self.range = start..end;
        }
        let expression = answer["expression"].as_str().unwrap_or_default().to_owned();
        self.said = match answer["value"].as_str() {
            Some(value) => Said::Value {
                expression,
                value: value.to_owned(),
            },
            None => Said::Nothing,
        };
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Hover, Said};

    #[test]
    fn a_hover_takes_its_own_answer_and_covers_the_expression_read() {
        let mut hover = Hover::asked(4, 18, 2);
        let answer = json!({
            "expression": "player.score",
            "range": { "line": 5, "start": 9, "end": 21 },
            "value": "3",
        });
        hover.answered(1, &answer);
        assert_eq!(
            hover.said(),
            &Said::Waiting,
            "an earlier ask's answer is dropped"
        );
        assert!(!hover.covers(4, 9));
        hover.answered(2, &answer);
        assert_eq!(
            hover.said(),
            &Said::Value {
                expression: "player.score".to_owned(),
                value: "3".to_owned()
            }
        );
        assert!(hover.covers(4, 8) && hover.covers(4, 19) && !hover.covers(4, 20));
        assert!(!hover.covers(5, 10));
    }

    #[test]
    fn a_refused_hover_shows_nothing() {
        let mut hover = Hover::asked(4, 18, 1);
        let answer = json!({
            "expression": "player.reset()",
            "refused": "a hover doesn't run calls: evaluate it in the console",
        });
        hover.answered(1, &answer);
        assert_eq!(hover.said(), &Said::Nothing);
    }
}
