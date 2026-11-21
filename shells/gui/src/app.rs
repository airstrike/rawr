//! The debugger window: one run's session, its program's phase, output and
//! latest frame, the source where it stopped, its frames and bindings, an
//! eval console and the controls that step it.
//!
//! Everything the window shows comes from the session's replies and
//! events, which arrive as messages: the window never waits on the
//! session, so it keeps drawing while the program is paused.

use std::path::PathBuf;

use iced::theme::Mode;
use iced::widget::text::{Ellipsis, Wrapping};
use iced::widget::{
    button, column, container, image, row, rule, scrollable, space, table, text, tooltip,
};
use iced::{keyboard, Element, Fill, Font, Length, Subscription, Task, Theme, Widget};
use iced_split::{horizontal_split, vertical_split};
use serde_json::{json, Value};

use crate::bindings;
use crate::console;
use crate::control::{Control, Shortcut};
use crate::session::{self, Client, Incoming, Link, Phase, Stop};
use crate::source;
use crate::theme;
use crate::tree;

/// Output kept for the window, oldest first out.
const OUTPUT: usize = 64 * 1024;

/// The debugger window.
pub struct App {
    /// The session's socket.
    socket: PathBuf,
    /// Where the program writes the frames it draws for the window: each
    /// capture its own file, numbered from this one.
    capture: PathBuf,
    captures: u64,
    /// How many times the window asked the program to go on: an answer to
    /// an earlier ask says nothing of where it is now.
    resumes: u64,
    link: Option<Link>,
    client: Client<Asked>,
    phase: Phase,
    root: String,
    output: String,
    window: Window,
    /// The program's frames, innermost first, as its last answer gave them.
    stack: Vec<Frame>,
    selected: usize,
    /// The stop the window last read the frames of.
    inspected: Option<Stop>,
    /// How many reads of the program's frames the window asked: only the
    /// answer to the newest shows, since an earlier one tells of a pause
    /// or a frame the window has left.
    reads: u64,
    /// The line breakpoints set, each a file and a line, as the session
    /// last told them, and that list's revision.
    breakpoints: Vec<(String, u64)>,
    revision: u64,
    /// The last thing that went wrong, until the next.
    note: Option<String>,
    source: source::State,
    console: console::State,
    bindings: bindings::State,
    /// The system draws light: the window follows it.
    light: bool,
    /// Where each split's line is, as a share of its whole.
    splits: Splits,
}

/// Where each split's line is: the share of the whole its first pane
/// takes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Splits {
    /// The panes over the output.
    output: f32,
    /// The source and console, left of the frame, frames and bindings.
    columns: f32,
    /// The source over the console.
    console: f32,
    /// The program's frame over the frames and bindings, once it shows a
    /// window.
    frame: f32,
    /// The frames over the bindings, with or without the frame over them.
    frames: f32,
}

/// What the debugger knows of the program's own window: a program shows
/// one once a capture of it first comes, and a program that never shows
/// one, whose captures each answer an error, gets no pane for it.
#[derive(Clone, Debug, Default)]
enum Window {
    /// No capture has come yet.
    #[default]
    Unseen,
    /// The program showed a window: its latest frame, or none while a
    /// restarted program has yet to draw its first. The pane stays either
    /// way, so it never comes and goes across a restart.
    Shown(Option<image::Handle>),
}

/// One of the window's split lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    Output,
    Columns,
    Console,
    Frame,
    Frames,
}

/// One of the program's calls: its function's Rust path and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    function: String,
    file: Option<String>,
    line: Option<u64>,
}

/// What a request was made for, which its reply goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    Hello,
    Clear,
    Inspect { frame: usize, read: u64 },
    Break,
    Step { resumes: u64 },
    Continue,
    Pause,
    Eval { entry: usize },
    Hover { asked: u64 },
    Screenshot { file: PathBuf },
}

#[derive(Clone, Debug)]
pub enum Message {
    Session(session::Event),
    Source(source::Message),
    Console(console::Message),
    Bindings(bindings::Message),
    Select(usize),
    /// A toolbar control, clicked.
    Control(Control),
    /// A control's key, pressed where nothing else took it.
    Shortcut(Shortcut),
    /// A split's line, dragged to this share of its whole.
    Split(Split, f32),
    Captured(Result<Vec<u8>, String>),
    /// The system's light or dark mode.
    Mode(Mode),
}

impl Default for Splits {
    fn default() -> Self {
        Self {
            output: 0.86,
            columns: 0.64,
            console: 0.72,
            frame: 0.36,
            frames: 0.34,
        }
    }
}

impl Splits {
    fn set(&mut self, split: Split, at: f32) {
        let share = match split {
            Split::Output => &mut self.output,
            Split::Columns => &mut self.columns,
            Split::Console => &mut self.console,
            Split::Frame => &mut self.frame,
            Split::Frames => &mut self.frames,
        };
        *share = at;
    }
}

impl Frame {
    fn from_json(frame: &Value) -> Self {
        Self {
            function: frame["function"].as_str().unwrap_or("?").to_owned(),
            file: frame["file"].as_str().map(str::to_owned),
            line: frame["line"].as_u64(),
        }
    }
}

impl App {
    /// The window for the session listening on `socket`.
    #[must_use]
    pub fn new(socket: PathBuf) -> Self {
        let capture = socket.with_file_name(format!("gui-{}.png", std::process::id()));
        Self {
            root: String::new(),
            socket,
            capture,
            captures: 0,
            resumes: 0,
            link: None,
            client: Client::default(),
            phase: Phase::Starting,
            output: String::new(),
            window: Window::default(),
            stack: Vec::new(),
            selected: 0,
            inspected: None,
            reads: 0,
            breakpoints: Vec::new(),
            revision: 0,
            note: None,
            source: source::State::default(),
            console: console::State::default(),
            bindings: bindings::State::default(),
            light: false,
            splits: Splits::default(),
        }
    }

    /// The window for the session listening on `socket`, with the task
    /// that reads the system's light or dark mode.
    pub fn boot(socket: PathBuf) -> (Self, Task<Message>) {
        (Self::new(socket), iced::system::theme().map(Message::Mode))
    }

    /// Where the program is, as the session told it.
    #[must_use]
    pub const fn phase(&self) -> &Phase {
        &self.phase
    }

    #[must_use]
    /// The window's title: the program's name, its crate's directory or
    /// its file, once the session says it, and rawr's.
    pub fn title(&self) -> String {
        match std::path::Path::new(&self.root).file_name() {
            Some(program) => format!("{} — rawr", program.to_string_lossy()),
            None => "rawr".to_owned(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run_with(self.socket.clone(), session::connect).map(Message::Session),
            iced::system::theme_changes().map(Message::Mode),
            keyboard::listen()
                .filter_map(Shortcut::of)
                .map(Message::Shortcut),
            keyboard::listen()
                .filter_map(source::saving)
                .map(Message::Source),
        ])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Session(session::Event::Connected(link)) => {
                link.send(&self.client.request("hello", json!({}), Asked::Hello));
                self.link = Some(link);
                Task::none()
            }
            Message::Session(session::Event::Line(line)) => match self.client.receive(&line) {
                Some(Incoming::Reply { kind, result }) => self.answered(kind, result),
                Some(Incoming::Event(event)) if event["event"] == "breakpoints" => {
                    self.follow_breakpoints(&event);
                    Task::none()
                }
                Some(Incoming::Event(event)) => {
                    // Each pause is a new one, even where the last was.
                    if event["event"] == "paused" {
                        self.inspected = None;
                    }
                    let reread = self.source.told(&event).map(Message::Source);
                    let phase = self.phase.after(&event);
                    Task::batch([self.moved(phase), reread])
                }
                Some(Incoming::Output { text, .. }) => {
                    self.heard(&text);
                    Task::none()
                }
                None => Task::none(),
            },
            Message::Session(session::Event::Closed(why)) => {
                self.link = None;
                self.client.abandon();
                self.phase = Phase::Closed;
                self.note = Some(why);
                Task::none()
            }
            Message::Source(message) => {
                let action = self.source.update(message);
                match action.instruction {
                    Some(source::Instruction::Toggle { file, line }) => {
                        let request = if self.breakpoints.contains(&(file.clone(), line)) {
                            json!({ "op": "clear", "file": file, "line": line })
                        } else {
                            json!({ "op": "break", "file": file, "line": line })
                        };
                        let kind = if request["op"] == "clear" {
                            Asked::Clear
                        } else {
                            Asked::Break
                        };
                        self.ask(request, kind);
                    }
                    // A hover reads only a paused program, in the frame
                    // the function under the pointer runs in, the selected
                    // one first.
                    Some(source::Instruction::Hover {
                        file,
                        line,
                        column,
                        text,
                        asked,
                    }) if self.phase.stop().is_some() => {
                        let request = json!({
                            "op": "hover", "file": file, "line": line, "column": column,
                            "text": text, "frame": self.selected,
                        });
                        self.ask(request, Asked::Hover { asked });
                    }
                    Some(source::Instruction::Hover { .. }) | None => {}
                }
                action.task.map(Message::Source)
            }
            Message::Console(message) => {
                let action = self.console.update(message);
                if let Some(console::Instruction::Eval { entry, code }) = action.instruction {
                    let request = json!({
                        "op": "eval", "code": code, "frame": self.selected, "children": true,
                    });
                    self.ask(request, Asked::Eval { entry });
                }
                action.task.map(Message::Console)
            }
            Message::Bindings(message) => {
                self.bindings.update(message);
                Task::none()
            }
            Message::Select(frame) => {
                self.selected = frame;
                self.read(frame);
                Task::none()
            }
            Message::Control(control) => self.act(control),
            Message::Shortcut(shortcut) => match Control::keyed(shortcut, &self.phase) {
                Some(control) => self.act(control),
                None => Task::none(),
            },
            Message::Split(split, at) => {
                self.splits.set(split, at);
                Task::none()
            }
            Message::Captured(Ok(bytes)) => {
                self.window = Window::Shown(Some(image::Handle::from_bytes(bytes)));
                Task::none()
            }
            Message::Captured(Err(error)) => {
                self.note = Some(error);
                Task::none()
            }
            Message::Mode(mode) => {
                self.light = mode == Mode::Light;
                Task::none()
            }
        }
    }

    /// Act on `control`, when the program has something for it to act on.
    fn act(&mut self, control: Control) -> Task<Message> {
        if !control.enabled(&self.phase) {
            return Task::none();
        }
        match control {
            // The program goes on: it runs from now to its next pause, and
            // only a pause, which ends a step that runs long, can be asked.
            Control::Step(step) => {
                let resumes = self.resumes + 1;
                self.ask(
                    json!({ "op": "step", "kind": step.name() }),
                    Asked::Step { resumes },
                );
                self.went_on()
            }
            Control::Continue => {
                self.ask(json!({ "op": "continue" }), Asked::Continue);
                self.went_on()
            }
            Control::Pause => {
                self.ask(json!({ "op": "pause" }), Asked::Pause);
                Task::none()
            }
            Control::Refresh => {
                self.capture();
                Task::none()
            }
        }
    }

    /// Ask the program `request` through the session, for `kind`.
    fn ask(&mut self, request: Value, kind: Asked) {
        if let Some(link) = &self.link {
            link.send(&self.client.program(request, kind));
        }
    }

    /// Read the values of `frame` where the program is: this read's answer
    /// replaces any earlier one's.
    fn read(&mut self, frame: usize) {
        self.reads += 1;
        let read = self.reads;
        self.ask(
            json!({ "frame": frame, "children": true }),
            Asked::Inspect { frame, read },
        );
    }

    /// Ask the program to draw its window for this one, into a file of
    /// this capture's own: two in flight never read or remove each other's.
    fn capture(&mut self) {
        self.captures += 1;
        let stem = self
            .capture
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let file = self
            .capture
            .with_file_name(format!("{stem}-{}.png", self.captures));
        self.ask(
            json!({ "op": "screenshot", "file": file.display().to_string() }),
            Asked::Screenshot { file },
        );
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
            .filter_map(|point| Some((point["file"].as_str()?.to_owned(), point["line"].as_u64()?)))
            .collect();
    }

    /// A step or continue was asked of the paused program, which runs on.
    fn went_on(&mut self) -> Task<Message> {
        self.resumes += 1;
        self.note = None;
        if self.phase.stop().is_some() {
            return self.moved(Phase::Running);
        }
        Task::none()
    }

    fn heard(&mut self, piece: &str) {
        self.output.push_str(piece);
        if self.output.len() > OUTPUT {
            let mut cut = self.output.len() - OUTPUT;
            while !self.output.is_char_boundary(cut) {
                cut += 1;
            }
            self.output.drain(..cut);
        }
    }

    /// The program moved to `phase`: read the frames of a new stop, and
    /// show the frame a program draws once it starts.
    fn moved(&mut self, phase: Phase) -> Task<Message> {
        let was_live = matches!(self.phase, Phase::Running | Phase::Paused(_));
        self.phase = phase;
        // A restarted program is a new one, whose window has yet to draw.
        if self.phase == Phase::Restarting
            && let Window::Shown(frame) = &mut self.window
        {
            *frame = None;
        }
        match self.phase.stop().cloned() {
            Some(stop) if self.inspected.as_ref() != Some(&stop) => {
                self.source.unhover();
                self.selected = 0;
                self.read(0);
                let task = match &stop.file {
                    Some(file) => self.source.show(file, stop.line).map(Message::Source),
                    None => Task::none(),
                };
                self.inspected = Some(stop);
                task
            }
            Some(_) => Task::none(),
            None => {
                self.source.unhover();
                if self.inspected.take().is_some() {
                    // A read still on its way tells of the pause just left.
                    self.reads += 1;
                    self.stack.clear();
                    self.bindings.show(Vec::new(), None);
                }
                if self.phase == Phase::Running && !was_live {
                    self.capture();
                }
                Task::none()
            }
        }
    }

    /// The reply to a request made for `kind`.
    fn answered(&mut self, kind: Asked, result: Result<Value, session::Error>) -> Task<Message> {
        // An earlier read, and what went wrong with it, is of a pause or a
        // frame the window has left.
        if let Asked::Inspect { read, .. } = kind
            && read != self.reads
        {
            return Task::none();
        }
        // A hover that reads nothing shows nothing, and tells no note.
        if let Asked::Hover { asked } = kind {
            if let Ok(answer) = &result {
                self.source.hovered(asked, answer);
            }
            return Task::none();
        }
        let answer = match result {
            Ok(answer) => answer,
            Err(error) => {
                self.note = Some(if error.code == session::NOT_RUNNING {
                    format!("the program isn't running ({})", self.phase.name())
                } else {
                    error.message
                });
                return Task::none();
            }
        };
        if let (Some(error), false) = (
            answer["error"].as_str(),
            matches!(kind, Asked::Eval { .. } | Asked::Screenshot { .. }),
        ) {
            self.note = Some(error.to_owned());
            return Task::none();
        }
        match kind {
            Asked::Hello => {
                self.root = answer["root"].as_str().unwrap_or_default().to_owned();
                let mut phase = self.phase.clone();
                for event in answer["events"].as_array().into_iter().flatten() {
                    phase = phase.after(event);
                }
                for piece in answer["output"].as_array().into_iter().flatten() {
                    if let Some(piece) = piece["text"].as_str() {
                        self.heard(piece);
                    }
                }
                self.follow_breakpoints(&answer["breakpoints"]);
                let moved = self.moved(phase);
                if self.source.file().is_some() {
                    return moved;
                }
                // Until the program stops, its entry file is shown, for
                // breakpoints to be set in.
                let root = std::path::Path::new(&self.root);
                let entry = if root.is_dir() {
                    root.join("src").join("main.rs")
                } else {
                    root.to_path_buf()
                };
                let shown = self
                    .source
                    .show(&entry.display().to_string(), None)
                    .map(Message::Source);
                Task::batch([moved, shown])
            }
            // The session tells the breakpoints as they are now to every
            // shell, this one among them.
            Asked::Break | Asked::Clear | Asked::Hover { .. } => Task::none(),
            Asked::Inspect { frame, .. } => {
                if answer["reached"] != true {
                    self.note = Some(format!("frame {frame} isn't there to read"));
                    return Task::none();
                }
                self.stack = answer["frames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(Frame::from_json)
                    .collect();
                self.bindings.show(
                    answer["bindings"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(bindings::Binding::from_json)
                        .collect(),
                    answer["unread"].as_str().map(str::to_owned),
                );
                match answer["file"].as_str() {
                    Some(file) => self
                        .source
                        .show(file, answer["line"].as_u64())
                        .map(Message::Source),
                    None => Task::none(),
                }
            }
            // A step ends at a new pause, even one where it started: a
            // loop's line, or a breakpoint it came back to. The window went
            // on when it asked, so any pause is new. One that went on into
            // native code says nothing of where it is: the pause it leads to
            // is told as an event, which may come before this answer.
            Asked::Step { resumes } => {
                if resumes != self.resumes {
                    return Task::none();
                }
                let phase = self.phase.answered(&answer["paused"]);
                self.moved(phase)
            }
            Asked::Continue => {
                self.capture();
                Task::none()
            }
            // The program pauses at its next statement, which its `paused`
            // event tells; one already paused says where.
            Asked::Pause => {
                let phase = self.phase.answered(&answer["paused"]);
                if phase.stop().is_some() {
                    return self.moved(phase);
                }
                Task::none()
            }
            Asked::Eval { entry } => {
                let result = match (answer["value"].as_str(), answer["error"].as_str()) {
                    (Some(value), _) => Ok(console::Answer {
                        value: value.to_owned(),
                        parts: tree::Part::all(&answer["children"]),
                    }),
                    (None, Some("not paused")) => {
                        Err("the program isn't paused: there is no frame to evaluate in".to_owned())
                    }
                    (None, error) => Err(error.unwrap_or("no answer").to_owned()),
                };
                self.console.answer(entry, result);
                Task::none()
            }
            Asked::Screenshot { file } => {
                if answer["error"].is_string() {
                    // A paused program's window draws nothing until it goes
                    // on: the frame shown stays.
                    return Task::none();
                }
                Task::perform(
                    async move {
                        let bytes = std::fs::read(&file).map_err(|error| error.to_string());
                        let _ = std::fs::remove_file(&file);
                        bytes
                    },
                    Message::Captured,
                )
            }
        }
    }

    /// The toolbar over the panes, which split the window: the source and
    /// the console left of the program's frame, its frames and bindings,
    /// and the output under them all. The frame's pane shows once the
    /// program shows a window. Each split's line drags.
    pub fn view(&self) -> Element<'_, Message> {
        let left = theme::split(horizontal_split(
            self.source_pane(),
            pane("Console", self.console.view().map(Message::Console)),
            self.splits.console,
            |at| Message::Split(Split::Console, at),
        ));
        let stack = theme::split(horizontal_split(
            pane(
                "Call stack",
                scrollable(self.frames())
                    .direction(theme::scrollbar())
                    .height(Fill),
            ),
            pane(
                "Bindings",
                scrollable(self.bindings.view().map(Message::Bindings))
                    .direction(theme::scrollbar())
                    .height(Fill),
            ),
            self.splits.frames,
            |at| Message::Split(Split::Frames, at),
        ));
        let right = match &self.window {
            Window::Unseen => stack.boxed(),
            Window::Shown(frame) => theme::split(horizontal_split(
                pane("Frame", frame_pane(frame.as_ref())),
                stack,
                self.splits.frame,
                |at| Message::Split(Split::Frame, at),
            ))
            .boxed(),
        };
        let panes = theme::split(vertical_split(left, right, self.splits.columns, |at| {
            Message::Split(Split::Columns, at)
        }));
        let output = scrollable(
            text(self.output.as_str())
                .font(Font::MONOSPACE)
                .size(theme::SIZE),
        )
        .direction(theme::scrollbar())
        .anchor_bottom()
        .width(Fill)
        .height(Fill);
        column![
            container(self.toolbar()).style(theme::side),
            rule::horizontal(1),
            theme::split(horizontal_split(
                panes,
                pane("Output", container(output).padding([2, 6])),
                self.splits.output,
                |at| Message::Split(Split::Output, at),
            )),
        ]
        .boxed()
    }

    /// The controls, each an icon with its label and key in a tooltip, then
    /// where the program is and the last thing that went wrong.
    fn toolbar(&self) -> Element<'_, Message> {
        let controls = Control::ALL.into_iter().map(|control| {
            let press = button(control.icon().size(15).center())
                .width(26)
                .height(24)
                .padding(0)
                .style(theme::control)
                .on_press_maybe(
                    control
                        .enabled(&self.phase)
                        .then_some(Message::Control(control)),
                );
            theme::tipped(press, control.tip(), tooltip::Position::Bottom)
        });
        let note = match &self.note {
            Some(note) => text(note.as_str()).style(theme::error),
            None => text(""),
        };
        row![
            row(controls).spacing(1),
            rule::vertical(1),
            self.status(),
            note.size(theme::SIZE)
                .wrapping(Wrapping::None)
                .ellipsis(Ellipsis::Start)
                .align_x(iced::Right)
                .width(Fill),
        ]
        .spacing(10)
        .height(32)
        .padding([0, 8])
        .align_y(iced::Center)
        .boxed()
    }

    /// Where the program is, as a status: a lamp in the phase's tone, the
    /// phase, and where or how far, quiet.
    fn status(&self) -> Element<'_, Message> {
        let (tone, word, detail) = match &self.phase {
            Phase::Starting => (theme::Tone::Quiet, "Starting", String::new()),
            Phase::Building { stage, percent } => (
                theme::Tone::Quiet,
                "Building",
                format!("{stage} {percent}%"),
            ),
            Phase::Running => (theme::Tone::Good, "Running", String::new()),
            Phase::Paused(stop) => {
                let place = match (&stop.line, &stop.function) {
                    (Some(line), Some(function)) => format!("line {line} in {function}"),
                    (Some(line), None) => format!("line {line}"),
                    (None, Some(function)) => format!("in {function}"),
                    (None, None) => String::new(),
                };
                (
                    theme::Tone::Accent,
                    "Paused",
                    format!("{place} · {}", stop.reason),
                )
            }
            Phase::Restarting => (theme::Tone::Quiet, "Restarting", String::new()),
            Phase::Failed => (
                theme::Tone::Bad,
                "Failed",
                "a save that fixes it starts it".to_owned(),
            ),
            Phase::Ended(status) => (theme::Tone::Quiet, "Ended", format!("status {status}")),
            Phase::Closed => (theme::Tone::Bad, "Closed", String::new()),
        };
        row![
            theme::lamp(tone),
            text(word).size(theme::SIZE).font(theme::STRONG),
            text(detail)
                .size(theme::SIZE)
                .style(theme::faint)
                .wrapping(Wrapping::None),
        ]
        .spacing(7)
        .align_y(iced::Center)
        .boxed()
    }

    fn source_pane(&self) -> Element<'_, Message> {
        let breakpoints: Vec<u64> = self
            .breakpoints
            .iter()
            .filter(|(file, _)| Some(file.as_str()) == self.source.file())
            .map(|(_, line)| *line)
            .collect();
        // The values shown in the code are the frame's whose line is
        // marked, in the file it is in.
        let values: Vec<(&str, &str)> = match self.stack.get(self.selected) {
            Some(frame) if frame.file.as_deref() == self.source.file() => {
                self.bindings.values().collect()
            }
            _ => Vec::new(),
        };
        container(column![
            self.source.heading().map(Message::Source),
            self.source.view(&breakpoints, &values).map(Message::Source),
        ])
        .width(Fill)
        .height(Fill)
        .style(theme::pane)
        .boxed()
    }

    /// The program's calls, innermost first, as a table of each call's
    /// function and where it is; the selected call's row is marked, and a
    /// click on another reads its bindings.
    fn frames(&self) -> Element<'_, Message> {
        // A function's cell fills its column, which takes what the places
        // leave; a place's cell is as wide as its text.
        let cell = |index: usize, content: Element<'static, Message>, width: Length| {
            button(content)
                .width(width)
                .padding([0, 6])
                .style(move |theme, status| theme::item(theme, status, index == self.selected))
                .on_press(Message::Select(index))
                .boxed()
        };
        let functions = table::column(space(), move |(index, frame): (usize, &Frame)| {
            let function = text(frame.function.clone())
                .font(Font::MONOSPACE)
                .size(theme::SIZE)
                .wrapping(Wrapping::None)
                .ellipsis(Ellipsis::Start);
            cell(index, function.boxed(), Fill)
        })
        .width(Fill);
        let places = table::column(space(), move |(index, frame): (usize, &Frame)| {
            let at = match (&frame.file, frame.line) {
                (Some(file), Some(line)) => format!("{}:{line}", source::short_path(file)),
                _ => String::new(),
            };
            cell(
                index,
                text(at)
                    .font(Font::MONOSPACE)
                    .size(theme::SIZE)
                    .style(text::secondary)
                    .wrapping(Wrapping::None)
                    .boxed(),
                Length::Shrink,
            )
        });
        table::table([functions, places], self.stack.iter().enumerate())
            .width(Fill)
            .padding(0)
            .separator(0)
            .boxed()
    }

    #[must_use]
    pub fn theme(&self) -> Theme {
        theme::window(self.light)
    }
}

/// The program's latest frame, or a quiet word while a restarted program
/// has yet to draw.
fn frame_pane(frame: Option<&image::Handle>) -> Element<'_, Message> {
    match frame {
        Some(handle) => image(handle.clone()).width(Fill).height(Fill).boxed(),
        None => container(
            text("Waiting for the window")
                .size(theme::SIZE)
                .style(text::secondary),
        )
        .center(Fill)
        .boxed(),
    }
}

/// A pane under its section bar, on the window's background.
fn pane<'a>(label: &str, content: impl Widget<Message> + 'a) -> Element<'a, Message> {
    container(column![theme::heading(label), content])
        .width(Fill)
        .height(Fill)
        .style(theme::side)
        .boxed()
}
