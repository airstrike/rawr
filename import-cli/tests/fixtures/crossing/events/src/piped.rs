//! A driver generic over a type that implements a trait, which a
//! function's `Wrapper<impl SomeTrait>` fills: it tells what it did as
//! signals of that type over a pipe, whose ends are generic over what they
//! carry, and a signal holds a value of the trait's associated type.

use std::fmt::Debug;
use std::sync::mpsc;

/// Something that steps a count by the words it is given.
pub trait SomeTrait {
    /// What a step carries.
    type Out: Debug;
    /// The count it starts at.
    fn start(&self) -> u32;
    /// The step `word` asks for, if any.
    fn step(&self, word: &str) -> Option<Self::Out>;
    /// The count after `out`.
    fn apply(&self, count: &mut u32, out: Self::Out);
}

/// What a function gives, around a type the caller can't name.
pub struct Wrapper<S: SomeTrait> {
    inner: S,
}

impl<S: SomeTrait> SomeTrait for Wrapper<S> {
    type Out = S::Out;

    fn start(&self) -> u32 {
        self.inner.start()
    }

    fn step(&self, word: &str) -> Option<S::Out> {
        self.inner.step(word)
    }

    fn apply(&self, count: &mut u32, out: S::Out) {
        self.inner.apply(count, out);
    }
}

/// A wrapper whose `"go"` steps the count by `by`.
pub fn wrapped(by: u32) -> Wrapper<impl SomeTrait<Out = u32>> {
    struct By(u32);

    impl SomeTrait for By {
        type Out = u32;

        fn start(&self) -> u32 {
            0
        }

        fn step(&self, word: &str) -> Option<u32> {
            (word == "go").then_some(self.0)
        }

        fn apply(&self, count: &mut u32, out: u32) {
            *count += out;
        }
    }

    Wrapper { inner: By(by) }
}

/// What a driver tells.
pub enum Signal<S: SomeTrait> {
    /// A step a word asked for, which `Driver::perform` takes.
    Moved(Moved<S>),
    /// The word asked for no step.
    Failed(String),
    /// The driver took the step it tells.
    Took(String),
    /// The driver takes the next word.
    Ready,
}

/// A step of `S`.
pub struct Moved<S: SomeTrait>(S::Out);

/// The sending end of a pipe.
pub struct Pipe<T>(mpsc::Sender<T>);

impl<T> Pipe<T> {
    fn send(&self, value: T) {
        let _ = self.0.send(value);
    }
}

/// The receiving end of a pipe.
pub struct Drain<T>(mpsc::Receiver<T>);

/// Nothing was sent yet.
#[derive(Debug)]
pub struct Empty;

impl<T> Drain<T> {
    /// What was sent first and not received yet.
    pub fn try_recv(&mut self) -> Result<T, Empty> {
        self.0.try_recv().map_err(|_| Empty)
    }
}

impl<S: SomeTrait> Drain<Signal<S>> {
    /// What each signal sent and not received yet told.
    pub fn told(&mut self) -> Vec<String> {
        let mut told = Vec::new();
        while let Ok(signal) = self.try_recv() {
            told.push(match signal {
                Signal::Moved(moved) => format!("moved {:?}", moved.0),
                Signal::Failed(word) => format!("failed {word}"),
                Signal::Took(step) => format!("took {step}"),
                Signal::Ready => "ready".to_owned(),
            });
        }
        told
    }
}

/// A pipe of `T`s.
pub fn pipe<T>() -> (Pipe<T>, Drain<T>) {
    let (sender, receiver) = mpsc::channel();
    (Pipe(sender), Drain(receiver))
}

/// A pipe of the signals of a driver of `stepper`.
pub fn pipe_for<S: SomeTrait>(_stepper: &S) -> (Pipe<Signal<S>>, Drain<Signal<S>>) {
    pipe()
}

/// A count that words step.
pub struct Driver<S: SomeTrait> {
    count: u32,
    pipe: Pipe<Signal<S>>,
}

impl<S: SomeTrait> Driver<S> {
    /// The count `stepper` starts at, ready for a word.
    pub fn new(pipe: Pipe<Signal<S>>, stepper: &S) -> Self {
        pipe.send(Signal::Ready);
        Self {
            count: stepper.start(),
            pipe,
        }
    }

    /// Tell the step `word` asks for, or that it asks for none.
    pub fn run(&mut self, stepper: &S, word: &str) {
        match stepper.step(word) {
            Some(out) => self.pipe.send(Signal::Moved(Moved(out))),
            None => self.pipe.send(Signal::Failed(word.to_owned())),
        }
    }

    /// Take the step `word` asks for, and tell it, or that it asks for
    /// none.
    pub fn press(&mut self, stepper: &S, word: &str) {
        match stepper.step(word) {
            Some(out) => {
                let told = format!("{out:?}");
                stepper.apply(&mut self.count, out);
                self.pipe.send(Signal::Took(told));
            }
            None => self.pipe.send(Signal::Failed(word.to_owned())),
        }
    }

    /// Take the step.
    pub fn perform(&mut self, stepper: &S, moved: Moved<S>) {
        stepper.apply(&mut self.count, moved.0);
        self.pipe.send(Signal::Ready);
    }

    /// The count.
    pub fn count(&self) -> u32 {
        self.count
    }
}
