//! A type whose inherent builder and whose trait method share a name: the
//! builder takes the value and an argument, the trait's method takes
//! `&self` and none. A program that holds the type behind `impl Tune` sees
//! only the trait's method.

/// A tuner over a source, which keeps the notes its builder was given.
#[derive(Debug)]
pub struct Tuner<S> {
    source: S,
    notes: Vec<String>,
}

/// A tuner over `source`, with no notes.
#[must_use]
pub fn tuner<S: Tune>(source: S) -> Tuner<S> {
    Tuner {
        source,
        notes: Vec::new(),
    }
}

impl<S: Tune> Tuner<S> {
    /// The tuner with `notes` as its notes.
    #[must_use]
    pub fn notes(self, notes: impl IntoIterator<Item = String>) -> Self {
        Self {
            notes: notes.into_iter().collect(),
            ..self
        }
    }
}

/// What gives notes.
pub trait Tune {
    /// The notes, borrowed from `self`.
    fn notes(&self) -> &[String];

    /// A name for the notes.
    fn name(&self) -> String {
        self.notes().join("-")
    }
}

impl<S: Tune> Tune for Tuner<S> {
    fn notes(&self) -> &[String] {
        &self.notes
    }

    fn name(&self) -> String {
        format!("{} over {}", self.notes.join("-"), self.source.name())
    }
}

/// A source with no notes of its own.
#[derive(Debug, Default)]
pub struct Silence;

impl Tune for Silence {
    fn notes(&self) -> &[String] {
        &[]
    }

    fn name(&self) -> String {
        "silence".to_owned()
    }
}

/// What opens a sheet to write notes on, in the shape of a serializer: its
/// method takes it by value and gives its associated sheet.
pub trait Open {
    /// The sheet it opens.
    type Sheet: Sheet;

    /// The sheet titled `title`.
    ///
    /// # Errors
    ///
    /// When the title is empty.
    fn open(self, title: &str) -> Result<Self::Sheet, String>;
}

/// A sheet notes are written on.
pub trait Sheet {
    /// Writes `note` on the sheet.
    ///
    /// # Errors
    ///
    /// When the note is empty.
    fn write(&mut self, note: &str) -> Result<(), String>;

    /// What the sheet holds, once written.
    ///
    /// # Errors
    ///
    /// When nothing was written.
    fn close(self) -> Result<String, String>;
}

/// What writes itself on a sheet, in the shape of a serializable value:
/// native code calls its generic method with an `Open` of its own.
pub trait Written {
    /// Writes `self` on a sheet `open` opens.
    ///
    /// # Errors
    ///
    /// When a write fails.
    fn write_on<O: Open>(&self, open: O) -> Result<String, String>;
}

/// `written`, written on a pad's sheet.
///
/// # Errors
///
/// When a write fails.
pub fn on_pad(written: &impl Written) -> Result<String, String> {
    written.write_on(Pad)
}

/// A pad of sheets.
#[derive(Debug, Default)]
pub struct Pad;

/// A sheet of a pad.
#[derive(Debug)]
pub struct Page {
    lines: Vec<String>,
}

impl Open for Pad {
    type Sheet = Page;

    fn open(self, title: &str) -> Result<Page, String> {
        if title.is_empty() {
            return Err("untitled".to_owned());
        }
        Ok(Page {
            lines: vec![title.to_owned()],
        })
    }
}

impl Sheet for Page {
    fn write(&mut self, note: &str) -> Result<(), String> {
        if note.is_empty() {
            return Err("empty".to_owned());
        }
        self.lines.push(note.to_owned());
        Ok(())
    }

    fn close(self) -> Result<String, String> {
        Ok(self.lines.join(": "))
    }
}
