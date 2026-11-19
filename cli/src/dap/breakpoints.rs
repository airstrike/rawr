//! The breakpoints the client knows, each by the id the protocol gives it,
//! kept in step with the list the session tells (#2278). A newer list than
//! the client's tells it, as `breakpoint` events, the breakpoints another
//! shell set (`new`) and cleared (`removed`), and those a save moved to
//! another line (`changed`), which the list names by the same inspector
//! id. While the client's own `setBreakpoints` or `setFunctionBreakpoints`
//! runs, the breakpoints it names are its own: their answer gives their
//! ids, and no event tells them.

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
    New {
        id: u64,
        point: Point,
    },
    /// A breakpoint the client knows now on another line.
    Changed {
        id: u64,
        point: Point,
    },
    Removed {
        id: u64,
    },
}

/// A breakpoint the client knows: the inspector's id for it in the list,
/// where it is, and the protocol's id.
#[derive(Debug)]
struct Entry {
    listed: Option<u64>,
    point: Point,
    id: u64,
}

/// The breakpoints the client knows, and the list they follow.
#[derive(Debug, Default)]
pub struct Known {
    /// The revision of the newest list followed.
    revision: u64,
    /// The last id given.
    given: u64,
    entries: Vec<Entry>,
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
        self.entries
            .iter()
            .find(|entry| entry.point == *point)
            .map(|entry| entry.id)
    }

    /// Follow `told`, `{"revision", "breakpoints": [..]}`, when it is newer
    /// than the list the client's breakpoints follow: the changes to tell
    /// the client, which leave out the breakpoints its own request names.
    ///
    /// A breakpoint the client knows is the list's entry with the same
    /// inspector id, which a save that moved its line keeps, or else one
    /// on the same place, which a request set again.
    pub fn follow(&mut self, told: &Value) -> Vec<Change> {
        let Some(revision) = told["revision"]
            .as_u64()
            .filter(|revision| *revision > self.revision)
        else {
            return Vec::new();
        };
        self.revision = revision;
        let now: Vec<(Option<u64>, Point)> = told["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|listed| Some((listed["id"].as_u64(), Point::from_json(listed)?)))
            .collect();
        let mut found: Vec<Option<usize>> = self
            .entries
            .iter()
            .map(|entry| {
                let listed = entry.listed?;
                now.iter().position(|(id, _)| *id == Some(listed))
            })
            .collect();
        let mut taken = vec![false; now.len()];
        for index in found.iter().flatten() {
            taken[*index] = true;
        }
        for (slot, entry) in found.iter_mut().zip(&self.entries) {
            if slot.is_none() {
                *slot = (0..now.len()).find(|index| !taken[*index] && now[*index].1 == entry.point);
                if let Some(index) = *slot {
                    taken[index] = true;
                }
            }
        }
        let mut changes = Vec::new();
        let mut entries = Vec::with_capacity(now.len());
        for (entry, slot) in std::mem::take(&mut self.entries).into_iter().zip(found) {
            let Some(index) = slot else {
                if !self.asked.contains(&entry.point) {
                    changes.push(Change::Removed { id: entry.id });
                }
                continue;
            };
            let (listed, point) = &now[index];
            if *point != entry.point && !self.asked.contains(point) {
                changes.push(Change::Changed {
                    id: entry.id,
                    point: point.clone(),
                });
            }
            entries.push(Entry {
                listed: *listed,
                point: point.clone(),
                id: entry.id,
            });
        }
        for ((listed, point), taken) in now.into_iter().zip(taken) {
            if taken {
                continue;
            }
            self.given += 1;
            if !self.asked.contains(&point) {
                changes.push(Change::New {
                    id: self.given,
                    point: point.clone(),
                });
            }
            entries.push(Entry {
                listed,
                point,
                id: self.given,
            });
        }
        self.entries = entries;
        changes
    }
}

impl Change {
    /// The protocol's `breakpoint` event body, with lines as the client
    /// counts them by `line_out`.
    pub fn body(&self, line_out: impl Fn(u64) -> u64) -> Value {
        let (reason, id, point) = match self {
            Self::New { id, point } => ("new", id, point),
            Self::Changed { id, point } => ("changed", id, point),
            Self::Removed { id } => {
                return json!({ "reason": "removed", "breakpoint": { "id": id, "verified": false } });
            }
        };
        let mut breakpoint = json!({ "id": id, "verified": true });
        if let Point::Line { file, line } = point {
            breakpoint["source"] = json!({ "path": file });
            breakpoint["line"] = json!(line_out(*line));
        }
        json!({ "reason": reason, "breakpoint": breakpoint })
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

    /// The list's entries as the inspector names them: `(id, line)`.
    fn listed(revision: u64, entries: &[(u64, u64)]) -> serde_json::Value {
        let specs: Vec<_> = entries
            .iter()
            .map(|(id, line)| json!({ "id": id, "file": "/p/src/main.rs", "line": line }))
            .collect();
        json!({ "revision": revision, "breakpoints": specs })
    }

    #[test]
    fn a_breakpoint_listed_by_the_same_id_on_another_line_is_changed() {
        let mut known = Known::default();
        let changes = known.follow(&listed(1, &[(1, 3), (2, 5)]));
        let ids: Vec<u64> = changes
            .iter()
            .map(|change| match change {
                Change::New { id, .. } => *id,
                other => panic!("{other:?}"),
            })
            .collect();
        // A save moves the first onto the second's old line and the second
        // down: each keeps its id, matched by the inspector's.
        assert_eq!(
            known.follow(&listed(2, &[(1, 5), (2, 6)])),
            [
                Change::Changed {
                    id: ids[0],
                    point: line(5)
                },
                Change::Changed {
                    id: ids[1],
                    point: line(6)
                },
            ]
        );
        assert_eq!(
            Change::Changed {
                id: ids[0],
                point: line(5)
            }
            .body(|line| line)["reason"],
            "changed"
        );
        // Set again where it is, under a new inspector id: the same one.
        assert_eq!(known.follow(&listed(3, &[(1, 5), (4, 6)])), []);
        assert_eq!(known.id(&line(6)), Some(ids[1]));
    }
}
