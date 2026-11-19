//! The debugger window: one run's session, its program's phase, output and
//! latest frame, the source where it stopped, its frames and bindings, an
//! eval console and the controls that step it.
//!
//! Everything the window shows comes from the session's replies and
//! events, which arrive as messages: the window never waits on the
//! session, so it keeps drawing while the program is paused.

use std::path::PathBuf;

use iced::theme::Mode;
use iced::widget::{button, column, container, image, row, rule, scrollable, text};
use iced::{Element, Fill, Font, Length, Subscription, Task, Theme};
use serde_json::{json, Value};

use crate::bindings;
use crate::console;
use crate::session::{self, Client, Incoming, Link, Phase, Stop};
use crate::source;

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
    frame: Option<image::Handle>,
    /// The program's frames, innermost first, as its last answer gave them.
    stack: Vec<Frame>,
    selected: usize,
    /// The stop the window last read the frames of.
    inspected: Option<Stop>,
    /// How many reads of the program's frames the window asked: only the
    /// answer to the newest shows, since an earlier one tells of a pause
    /// or a frame the window has left.
    reads: u64,
    /// The line breakpoints set, each a file and a line.
    breakpoints: Vec<(String, u64)>,
    /// The last thing that went wrong, until the next.
    note: Option<String>,
    source: source::State,
    console: console::State,
    bindings: bindings::State,
    /// The system draws light: the window follows it.
    light: bool,
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
    Breakpoints,
    Inspect { frame: usize, read: u64 },
    Break,
    Step { resumes: u64 },
    Continue,
    Pause,
    Eval { entry: usize },
    Screenshot { file: PathBuf },
}

/// How a step goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Over,
    Into,
    Out,
}

#[derive(Clone, Debug)]
pub enum Message {
    Session(session::Event),
    Source(source::Message),
    Console(console::Message),
    Bindings(bindings::Message),
    Select(usize),
    Step(Step),
    Continue,
    Pause,
    Refresh,
    Captured(Result<Vec<u8>, String>),
    /// The system's light or dark mode.
    Mode(Mode),
}

impl Step {
    const fn name(self) -> &'static str {
        match self {
            Self::Over => "over",
            Self::Into => "into",
            Self::Out => "out",
        }
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
            root: socket.display().to_string(),
            socket,
            capture,
            captures: 0,
            resumes: 0,
            link: None,
            client: Client::default(),
            phase: Phase::Starting,
            output: String::new(),
            frame: None,
            stack: Vec::new(),
            selected: 0,
            inspected: None,
            reads: 0,
            breakpoints: Vec::new(),
            note: None,
            source: source::State::default(),
            console: console::State::default(),
            bindings: bindings::State::default(),
            light: false,
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
    pub fn title(&self) -> String {
        format!("rawr gui: {}", self.root)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run_with(self.socket.clone(), session::connect).map(Message::Session),
            iced::system::theme_changes().map(Message::Mode),
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
                Some(Incoming::Event(event)) => {
                    // Each pause is a new one, even where the last was.
                    if event["event"] == "paused" {
                        self.inspected = None;
                    }
                    let phase = self.phase.after(&event);
                    self.moved(phase)
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
                if let Some(source::Instruction::Toggle { file, line }) = action.instruction {
                    let request = if self.breakpoints.contains(&(file.clone(), line)) {
                        json!({ "op": "clear", "file": file, "line": line })
                    } else {
                        json!({ "op": "break", "file": file, "line": line })
                    };
                    let kind = if request["op"] == "clear" {
                        Asked::Breakpoints
                    } else {
                        Asked::Break
                    };
                    self.ask(request, kind);
                }
                action.task.map(Message::Source)
            }
            Message::Console(message) => {
                let action = self.console.update(message);
                if let Some(console::Instruction::Eval { entry, code }) = action.instruction {
                    self.ask(json!({ "op": "eval", "code": code }), Asked::Eval { entry });
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
            // The program goes on: it runs from now to its next pause, and
            // only a pause, which ends a step that runs long, can be asked.
            Message::Step(step) => {
                let resumes = self.resumes + 1;
                self.ask(
                    json!({ "op": "step", "kind": step.name() }),
                    Asked::Step { resumes },
                );
                self.went_on()
            }
            Message::Continue => {
                self.ask(json!({ "op": "continue" }), Asked::Continue);
                self.went_on()
            }
            Message::Pause => {
                self.ask(json!({ "op": "pause" }), Asked::Pause);
                Task::none()
            }
            Message::Refresh => {
                self.capture();
                Task::none()
            }
            Message::Captured(Ok(bytes)) => {
                self.frame = Some(image::Handle::from_bytes(bytes));
                Task::none()
            }
            Message::Captured(Err(error)) => {
                self.note = Some(error);
                Task::none()
            }
            Message::Mode(mode) => {
                self.light = mode == Mode::Light;
                self.source.follow(self.light);
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
        self.ask(json!({ "frame": frame }), Asked::Inspect { frame, read });
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
        match self.phase.stop().cloned() {
            Some(stop) if self.inspected.as_ref() != Some(&stop) => {
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
                // The breakpoints set so far: a clear that matches none
                // answers with all of them.
                self.ask(json!({ "op": "clear", "line": 0 }), Asked::Breakpoints);
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
            Asked::Breakpoints => {
                self.breakpoints = answer["left"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|point| {
                        Some((point["file"].as_str()?.to_owned(), point["line"].as_u64()?))
                    })
                    .collect();
                Task::none()
            }
            Asked::Break => {
                if let (Some(file), Some(line)) = (
                    answer["set"]["file"].as_str(),
                    answer["set"]["line"].as_u64(),
                ) {
                    let point = (file.to_owned(), line);
                    if !self.breakpoints.contains(&point) {
                        self.breakpoints.push(point);
                    }
                }
                Task::none()
            }
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
                    (Some(value), _) => Ok(value.to_owned()),
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

    pub fn view(&self) -> Element<'_, Message> {
        let paused = self.phase.stop().is_some();
        let live = matches!(self.phase, Phase::Running | Phase::Paused(_));
        let control = |label: &'static str, message: Message, enabled: bool| {
            button(text(label))
                .on_press_maybe(enabled.then_some(message))
                .padding([4, 10])
        };
        let phase = match &self.phase {
            Phase::Building { stage, percent } => format!("building: {stage} {percent}%"),
            Phase::Paused(stop) => format!(
                "paused ({}) at line {}",
                stop.reason,
                stop.line
                    .map_or_else(|| "?".to_owned(), |line| line.to_string())
            ),
            Phase::Ended(status) => format!("ended with status {status}"),
            other => other.name().to_owned(),
        };
        let toolbar = row![
            text(phase).width(Fill),
            control("Continue", Message::Continue, paused),
            control("Pause", Message::Pause, self.phase == Phase::Running),
            control("Step over", Message::Step(Step::Over), paused),
            control("Step into", Message::Step(Step::Into), paused),
            control("Step out", Message::Step(Step::Out), paused),
            control("Refresh frame", Message::Refresh, live && !paused),
        ]
        .spacing(6)
        .align_y(iced::Center);
        let note: Element<'_, Message> = match &self.note {
            Some(note) => text(note.as_str()).size(13).style(text::danger).into(),
            None => text(self.root.as_str()).size(13).into(),
        };

        let frame: Element<'_, Message> = match &self.frame {
            Some(handle) => image(handle.clone()).width(Fill).height(Fill).into(),
            None => container(text("No frame yet")).center(Fill).into(),
        };
        let frames = column(self.stack.iter().enumerate().map(|(index, frame)| {
            let at = match (&frame.file, frame.line) {
                (Some(file), Some(line)) => format!("{}:{line}", short_path(file)),
                _ => String::new(),
            };
            button(
                row![
                    text(frame.function.as_str()).font(Font::MONOSPACE),
                    text(at).size(12),
                ]
                .spacing(8)
                .align_y(iced::Center),
            )
            .width(Fill)
            .padding([2, 6])
            .style(if index == self.selected {
                button::primary
            } else {
                button::text
            })
            .on_press(Message::Select(index))
            .into()
        }));

        let right = column![
            container(frame).height(Length::Fixed(260.0)).width(Fill),
            heading("Frames"),
            scrollable(frames).height(Length::FillPortion(1)),
            heading("Bindings"),
            scrollable(self.bindings.view().map(Message::Bindings))
                .height(Length::FillPortion(2))
                .width(Fill),
        ]
        .spacing(6)
        .width(Length::FillPortion(2));

        let breakpoints: Vec<u64> = self
            .breakpoints
            .iter()
            .filter(|(file, _)| Some(file.as_str()) == self.source.file())
            .map(|(_, line)| *line)
            .collect();
        let left = column![
            container(self.source.view(&breakpoints).map(Message::Source)).height(Fill),
            heading("Console"),
            container(self.console.view().map(Message::Console)).height(Length::Fixed(170.0)),
        ]
        .spacing(6)
        .width(Length::FillPortion(3));

        let output = scrollable(text(self.output.as_str()).font(Font::MONOSPACE).size(12))
            .anchor_bottom()
            .width(Fill)
            .height(Length::Fixed(120.0));

        column![
            toolbar,
            note,
            rule::horizontal(1),
            row![left, rule::vertical(1), right].spacing(8).height(Fill),
            heading("Output"),
            output,
        ]
        .spacing(6)
        .padding(10)
        .into()
    }

    #[must_use]
    pub fn theme(&self) -> Theme {
        if self.light {
            Theme::Light
        } else {
            Theme::Dark
        }
    }
}

fn heading(label: &str) -> Element<'_, Message> {
    text(label).size(13).style(text::secondary).into()
}

/// The last two parts of `file`'s path.
fn short_path(file: &str) -> &str {
    let mut cuts = file.rmatch_indices('/').map(|(at, _)| at);
    cuts.nth(1).map_or(file, |at| &file[at + 1..])
}
