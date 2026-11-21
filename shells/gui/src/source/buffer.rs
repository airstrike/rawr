//! A file's buffer: the text being edited and the text on disk it was
//! last read or saved as.

use std::borrow::Cow;

use iced::widget::text_editor::{Action, Content};

/// A file's text being edited, and whether it differs from the file's.
#[derive(Debug)]
pub struct Buffer {
    content: Content,
    saved: String,
    edited: bool,
}

impl Buffer {
    /// A buffer of `text`, as read from disk.
    pub fn read(text: String) -> Self {
        Self {
            content: Content::with_text(&text),
            saved: text,
            edited: false,
        }
    }

    pub const fn content(&self) -> &Content {
        &self.content
    }

    /// The buffer holds edits not saved.
    pub const fn edited(&self) -> bool {
        self.edited
    }

    /// Take `text`, read from disk again: a buffer without edits shows it,
    /// and one with edits keeps them.
    pub fn reread(&mut self, text: String) {
        if self.edited || text == self.saved {
            return;
        }
        self.content = Content::with_text(&text);
        self.saved = text;
    }

    /// Apply the editor's `action`.
    pub fn perform(&mut self, action: Action) {
        let edit = action.is_edit();
        self.content.perform(action);
        if edit {
            self.edited = self.content.text() != self.saved;
        }
    }

    /// The text to write to disk.
    pub fn text(&self) -> String {
        self.content.text()
    }

    /// `text` was written to disk: the buffer holds no edits unless it
    /// changed since.
    pub fn wrote(&mut self, text: String) {
        self.edited = self.content.text() != text;
        self.saved = text;
    }

    /// Line `index`'s text, from 0.
    pub fn line(&self, index: usize) -> Option<Cow<'_, str>> {
        self.content.line(index).map(|line| line.text)
    }

    pub fn lines(&self) -> usize {
        self.content.line_count()
    }
}
