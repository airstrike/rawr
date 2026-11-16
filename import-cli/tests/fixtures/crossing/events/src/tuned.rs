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
