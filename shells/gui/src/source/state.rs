//! How the source follows the program and the person editing it: the file
//! a stop shows, edits, saves, what a save did, and the pointer.

use iced::widget::operation::scrollable::{scroll_to, AbsoluteOffset};
use iced::widget::operation::Animation;
use iced::{keyboard, Task};
use serde_json::Value;

use super::buffer::Buffer;
use super::hover::Hover;
use super::inline;
use super::reload::Failure;
use super::view::{self, LINE};
use super::{Instruction, Message, Reload, State};
use crate::action::Action;

/// The save a key press asks for: Cmd-S, or Ctrl-S off macOS.
#[must_use]
pub fn saving(event: keyboard::Event) -> Option<Message> {
    let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };
    super::saves(&key, modifiers).then_some(Message::Save)
}

/// Lines kept in view above a marked line it scrolls to.
const ABOVE: f32 = 6.0;

impl State {
    /// Show `file` about `line`. The file is read again, as a save may
    /// have changed it; a buffer with edits keeps them.
    pub fn show(&mut self, file: &str, line: Option<u64>) -> Task<Message> {
        self.marked = line;
        if self.file.as_deref() != Some(file) {
            self.file = Some(file.to_owned());
            self.unread = None;
            self.pointer = None;
            self.hover = None;
            self.viewport = None;
        }
        Task::batch([self.reveal(), self.read()])
    }

    /// The file shown.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// The line marked, from 1.
    #[must_use]
    pub const fn marked(&self) -> Option<u64> {
        self.marked
    }

    /// The file shown holds edits not saved.
    #[must_use]
    pub fn edited(&self) -> bool {
        self.buffer().is_some_and(Buffer::edited)
    }

    /// What the last save did.
    #[must_use]
    pub const fn reload(&self) -> Option<&Reload> {
        self.reload.as_ref()
    }

    /// The session told `event`: when it says what a save did, show it,
    /// and read the file again, which another editor may have saved.
    pub fn told(&mut self, event: &Value) -> Task<Message> {
        let Some(reload) = Reload::of(event) else {
            return Task::none();
        };
        // A restart's build ends in `running`: the restart is what the
        // save did.
        self.reload = Some(reload);
        self.read()
    }

    pub fn update(&mut self, message: Message) -> Action<Instruction, Message> {
        match message {
            Message::Loaded { file, text } => {
                if self.file.as_deref() == Some(file.as_str()) {
                    match text {
                        Ok(text) => {
                            self.unread = None;
                            let fresh = !self.buffers.contains_key(&file);
                            self.buffers
                                .entry(file)
                                .and_modify(|buffer| buffer.reread(text.clone()))
                                .or_insert_with(|| Buffer::read(text));
                            if fresh {
                                return Action::task(self.reveal());
                            }
                        }
                        Err(error) => self.unread = Some(error),
                    }
                }
                Action::none()
            }
            Message::Toggle(line) => match &self.file {
                Some(file) => Action::instruction(Instruction::Toggle {
                    file: file.clone(),
                    line,
                }),
                None => Action::none(),
            },
            Message::Edit(action) => {
                if action.is_edit() {
                    self.pointer = None;
                    self.hover = None;
                }
                if let Some(buffer) = self.buffer_mut() {
                    buffer.perform(action);
                }
                Action::none()
            }
            Message::Save => {
                self.save();
                Action::none()
            }
            Message::Point(point) => {
                self.pointer = view::under(point);
                self.point()
            }
            Message::Leave => {
                self.pointer = None;
                self.hover = None;
                Action::none()
            }
            Message::Scrolled(viewport) => {
                self.viewport = Some((viewport.absolute_offset().y, viewport.bounds.height));
                Action::none()
            }
        }
    }

    /// The pointer moved: ask what the place under it reads as, unless the
    /// hover shown covers it already. Nothing shows where no place is.
    fn point(&mut self) -> Action<Instruction, Message> {
        let Some((line, column)) = self.pointer else {
            self.hover = None;
            return Action::none();
        };
        if self
            .hover
            .as_ref()
            .is_some_and(|hover| hover.covers(line, column))
        {
            return Action::none();
        }
        let text = self.buffer().and_then(|buffer| buffer.line(line));
        let (Some(file), Some(text)) = (self.file.clone(), text) else {
            self.hover = None;
            return Action::none();
        };
        if inline::place_at(&text, column).is_none() {
            self.hover = None;
            return Action::none();
        }
        let text = text.into_owned();
        self.hovers += 1;
        self.hover = Some(Hover::asked(line, column, self.hovers));
        Action::instruction(Instruction::Hover {
            file,
            line: line + 1,
            column: column + 1,
            text,
            asked: self.hovers,
        })
    }

    /// The program answered the hover asked as `asked` with `answer`.
    pub fn hovered(&mut self, asked: u64, answer: &Value) {
        if let Some(hover) = &mut self.hover {
            hover.answered(asked, answer);
        }
    }

    /// The program went on or paused anew: what the hover read is gone,
    /// and the pointer's next move asks again.
    pub fn unhover(&mut self) {
        self.hover = None;
    }

    /// Write the file shown, when it holds edits: rawr's run sees the
    /// save and says what it did.
    fn save(&mut self) {
        let Some(file) = self.file.clone() else {
            return;
        };
        let Some(buffer) = self.buffers.get_mut(&file).filter(|buffer| buffer.edited()) else {
            return;
        };
        let text = buffer.text();
        self.reload = Some(match std::fs::write(&file, &text) {
            Ok(()) => {
                buffer.wrote(text);
                Reload::Saving
            }
            Err(error) => Reload::Failed(Failure {
                message: format!("couldn't write the file: {error}"),
                file: None,
                line: None,
            }),
        });
    }

    /// Read the file shown from disk.
    fn read(&self) -> Task<Message> {
        let Some(file) = self.file.clone() else {
            return Task::none();
        };
        Task::perform(
            {
                let file = file.clone();
                async move { std::fs::read_to_string(&file).map_err(|error| error.to_string()) }
            },
            move |text| Message::Loaded {
                file: file.clone(),
                text,
            },
        )
    }

    /// Scroll the marked line into view, a few lines under the top, when
    /// it is out of view.
    fn reveal(&self) -> Task<Message> {
        let Some(marked) = self.marked else {
            return Task::none();
        };
        let at = (marked.saturating_sub(1) as f32) * LINE;
        if let Some((top, height)) = self.viewport
            && at >= top + LINE
            && at + 2.0 * LINE <= top + height
        {
            return Task::none();
        }
        scroll_to(
            view::scroller(),
            AbsoluteOffset {
                x: None,
                y: Some((at - ABOVE * LINE).max(0.0)),
            },
            Animation::Instant,
        )
    }

    pub(super) fn buffer(&self) -> Option<&Buffer> {
        self.buffers.get(self.file.as_deref()?)
    }

    fn buffer_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.file.as_deref()?)
    }
}

#[cfg(test)]
mod tests {
    use iced::widget::text_editor::{Action, Edit};

    use super::super::{Message, Reload, State};

    /// The save path: an edit marks the buffer, Cmd-S writes the file and
    /// the buffer is clean, and what the run says the save did shows.
    #[test]
    fn an_edit_saves_to_disk_and_the_run_s_answer_shows() {
        let directory = std::env::temp_dir().join(format!("rawr-gui-save-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let file = file.display().to_string();

        let mut state = State::default();
        let _ = state.show(&file, Some(1));
        let _ = state.update(Message::Loaded {
            file: file.clone(),
            text: Ok("fn main() {}\n".to_owned()),
        });
        assert!(!state.edited());
        let _ = state.update(Message::Edit(Action::Edit(Edit::Insert('x'))));
        assert!(state.edited(), "an edit marks the buffer");

        let _ = state.update(Message::Save);
        assert_eq!(state.reload(), Some(&Reload::Saving));
        assert!(!state.edited(), "the saved buffer is clean");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "xfn main() {}\n");

        let _ = state.told(&serde_json::json!({ "event": "swapped" }));
        assert_eq!(state.reload(), Some(&Reload::Swapped));

        // Another editor's save shows in a buffer without edits, and a
        // buffer with edits keeps them.
        let _ = state.update(Message::Loaded {
            file: file.clone(),
            text: Ok("fn main() { 1; }\n".to_owned()),
        });
        assert_eq!(state.buffer().unwrap().text(), "fn main() { 1; }\n");
        let _ = state.update(Message::Edit(Action::Edit(Edit::Insert('y'))));
        let _ = state.update(Message::Loaded {
            file: file.clone(),
            text: Ok("fn main() { 2; }\n".to_owned()),
        });
        assert_eq!(state.buffer().unwrap().text(), "yfn main() { 1; }\n");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
