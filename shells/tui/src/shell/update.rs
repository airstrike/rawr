//! What each input does to the shell: keys become requests to the
//! session, and its answers and events become what the shell shows.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{json, Value};

use super::{
    Binding, Breakpoint, Flow, Focus, Frame, Input, Kind, Line, Pending, Prompt, Purpose, Shell,
    Stream,
};
use crate::client::{Message, Reply, NOT_RUNNING};
use crate::phase::{Phase, Stop};
use crate::value::Node;

/// Console lines kept, oldest first out.
const CONSOLE: usize = 2000;
/// How far Page Up and Page Down move the source cursor.
const PAGE: usize = 20;

impl Shell {
    /// Take one input.
    pub fn handle(&mut self, input: Input) -> Flow {
        match input {
            Input::Key(key) => return self.key(key),
            Input::Resize => {}
            Input::Session(message) => self.message(message),
            Input::Output { stream, text } => {
                self.client
                    .notify("output", json!({ "stream": stream.name(), "text": text }));
                self.output(stream, &text);
            }
        }
        Flow::Continue
    }

    fn key(&mut self, key: KeyEvent) -> Flow {
        if key.kind == KeyEventKind::Release {
            return Flow::Continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Flow::Quit;
        }
        if self.prompt.is_some() {
            self.prompt_key(key.code);
            return Flow::Continue;
        }
        self.status = None;
        match key.code {
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.next().next(),
            KeyCode::Up | KeyCode::Char('k') => self.go(-1),
            KeyCode::Down | KeyCode::Char('j') => self.go(1),
            KeyCode::PageUp => self.go(-(PAGE as isize)),
            KeyCode::PageDown => self.go(PAGE as isize),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ')
                if self.focus == Focus::Bindings =>
            {
                self.toggle(true);
            }
            KeyCode::Left if self.focus == Focus::Bindings => self.toggle(false),
            KeyCode::Char('b') => self.toggle_breakpoint(),
            KeyCode::Char('B') => self.open(Purpose::Break),
            KeyCode::Char('e' | ':') => self.open(Purpose::Eval),
            KeyCode::Char('s' | 'n') => self.resume(json!({ "op": "step", "kind": "over" })),
            KeyCode::Char('i') => self.resume(json!({ "op": "step", "kind": "into" })),
            KeyCode::Char('o') => self.resume(json!({ "op": "step", "kind": "out" })),
            KeyCode::Char('c') => self.resume(json!({ "op": "continue" })),
            KeyCode::Char('p') => self.pause(),
            _ => {}
        }
        Flow::Continue
    }

    fn prompt_key(&mut self, code: KeyCode) {
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        match code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Backspace => {
                prompt.text.pop();
            }
            KeyCode::Char(char) => prompt.text.push(char),
            KeyCode::Enter => {
                let Some(prompt) = self.prompt.take() else {
                    return;
                };
                let text = prompt.text.trim();
                if text.is_empty() {
                    return;
                }
                match prompt.purpose {
                    Purpose::Eval => self.evaluate(text.to_owned()),
                    Purpose::Break => self.break_at(text),
                }
            }
            _ => {}
        }
    }

    fn open(&mut self, purpose: Purpose) {
        self.prompt = Some(Prompt {
            purpose,
            text: String::new(),
        });
    }

    /// Move the focused pane's cursor by `by`.
    fn go(&mut self, by: isize) {
        match self.focus {
            Focus::Source => {
                let last = self.source.lines.len().saturating_sub(1);
                self.cursor = self.cursor.saturating_add_signed(by).min(last);
            }
            Focus::Frames => {
                let last = self.frames.len().saturating_sub(1);
                let frame = self.frame.saturating_add_signed(by).min(last);
                if frame != self.frame && self.phase.stop().is_some() {
                    self.frame = frame;
                    self.inspect();
                }
            }
            Focus::Bindings => {
                let last = self.rows().len().saturating_sub(1);
                self.row = self.row.saturating_add_signed(by).min(last);
            }
        }
    }

    /// Open (or close) the selected value.
    fn toggle(&mut self, open: bool) {
        let rows = self.rows();
        let Some(row) = rows.get(self.row) else {
            return;
        };
        if !row.expandable {
            return;
        }
        let path = row.path.clone();
        if open && !self.expanded.remove(&path) {
            self.expanded.insert(path);
        } else if !open {
            self.expanded.remove(&path);
        }
    }

    /// Set a breakpoint at the cursor's line, or clear the one there.
    fn toggle_breakpoint(&mut self) {
        let Some(path) = &self.source.path else {
            return;
        };
        let file = path.to_string_lossy().into_owned();
        let line = self.cursor + 1;
        let set = self.breakpoints.contains(&Breakpoint::Line {
            file: file.clone(),
            line,
        });
        let (op, pending) = if set {
            ("clear", Pending::Clear)
        } else {
            ("break", Pending::Break)
        };
        self.program(json!({ "op": op, "file": file, "line": line }), pending);
    }

    /// Break where `text` says: `file:line`, a line of the shown file, or
    /// a function's name.
    fn break_at(&mut self, text: &str) {
        let shown = self
            .source
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        let request = match text
            .rsplit_once(':')
            .map(|(file, line)| (file, line.parse::<usize>()))
        {
            Some((file, Ok(line))) if !file.ends_with(':') => {
                json!({ "op": "break", "file": self.absolute(file), "line": line })
            }
            _ => match (text.parse::<usize>(), shown) {
                (Ok(line), Some(file)) => json!({ "op": "break", "file": file, "line": line }),
                _ => json!({ "op": "break", "function": text }),
            },
        };
        self.program(request, Pending::Break);
    }

    /// `file` as the inspector names files: absolute, from the directory of
    /// the shown file's package when relative.
    fn absolute(&self, file: &str) -> String {
        let path = Path::new(file);
        if path.is_absolute() {
            return file.to_owned();
        }
        let base = Path::new(&self.root);
        let base = if base.is_file() {
            base.parent().unwrap_or(base)
        } else {
            base
        };
        let joined = base.join(path);
        std::fs::canonicalize(&joined)
            .unwrap_or(joined)
            .to_string_lossy()
            .into_owned()
    }

    fn resume(&mut self, request: Value) {
        if self.phase.stop().is_none() {
            self.status = Some("the program isn't paused: set a breakpoint with `b`".to_owned());
            return;
        }
        self.program(request, Pending::Resume);
    }

    /// Pause the running program at the next statement it runs; its
    /// `paused` event says where.
    fn pause(&mut self) {
        if !self.phase.is_live() {
            self.status = Some("the program isn't running".to_owned());
            return;
        }
        if self.phase.stop().is_some() {
            return;
        }
        self.program(json!({ "op": "pause" }), Pending::Resume);
    }

    fn evaluate(&mut self, code: String) {
        self.push(Kind::Eval, code.clone());
        self.program(json!({ "op": "eval", "code": code }), Pending::Eval);
    }

    /// Read the shown frame's bindings, where the program is paused.
    fn inspect(&mut self) {
        let frame = self.frame;
        self.cancel_reads();
        self.program(json!({ "frame": frame }), Pending::Inspect { frame });
    }

    fn program(&mut self, request: Value, pending: Pending) {
        self.send("program", request, pending);
    }

    pub(super) fn send(&mut self, method: &str, params: Value, pending: Pending) {
        let id = self.client.request(method, params);
        self.pending.insert(id, pending);
    }

    fn message(&mut self, message: Message) {
        match message {
            Message::Answer { id, reply } => {
                if let Some(pending) = self.pending.remove(&id) {
                    self.answered(pending, reply);
                }
            }
            Message::Event(event) => self.event(&event),
            Message::Output { stream, text } => {
                let stream = if stream == "stderr" {
                    Stream::Stderr
                } else {
                    Stream::Stdout
                };
                self.output(stream, &text);
            }
            Message::Closed => {
                self.phase = Phase::Gone;
                self.paused_view_ends();
                self.pending.clear();
                self.push(Kind::Note, "the run ended".to_owned());
            }
        }
    }

    fn event(&mut self, event: &Value) {
        if event["event"] == "breakpoints" {
            self.follow_breakpoints(event);
            return;
        }
        let before = self.phase.stop().cloned();
        self.phase = self.phase.after(event);
        match event["event"].as_str().unwrap_or_default() {
            "failed" => {
                for diagnostic in event["diagnostics"].as_array().into_iter().flatten() {
                    let message = diagnostic["message"].as_str().unwrap_or_default();
                    let at = match (diagnostic["file"].as_str(), diagnostic["line"].as_u64()) {
                        (Some(file), Some(line)) => format!("{}:{line}: ", file_name(file)),
                        _ => String::new(),
                    };
                    self.push(Kind::Error, format!("{at}{message}"));
                }
            }
            "ended" => self.push(Kind::Note, self.phase.to_string()),
            "restarting" => self.push(
                Kind::Note,
                format!(
                    "restarting: {}",
                    event["reason"].as_str().unwrap_or("a save")
                ),
            ),
            _ => {}
        }
        match self.phase.stop().cloned() {
            Some(stop) if before.as_ref() != Some(&stop) || event["event"] == "paused" => {
                self.paused_at(&stop);
            }
            Some(_) => {}
            None => self.paused_view_ends(),
        }
    }

    /// The program paused at `stop`: show it and read its innermost frame.
    fn paused_at(&mut self, stop: &Stop) {
        self.show(Path::new(stop.file()), stop.line());
        self.frame = 0;
        self.inspect();
    }

    /// The program went on: its frames are gone.
    fn paused_view_ends(&mut self) {
        self.frames.clear();
        self.frame = 0;
        self.bindings.clear();
        self.unread = None;
        self.row = 0;
        self.cancel_reads();
    }

    /// Give up the reads still waiting: a read the program can't answer
    /// where it is waits for its next step.
    fn cancel_reads(&mut self) {
        let reads: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, pending)| matches!(pending, Pending::Inspect { .. }))
            .map(|(id, _)| *id)
            .collect();
        for id in reads {
            self.pending.remove(&id);
            self.client.notify("cancel", json!({ "id": id }));
        }
    }

    fn answered(&mut self, pending: Pending, reply: Reply) {
        let answer = match reply {
            Reply::Result(answer) => answer,
            Reply::Error { code, message } => {
                self.status = Some(if code == NOT_RUNNING {
                    "the program isn't running".to_owned()
                } else {
                    message
                });
                if pending == Pending::Eval {
                    self.push(Kind::Error, "the program isn't running".to_owned());
                }
                return;
            }
        };
        match pending {
            Pending::Hello => self.greeted(&answer),
            Pending::Inspect { frame } => self.inspected(frame, &answer),
            // The session tells the breakpoints as they are now to every
            // shell, this one among them.
            Pending::Break | Pending::Clear => {
                if let Some(error) = answer["error"].as_str() {
                    self.status = Some(error.to_owned());
                }
            }
            Pending::Resume => match answer["error"].as_str() {
                Some("not paused") => self.status = Some("the program isn't paused".to_owned()),
                Some(error) => self.status = Some(error.to_owned()),
                None => {}
            },
            Pending::Eval => match (answer["value"].as_str(), answer["error"].as_str()) {
                (Some(value), _) => self.push(Kind::Value, value.to_owned()),
                (None, Some("not paused")) => {
                    self.push(Kind::Error, "the program isn't paused".to_owned());
                }
                (None, Some(error)) => self.push(Kind::Error, error.to_owned()),
                (None, None) => {}
            },
        }
    }

    /// The session's `hello`: what happened so far, and the output kept.
    fn greeted(&mut self, answer: &Value) {
        self.root = answer["root"].as_str().unwrap_or_default().to_owned();
        for piece in answer["output"].as_array().into_iter().flatten() {
            let stream = if piece["stream"] == "stderr" {
                Stream::Stderr
            } else {
                Stream::Stdout
            };
            if let Some(text) = piece["text"].as_str() {
                self.output(stream, text);
            }
        }
        for event in answer["events"].as_array().into_iter().flatten() {
            self.phase = self.phase.after(event);
        }
        self.follow_breakpoints(&answer["breakpoints"]);
        if let Some(stop) = self.phase.stop().cloned() {
            self.paused_at(&stop);
        }
    }

    fn inspected(&mut self, frame: usize, answer: &Value) {
        if frame != self.frame || self.phase.stop().is_none() {
            return;
        }
        if answer["reached"] != true {
            self.status = Some(format!("frame {frame} can't be read where the program is"));
            return;
        }
        self.frames = answer["frames"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|at| Frame {
                function: at["function"].as_str().unwrap_or("?").to_owned(),
                file: at["file"].as_str().map(str::to_owned),
                line: at["line"]
                    .as_u64()
                    .and_then(|line| usize::try_from(line).ok()),
            })
            .collect();
        self.bindings = answer["bindings"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|binding| Binding {
                name: binding["name"].as_str().unwrap_or_default().to_owned(),
                value: match binding["value"].as_str() {
                    Some(text) => Ok(Node::parse(text)),
                    None => Err(binding["error"].as_str().unwrap_or_default().to_owned()),
                },
            })
            .collect();
        self.unread = answer["unread"].as_str().map(str::to_owned);
        self.row = self.row.min(self.rows().len().saturating_sub(1));
        // A caller shows the line of its call.
        if let (Some(file), Some(line)) = (answer["file"].as_str(), answer["line"].as_u64()) {
            self.show(Path::new(file), usize::try_from(line).unwrap_or(1));
        }
    }

    /// Show `path`, with the cursor on one-based `line`.
    pub(super) fn show(&mut self, path: &Path, line: usize) {
        // A file is read again each time, as a save may have changed it;
        // its colours are kept while its text is the same.
        let lines: Vec<String> = std::fs::read_to_string(path)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        if self.source.path.as_deref() != Some(path) || self.source.lines != lines {
            self.source.colours.forget();
            self.source.lines = lines;
            self.source.path = Some(path.to_path_buf());
        }
        self.cursor = line
            .saturating_sub(1)
            .min(self.source.lines.len().saturating_sub(1));
    }

    /// The breakpoints the session told, `{"revision", "breakpoints"}`,
    /// when newer than those shown: whichever shell set or cleared them.
    fn follow_breakpoints(&mut self, told: &Value) {
        let Some(revision) = told["revision"]
            .as_u64()
            .filter(|revision| *revision >= self.revision)
        else {
            return;
        };
        self.revision = revision;
        self.breakpoints = told["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Breakpoint::from_json)
            .collect();
    }

    fn output(&mut self, stream: Stream, text: &str) {
        let kind = match stream {
            Stream::Stdout => Kind::Stdout,
            Stream::Stderr => Kind::Stderr,
        };
        let mut pieces = text.split('\n').peekable();
        while let Some(piece) = pieces.next() {
            let last = pieces.peek().is_none();
            if last && piece.is_empty() {
                self.partial = None;
                break;
            }
            match self.console.last_mut() {
                Some(line) if self.partial == Some(kind) && line.kind == kind => {
                    line.text.push_str(piece);
                }
                _ => self.push(kind, piece.to_owned()),
            }
            self.partial = if last { Some(kind) } else { None };
        }
    }

    fn push(&mut self, kind: Kind, text: String) {
        self.partial = None;
        self.console.push(Line { kind, text });
        if self.console.len() > CONSOLE {
            let excess = self.console.len() - CONSOLE;
            self.console.drain(..excess);
        }
    }
}

impl Focus {
    const fn next(self) -> Self {
        match self {
            Self::Source => Self::Frames,
            Self::Frames => Self::Bindings,
            Self::Bindings => Self::Source,
        }
    }
}

impl Breakpoint {
    /// A breakpoint as the inspector writes one: `{"file", "line"}` or
    /// `{"function"}`.
    fn from_json(spec: &Value) -> Option<Self> {
        if let Some(name) = spec["function"].as_str() {
            return Some(Self::Function(name.to_owned()));
        }
        Some(Self::Line {
            file: spec["file"].as_str()?.to_owned(),
            line: usize::try_from(spec["line"].as_u64()?).ok()?,
        })
    }
}

/// A path's last part, for a short line.
pub(super) fn file_name(file: &str) -> String {
    Path::new(file).file_name().map_or_else(
        || file.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}
