//! The source where the program is, as an editor: the file's text in
//! colour, edited in place and saved with Cmd-S, which rawr's run takes as
//! any save. The line the program is at is marked, the values of the
//! names near it show at the ends of their lines, a name under the pointer
//! shows its value, and the gutter sets and clears a breakpoint on each
//! line.
//!
//! Each file shown keeps its buffer: edits not saved stay across stops in
//! other files. A file changed on disk shows its new text when its buffer
//! holds no edits.

mod buffer;
mod inline;
mod reload;
mod state;
mod view;

use std::collections::HashMap;

use iced::widget::scrollable::Viewport;
use iced::widget::text_editor;
use iced::Point;

pub use reload::{Failure, Reload};
pub use state::saving;

use buffer::Buffer;

/// The files shown and the one shown now.
#[derive(Debug, Default)]
pub struct State {
    file: Option<String>,
    buffers: HashMap<String, Buffer>,
    /// Why the file shown couldn't be read.
    unread: Option<String>,
    /// The line marked: where the program stopped, or the call a caller's
    /// frame is at.
    marked: Option<u64>,
    /// The line and column under the pointer, from 0.
    pointer: Option<(usize, usize)>,
    /// The part of the file in view: its top and height.
    viewport: Option<(f32, f32)>,
    /// What the last save did.
    reload: Option<Reload>,
}

#[derive(Clone, Debug)]
pub enum Message {
    Loaded {
        file: String,
        text: Result<String, String>,
    },
    Toggle(u64),
    Edit(text_editor::Action),
    /// Write the file shown to disk.
    Save,
    /// The pointer moved over the code, to this point in it.
    Point(Point),
    /// The pointer left the code.
    Leave,
    Scrolled(Viewport),
}

/// What the source asks of the debugger.
#[derive(Debug, PartialEq, Eq)]
pub enum Instruction {
    /// Set a breakpoint at the line, or clear the one there.
    Toggle { file: String, line: u64 },
}

/// Whether `key` with `modifiers` is the save's: Cmd-S, or Ctrl-S off
/// macOS. The editor and the window both take it.
fn saves(key: &iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> bool {
    modifiers.command() && key.as_ref() == iced::keyboard::Key::Character("s")
}

/// The last two parts of `file`'s path.
#[must_use]
pub fn short_path(file: &str) -> &str {
    let mut cuts = file.rmatch_indices('/').map(|(at, _)| at);
    cuts.nth(1).map_or(file, |at| &file[at + 1..])
}
