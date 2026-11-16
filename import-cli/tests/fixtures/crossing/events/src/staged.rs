//! A trait whose method lends a slice of a type that names the trait's
//! associated types, implemented by a generic wrapper that forwards them to
//! the opaque type it wraps. The slice's elements hold a closure, so they
//! don't clone: a caller borrows them in place.

use std::marker::PhantomData;

/// A named way to start, which makes a state and the notes it starts with.
pub struct Stage<State, Note> {
    name: String,
    start: Box<dyn Fn() -> (State, Vec<Note>)>,
}

impl<State, Note> Stage<State, Note> {
    /// A stage of the name that starts with what `start` makes.
    pub fn new(name: impl Into<String>, start: impl Fn() -> (State, Vec<Note>) + 'static) -> Self {
        Self {
            name: name.into(),
            start: Box::new(start),
        }
    }

    /// The stage's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// What has stages.
pub trait Staged {
    /// The state a stage starts.
    type State;
    /// What a stage notes as it starts.
    type Note;

    /// What starts with no stage.
    fn start(&self) -> (Self::State, Vec<Self::Note>);

    /// The stages, borrowed from `self`.
    fn stages(&self) -> &[Stage<Self::State, Self::Note>] {
        &[]
    }
}

/// A wrapper that holds its stages beside what it wraps.
pub struct Play<P: Staged> {
    inner: P,
    stages: Vec<Stage<P::State, P::Note>>,
}

/// A play that starts with what `start` makes, with no stages.
pub fn play<State, Note>(
    start: impl Fn() -> (State, Vec<Note>),
) -> Play<impl Staged<State = State, Note = Note>> {
    struct Instance<State, Note, F> {
        start: F,
        made: PhantomData<fn() -> (State, Note)>,
    }

    impl<State, Note, F: Fn() -> (State, Vec<Note>)> Staged for Instance<State, Note, F> {
        type State = State;
        type Note = Note;

        fn start(&self) -> (State, Vec<Note>) {
            (self.start)()
        }
    }

    Play {
        inner: Instance {
            start,
            made: PhantomData,
        },
        stages: Vec::new(),
    }
}

impl<P: Staged> Play<P> {
    /// The play with `stages` as its stages: a builder that shares its
    /// name with the trait's method.
    #[must_use]
    pub fn stages(self, stages: impl IntoIterator<Item = Stage<P::State, P::Note>>) -> Self {
        Self {
            stages: stages.into_iter().collect(),
            ..self
        }
    }
}

impl<P: Staged> Staged for Play<P> {
    type State = P::State;
    type Note = P::Note;

    fn start(&self) -> (P::State, Vec<P::Note>) {
        self.inner.start()
    }

    fn stages(&self) -> &[Stage<P::State, P::Note>] {
        &self.stages
    }
}

/// How many notes `stage` starts with, or `staged` with no stage; and how
/// many stages `staged` has.
pub fn started<P: Staged>(staged: &P, stage: Option<&Stage<P::State, P::Note>>) -> (usize, usize) {
    let stages = staged.stages().len();
    let (_, notes) = match stage {
        Some(stage) => (stage.start)(),
        None => staged.start(),
    };
    (notes.len(), stages)
}
