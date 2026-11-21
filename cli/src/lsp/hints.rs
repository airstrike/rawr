//! Inlay hints: where the program is paused, each of the frame's bindings
//! at the end of the last line at or above the pause that names it, up to
//! the start of its function, read by the hover read at that name, so a
//! shadowed name shows the value its line sees.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use super::{document, Server};

/// How far above the paused line names are looked for, at most.
const REACH: usize = 40;

/// The longest value shown at a line's end, in characters.
const LONGEST: usize = 28;

/// The hints of a run's pause.
#[derive(Debug, Default)]
pub(super) enum Hints {
    /// None: the program isn't paused.
    #[default]
    None,
    /// Being read: the reads still to come.
    Reading {
        file: PathBuf,
        left: usize,
        read: Vec<Hint>,
    },
    /// Read, for `file`.
    Read { file: PathBuf, read: Vec<Hint> },
}

impl Hints {
    /// Whether no hint shows.
    pub(super) fn is_empty(&self) -> bool {
        match self {
            Self::None => true,
            Self::Reading { read, .. } | Self::Read { read, .. } => read.is_empty(),
        }
    }
}

/// One binding's value at one line's end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hint {
    /// The line, from 1, and where on it the name is, from 1.
    line: usize,
    column: usize,
    label: String,
}

/// Where a binding is named, as the hints read it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Named {
    name: String,
    line: usize,
    column: usize,
}

impl Server {
    /// Read the hints of link `serial`'s pause, then have the client ask
    /// for them again. A pause that is gone takes its hints with it.
    pub(super) fn read_hints(&mut self, serial: u64) {
        let Some(link) = self.link(serial) else {
            return;
        };
        let shown = !std::mem::take(&mut link.hints).is_empty();
        let (Some(paused), session, stamp) =
            (link.paused.clone(), Arc::clone(&link.session), link.stamp())
        else {
            if shown {
                self.refresh();
            }
            return;
        };
        let (Some(file), Some(line)) = (paused["file"].as_str(), paused["line"].as_u64()) else {
            return;
        };
        let file = std::fs::canonicalize(file).unwrap_or_else(|_| PathBuf::from(file));
        let line = usize::try_from(line).unwrap_or_default();
        let asking = Arc::clone(&session);
        self.ask(&asking, json!({ "frame": 0 }), move |server, answer| {
            if !server.holds(stamp) {
                return;
            }
            let names: Vec<&str> = answer
                .as_ref()
                .and_then(|answer| answer["bindings"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|binding| binding["name"].as_str())
                .collect();
            let Some(text) = server.text_of(&file) else {
                return;
            };
            let named = placed(&text, line, &names);
            let Some(link) = server.link(serial) else {
                return;
            };
            link.hints = Hints::Reading {
                file: file.clone(),
                left: named.len(),
                read: Vec::new(),
            };
            if named.is_empty() {
                return server.hints_read(serial);
            }
            for named in named {
                let request = json!({
                    "op": "hover",
                    "file": file,
                    "line": named.line,
                    "column": named.column,
                    "text": document::line(&text, named.line),
                    "frame": 0,
                });
                server.ask(&session, request, move |server, answer| {
                    if !server.holds(stamp) {
                        return;
                    }
                    let hint = answer.and_then(|answer| {
                        let value = answer["value"].as_str()?;
                        // The name may be a field's, as `score` in
                        // `player.score`: then it isn't the binding.
                        (answer["expression"] == named.name.as_str()).then(|| Hint {
                            line: named.line,
                            column: named.column,
                            label: format!("{} = {}", named.name, short(value)),
                        })
                    });
                    let Some(Hints::Reading { left, read, .. }) =
                        server.link(serial).map(|link| &mut link.hints)
                    else {
                        return;
                    };
                    read.extend(hint);
                    *left -= 1;
                    if *left == 0 {
                        server.hints_read(serial);
                    }
                });
            }
        });
    }

    /// Link `serial`'s hints are all read: the client asks for them again.
    fn hints_read(&mut self, serial: u64) {
        let Some(link) = self.link(serial) else {
            return;
        };
        if let Hints::Reading { file, read, .. } = std::mem::take(&mut link.hints) {
            link.hints = Hints::Read { file, read };
        }
        self.refresh();
    }

    /// Have the client ask for every document's inlay hints again, when it
    /// can.
    pub(super) fn refresh(&mut self) {
        if !self.refreshes {
            return;
        }
        self.asked += 1;
        let id = self.asked;
        self.send(json!({ "id": id, "method": "workspace/inlayHint/refresh" }));
    }

    /// `textDocument/inlayHint`: the hints of the pause in the document,
    /// within the range asked.
    pub(super) fn inlay_hints(&mut self, id: &Value, params: &Value) {
        let hints = self.hints_in(params).unwrap_or_default();
        self.respond(id, json!(hints));
    }

    fn hints_in(&mut self, params: &Value) -> Option<Vec<Value>> {
        let path = self
            .documents
            .get(params["textDocument"]["uri"].as_str()?)?
            .path
            .clone();
        let index = self.program_of(&path)?;
        let Hints::Read { file, read } = &self.programs[index].link.as_ref()?.hints else {
            return None;
        };
        if *file != path {
            return None;
        }
        let text = self.text_of(&path)?;
        let first = params["range"]["start"]["line"].as_u64().unwrap_or(0);
        let last = params["range"]["end"]["line"].as_u64().unwrap_or(u64::MAX);
        let mut lines: Vec<(usize, Vec<&Hint>)> = Vec::new();
        let mut sorted: Vec<&Hint> = read.iter().collect();
        sorted.sort_by_key(|hint| (hint.line, hint.column));
        for hint in sorted {
            match lines.last_mut() {
                Some((line, hints)) if *line == hint.line => hints.push(hint),
                _ => lines.push((hint.line, vec![hint])),
            }
        }
        let encoding = self.encoding;
        let hints = lines
            .into_iter()
            .filter(|(line, _)| (first..=last).contains(&(*line as u64 - 1)))
            .map(|(line, hints)| {
                let written = document::line(&text, line);
                let labels: Vec<&str> = hints.iter().map(|hint| hint.label.as_str()).collect();
                json!({
                    "position": {
                        "line": line - 1,
                        "character": encoding.character(written, written.chars().count() + 1),
                    },
                    "label": labels.join(", "),
                    "paddingLeft": true,
                })
            })
            .collect();
        Some(hints)
    }

    /// The text of `path`: the client's while it has it open, else the
    /// file's.
    pub(super) fn text_of(&self, path: &Path) -> Option<String> {
        self.documents
            .values()
            .find(|document| document.path == path)
            .map(|document| document.text().to_owned())
            .or_else(|| std::fs::read_to_string(path).ok())
    }
}

/// Where each of `names` is named last at or above line `paused` (from 1)
/// of `text`, up to the first line of the function it is in.
fn placed(text: &str, paused: usize, names: &[&str]) -> Vec<Named> {
    let mut seen = vec![false; names.len()];
    let mut placed = Vec::new();
    for line in (paused.saturating_sub(REACH).max(1)..=paused).rev() {
        let written = document::line(text, line);
        let words = words(written);
        for (column, word) in &words {
            if let Some(at) = names.iter().position(|name| name == word)
                && !seen[at]
            {
                seen[at] = true;
                placed.push(Named {
                    name: (*word).to_owned(),
                    line,
                    column: *column,
                });
            }
        }
        // A function's first line names its parameters, and the lines
        // above it are another function's.
        if line < paused && words.iter().any(|(_, word)| *word == "fn") {
            break;
        }
    }
    placed
}

/// The names in `text`, each with the character it starts at, from 1: its
/// identifiers outside strings and comments, and not after a `.`, where a
/// name is a field's.
fn words(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut start: Option<(usize, usize)> = None;
    let mut previous = ' ';
    // The word read now is a field's.
    let mut field = false;
    let mut characters = text.char_indices().enumerate().peekable();
    while let Some((column, (byte, character))) = characters.next() {
        if quoted {
            match (escaped, character) {
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => escaped = false,
            }
            continue;
        }
        if character == '_' || character.is_alphanumeric() {
            if start.is_none() && !field {
                if previous == '.' {
                    field = true;
                } else {
                    start = Some((column + 1, byte));
                }
            }
            previous = character;
            continue;
        }
        field = false;
        if let Some((first, from)) = start.take() {
            found.push((first, &text[from..byte]));
        }
        match (character, characters.peek().map(|(_, (_, next))| *next)) {
            ('/', Some('/')) => break,
            ('"', _) => quoted = true,
            _ => {}
        }
        previous = character;
    }
    if let Some((first, from)) = start.filter(|_| !quoted) {
        found.push((first, &text[from..]));
    }
    found
}

/// `value` as a line's end shows it: on one line, cut short when long.
fn short(value: &str) -> String {
    let flat: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= LONGEST {
        return flat;
    }
    let mut cut: String = flat.chars().take(LONGEST - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::{placed, words, Named};

    #[test]
    fn names_outside_strings_comments_and_fields_are_words() {
        let line = r#"    let total = player.score + turn; // turn "x""#;
        let found: Vec<&str> = words(line).into_iter().map(|(_, word)| word).collect();
        assert_eq!(found, ["let", "total", "player", "turn"]);
        assert_eq!(words("say(\"turn\", turn)")[1], (13, "turn"));
    }

    #[test]
    fn each_binding_is_placed_on_the_last_line_that_names_it_in_its_function() {
        let text = "fn other() { let turn = 9; }\n\
                    fn main() {\n\
                    \x20   let turn = 1;\n\
                    \x20   let score = turn + 2;\n\
                    \x20   println!(\"{score}\");\n";
        let named = placed(text, 5, &["turn", "score"]);
        assert_eq!(
            named,
            [
                Named {
                    name: "score".to_owned(),
                    line: 4,
                    column: 9,
                },
                Named {
                    name: "turn".to_owned(),
                    line: 4,
                    column: 17,
                }
            ]
        );
    }
}
