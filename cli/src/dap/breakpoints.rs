//! The breakpoints the client knows, each by the id the protocol gives it,
//! kept in step with the list the session tells (#2278). A newer list than
//! the client's tells it, as `breakpoint` events, the breakpoints another
//! shell set (`new`) and cleared (`removed`). While the client's own
//! `setBreakpoints` or `setFunctionBreakpoints` runs, the breakpoints it
//! names are its own: their answer gives their ids, and no event tells
//! them.

use serde_json::{json, Value};

/// A breakpoint as the program's inspector keeps it: a file's line as it
/// was set, or a function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Point {
    Line { file: String, line: u64 },
    Function(String),
}

/// What a newer list changed of the breakpoints the client knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    New { id: u64, point: Point },
    Removed { id: u64 },
}

/// The breakpoints the client knows, and the list they follow.
#[derive(Debug, Default)]
pub struct Known {
    /// The revision of the newest list followed.
    revision: u64,
    /// The last id given.
    given: u64,
    points: Vec<(Point, u64)>,
    /// The breakpoints the client's request in flight sets or clears.
    asked: Vec<Point>,
}

impl Point {
    /// A breakpoint as the inspector writes one: `{"file", "line"}` or
    /// `{"function"}`.
    #[must_use]
    pub fn from_json(spec: &Value) -> Option<Self> {
        if let Some(name) = spec["function"].as_str() {
            return Some(Self::Function(name.to_owned()));
        }
        Some(Self::Line {
            file: spec["file"].as_str()?.to_owned(),
            line: spec["line"].as_u64()?,
        })
    }
}

impl Known {
    /// The client's request for `asked` begins: the changes to those are
    /// its own until [`Self::answered`].
    pub fn ask(&mut self, asked: Vec<Point>) {
        self.asked = asked;
    }

    /// The client's request ends, its last answer the list `told`: the
    /// changes to tell the client, and the ids its own breakpoints have.
    pub fn answered(&mut self, told: Option<&Value>) -> Vec<Change> {
        let changes = told.map(|told| self.follow(told)).unwrap_or_default();
        self.asked.clear();
        changes
    }

    /// The id the client knows `point` by.
    #[must_use]
    pub fn id(&self, point: &Point) -> Option<u64> {
        self.points
            .iter()
            .find(|(known, _)| known == point)
            .map(|(_, id)| *id)
    }

    /// Follow `told`, `{"revision", "breakpoints": [..]}`, when it is newer
    /// than the list the client's breakpoints follow: the changes to tell
    /// the client, which leave out the breakpoints its own request names.
    pub fn follow(&mut self, told: &Value) -> Vec<Change> {
        let Some(revision) = told["revision"]
            .as_u64()
            .filter(|revision| *revision > self.revision)
        else {
            return Vec::new();
        };
        self.revision = revision;
        let now: Vec<Point> = told["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Point::from_json)
            .collect();
        let mut changes = Vec::new();
        let asked = &self.asked;
        self.points.retain(|(point, id)| {
            let kept = now.contains(point);
            if !kept && !asked.contains(point) {
                changes.push(Change::Removed { id: *id });
            }
            kept
        });
        for point in now {
            if self.id(&point).is_some() {
                continue;
            }
            self.given += 1;
            if !self.asked.contains(&point) {
                changes.push(Change::New {
                    id: self.given,
                    point: point.clone(),
                });
            }
            self.points.push((point, self.given));
        }
        changes
    }
}

impl Change {
    /// The protocol's `breakpoint` event body, with lines as the client
    /// counts them by `line_out`.
    pub fn body(&self, line_out: impl Fn(u64) -> u64) -> Value {
        match self {
            Self::New {
                id,
                point: Point::Line { file, line },
            } => json!({
                "reason": "new",
                "breakpoint": {
                    "id": id, "verified": true,
                    "source": { "path": file }, "line": line_out(*line),
                },
            }),
            Self::New {
                id,
                point: Point::Function(_),
            } => json!({ "reason": "new", "breakpoint": { "id": id, "verified": true } }),
            Self::Removed { id } => {
                json!({ "reason": "removed", "breakpoint": { "id": id, "verified": false } })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Change, Known, Point};

    fn line(line: u64) -> Point {
        Point::Line {
            file: "/p/src/main.rs".to_owned(),
            line,
        }
    }

    fn told(revision: u64, lines: &[u64]) -> serde_json::Value {
        let specs: Vec<_> = lines
            .iter()
            .map(|line| json!({ "file": "/p/src/main.rs", "line": line, "text": "" }))
            .collect();
        json!({ "event": "breakpoints", "revision": revision, "breakpoints": specs })
    }

    #[test]
    fn another_shells_breakpoints_are_told_and_the_clients_own_are_not() {
        let mut known = Known::default();
        // The client sets line 3; the clear and the break its request makes
        // are told on the way, then answered.
        known.ask(vec![line(3)]);
        assert_eq!(known.follow(&told(1, &[])), []);
        assert_eq!(known.follow(&told(2, &[3])), []);
        assert_eq!(known.answered(Some(&told(2, &[3]))), []);
        let own = known
            .id(&line(3))
            .expect("the client's breakpoint has an id");

        // Another shell sets line 7, then clears it.
        let changes = known.follow(&told(3, &[3, 7]));
        let [Change::New { id, point }] = changes.as_slice() else {
            panic!("{changes:?}");
        };
        assert_eq!(point, &line(7));
        assert_ne!(*id, own);
        assert_eq!(known.follow(&told(4, &[3])), [Change::Removed { id: *id }]);
        // A list older than the newest followed changes nothing.
        assert_eq!(known.follow(&told(3, &[3, 7])), []);

        // The client sets lines 3 and 5: its line 3 goes and comes back
        // within its request, and nothing tells either.
        known.ask(vec![line(3), line(5)]);
        assert_eq!(known.follow(&told(5, &[])), []);
        assert_eq!(known.answered(Some(&told(7, &[3, 5]))), []);
        assert!(known.id(&line(5)).is_some());
        assert_eq!(
            Change::Removed { id: own }.body(|line| line)["reason"],
            "removed"
        );
    }
}
