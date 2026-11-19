//! The terminal shell's state: what the session told it, what the person
//! looks at, and the requests it waits on. [`Shell::handle`] takes each
//! input, a key or a message, and [`Shell::draw`] shows the state.

mod bindings;
mod update;
mod view;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossterm::event::KeyEvent;

use crate::client::{Client, Message};
use crate::phase::Phase;
use crate::value::Node;

/// What the shell takes, in the order it came.
#[derive(Debug)]
pub enum Input {
    /// A key the person pressed.
    Key(KeyEvent),
    /// The terminal changed size: the screen draws again.
    Resize,
    /// What the session said.
    Session(Message),
    /// Output of the run this shell started, which it tells the session
    /// for the other shells.
    Output { stream: Stream, text: String },
}

/// One of a run's output streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    /// The stream's name, as the session's `output` says it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// Whether the shell goes on after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// A terminal debugger on one run's session.
#[derive(Debug)]
pub struct Shell {
    client: Client,
    phase: Phase,
    /// The requests sent and not yet answered, by id.
    pending: HashMap<u64, Pending>,
    /// The program's root, as the session names it.
    root: String,
    /// Where the program's inspector keeps its breakpoints.
    saved: PathBuf,
    breakpoints: Vec<Breakpoint>,
    /// The run's output and the evaluations, oldest first.
    console: Vec<Line>,
    /// The stream whose last piece ended without a newline.
    partial: Option<Kind>,
    /// The file shown, and the line the cursor is on, from zero.
    source: Source,
    cursor: usize,
    /// The paused program's calls, innermost first, and the one shown.
    frames: Vec<Frame>,
    frame: usize,
    bindings: Vec<Binding>,
    /// Why the shown frame's bindings can't be read.
    unread: Option<String>,
    /// The open values, each by its binding's name and the keys down to it.
    expanded: HashSet<Vec<String>>,
    /// The binding row selected.
    row: usize,
    focus: Focus,
    prompt: Option<Prompt>,
    /// The last thing the shell has to say, such as a refused request.
    status: Option<String>,
}

/// A request the shell waits on the answer to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Hello,
    Inspect { frame: usize },
    Break,
    Clear,
    Resume,
    Eval,
}

/// A breakpoint as the inspector keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Breakpoint {
    Line { file: String, line: usize },
    Function(String),
}

/// One line of the console.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    kind: Kind,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Stdout,
    Stderr,
    /// Code given to `eval`.
    Eval,
    /// What it evaluated to.
    Value,
    /// Why it didn't, or a diagnostic of a failed save.
    Error,
    /// What happened to the program.
    Note,
}

/// A file's lines, read when it is shown.
#[derive(Debug, Default)]
struct Source {
    path: Option<PathBuf>,
    lines: Vec<String>,
}

/// One call of the paused program.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    function: String,
    file: Option<String>,
    line: Option<usize>,
}

/// One binding of the shown frame: its value as a tree, or why it can't
/// be read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    name: String,
    value: Result<Node, String>,
}

/// The pane the arrow keys move in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Focus {
    #[default]
    Source,
    Frames,
    Bindings,
}

/// A line the person writes: code to evaluate, or where to break.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Prompt {
    purpose: Purpose,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Eval,
    Break,
}

impl Shell {
    /// A shell on the session `client` talks to, showing `source` until
    /// the program pauses, with the breakpoints kept at `saved`. It says
    /// `hello` at once.
    #[must_use]
    pub fn new(client: Client, source: PathBuf, saved: PathBuf) -> Self {
        let mut shell = Self {
            client,
            phase: Phase::default(),
            pending: HashMap::new(),
            root: String::new(),
            saved,
            breakpoints: Vec::new(),
            console: Vec::new(),
            partial: None,
            source: Source::default(),
            cursor: 0,
            frames: Vec::new(),
            frame: 0,
            bindings: Vec::new(),
            unread: None,
            expanded: HashSet::new(),
            row: 0,
            focus: Focus::default(),
            prompt: None,
            status: None,
        };
        shell.show(&source, 1);
        shell.send("hello", serde_json::Value::Null, Pending::Hello);
        shell
    }
}
