//! The runtime half of [`crate::future`]: futures Rawr code holds, and
//! [`Script`], which polls one as a std future.

// Without `sync` the values are not `Send`, and one thread runs everything.
#![cfg_attr(not(feature = "sync"), allow(clippy::arc_with_non_send_sync))]

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::task::{Context, Waker};

use super::{Frame, Poll};

use crate::{item, Dynamic, EvalAltResult, Position, RawrResultOf};

/// What a native future gives when it ends: a function that makes its Rawr
/// value on the owner thread, where imported values are stored.
#[cfg(feature = "sync")]
pub type Finish = Box<dyn FnOnce() -> RawrResultOf<Dynamic> + Send>;
/// What a native future gives when it ends.
#[cfg(not(feature = "sync"))]
pub type Finish = Box<dyn FnOnce() -> RawrResultOf<Dynamic>>;

/// A native future, made on the thread that first polls it.
#[cfg(feature = "sync")]
pub type Native = Pin<Box<dyn std::future::Future<Output = Finish> + Send>>;
/// A native future, made on the thread that first polls it.
#[cfg(not(feature = "sync"))]
pub type Native = Pin<Box<dyn std::future::Future<Output = Finish>>>;

/// What makes a native future.
#[cfg(feature = "sync")]
type Make = Box<dyn FnOnce() -> Native + Send>;
#[cfg(not(feature = "sync"))]
type Make = Box<dyn FnOnce() -> Native>;

/// Work for the owner thread, boxed.
#[cfg(feature = "sync")]
type Work = Box<dyn FnOnce() + Send>;
#[cfg(not(feature = "sync"))]
type Work = Box<dyn FnOnce()>;

/// `Send` when the crate is built with `sync`, as the values are.
#[cfg(feature = "sync")]
pub trait MaybeSend: Send {}
#[cfg(feature = "sync")]
impl<T: Send> MaybeSend for T {}
/// `Send` when the crate is built with `sync`, as the values are.
#[cfg(not(feature = "sync"))]
pub trait MaybeSend {}
#[cfg(not(feature = "sync"))]
impl<T> MaybeSend for T {}

/// A future as Rawr code holds it.
#[derive(Clone)]
pub struct Future(Arc<Mutex<Option<Kind>>>);

enum Kind {
    /// An async body's step function, not yet started.
    Script(item::Value),
    /// A native future, made when first polled.
    Native(Make),
    /// Futures polled together; their outputs in order, as a tuple.
    Join(Vec<Future>),
    /// A value already there.
    Ready(Dynamic),
}

impl std::fmt::Debug for Future {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Future")
    }
}

impl Future {
    /// The future of an async body whose step function is `step`: called
    /// with `()` it starts, and called with a [`Frame`] it goes on.
    #[must_use]
    pub fn script(step: item::Value) -> Self {
        Self::of(Kind::Script(step))
    }

    /// A native future that `make` makes on the thread that first polls it.
    #[must_use]
    pub fn native(make: impl FnOnce() -> Native + MaybeSend + 'static) -> Self {
        Self::of(Kind::Native(Box::new(make)))
    }

    /// The futures in `parts`, polled together; their outputs make one
    /// tuple, in order, as `join!` gives them.
    #[must_use]
    pub fn join(parts: Vec<Self>) -> Self {
        Self::of(Kind::Join(parts))
    }

    /// A future whose output is `value`.
    #[must_use]
    pub fn ready(value: Dynamic) -> Self {
        Self::of(Kind::Ready(value))
    }

    fn of(kind: Kind) -> Self {
        Self(Arc::new(Mutex::new(Some(kind))))
    }

    /// Run this future as a std future. Its first run takes it: a copy
    /// that runs again panics, as a moved future can't be polled twice.
    #[must_use]
    pub fn into_script(self) -> Script {
        Script {
            poller: Poller::from(&self),
        }
    }
}

/// A Rawr future as a [`std::future::Future`]. It is `Send` when the crate
/// is built with `sync`.
pub struct Script {
    poller: Poller,
}

impl std::fmt::Debug for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Script")
    }
}

impl Script {
    /// Poll for the Rawr output, or the error that ended it.
    pub fn poll_output(&mut self, cx: &mut Context<'_>) -> std::task::Poll<RawrResultOf<Dynamic>> {
        self.poller.poll(cx)
    }
}

impl std::future::Future for Script {
    type Output = crate::Value;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Self::Output> {
        match self.poller.poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Ok(value)) => std::task::Poll::Ready(crate::Value::from(value)),
            std::task::Poll::Ready(Err(error)) => panic!("{}", message(&error)),
        }
    }
}

/// The text of a script failure, as `rawr run` prints it.
#[must_use]
pub fn message(error: &EvalAltResult) -> String {
    match error {
        EvalAltResult::ErrorRuntime(value, ..) if value.is_string() => value.to_string(),
        error => error.to_string(),
    }
}

/// A run-time failure with `message`, as a native future's output gives
/// one.
pub fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    failure(message)
}

fn failure(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

enum Poller {
    Script(Box<Stepper>),
    Native {
        make: Option<Make>,
        running: Option<Native>,
        made: Option<Arc<Slot>>,
    },
    /// Each part until it ends, then its output: a part that ended is
    /// dropped then, with what it captured, as `join!` drops it.
    Join(Vec<(Option<Poller>, Option<Dynamic>)>),
    Ready(Option<Dynamic>),
    Failed(Option<Box<EvalAltResult>>),
}

impl From<&Future> for Poller {
    fn from(future: &Future) -> Self {
        let kind = future
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match kind {
            None => Self::Failed(Some(failure("rawr: this future was already awaited"))),
            Some(Kind::Script(step)) => Self::Script(Box::new(Stepper::new(step))),
            Some(Kind::Native(make)) => Self::Native {
                make: Some(make),
                running: None,
                made: None,
            },
            Some(Kind::Join(parts)) => Self::Join(
                parts
                    .iter()
                    .map(|part| (Some(Self::from(part)), None))
                    .collect(),
            ),
            Some(Kind::Ready(value)) => Self::Ready(Some(value)),
        }
    }
}

impl Poller {
    fn poll(&mut self, cx: &mut Context<'_>) -> std::task::Poll<RawrResultOf<Dynamic>> {
        use std::task::Poll::{Pending, Ready};
        match self {
            Self::Script(stepper) => stepper.poll(cx),
            Self::Native {
                make,
                running,
                made,
            } => {
                if let Some(slot) = made {
                    return slot.poll(cx);
                }
                if running.is_none() {
                    let Some(make) = make.take() else {
                        return Ready(Err(failure(
                            "rawr: a native future was polled after it ended",
                        )));
                    };
                    *running = Some(make());
                }
                let Some(future) = running.as_mut() else {
                    return Pending;
                };
                match future.as_mut().poll(cx) {
                    Pending => Pending,
                    Ready(finish) => {
                        *running = None;
                        let slot = Slot::run(finish, cx.waker());
                        let output = slot.poll(cx);
                        *made = Some(slot);
                        output
                    }
                }
            }
            Self::Join(parts) => {
                let mut pending = false;
                for (part, output) in parts.iter_mut() {
                    let Some(poller) = part.as_mut() else {
                        continue;
                    };
                    match poller.poll(cx) {
                        Pending => pending = true,
                        Ready(Ok(value)) => {
                            *output = Some(value);
                            *part = None;
                        }
                        Ready(Err(error)) => return Ready(Err(error)),
                    }
                }
                if pending {
                    return Pending;
                }
                Ready(Ok(Dynamic::from_array(
                    parts
                        .iter_mut()
                        .filter_map(|(_, output)| output.take())
                        .collect(),
                )))
            }
            Self::Ready(value) => match value.take() {
                Some(value) => Ready(Ok(value)),
                None => Ready(Err(failure("rawr: a future was polled after it ended"))),
            },
            Self::Failed(error) => Ready(Err(error
                .take()
                .unwrap_or_else(|| failure("rawr: a future was polled after it ended")))),
        }
    }
}

/// An async body run step by step on the owner thread.
struct Stepper {
    shared: Arc<Mutex<Step>>,
    awaiting: Option<Box<Poller>>,
}

struct Step {
    /// The step function, with what the body captured; dropped when the
    /// body ends, as Rust drops a finished future's state.
    step: Option<item::Value>,
    /// The suspended body, once it has awaited.
    frame: Option<Frame>,
    /// The awaited future's output, to resume with.
    input: Option<Dynamic>,
    state: State,
    waker: Option<Waker>,
}

enum State {
    /// Ready to run its next step.
    Idle,
    /// Its next step waits for the owner.
    Queued,
    /// The step awaits this future.
    Awaiting(Future),
    /// The body ended.
    Done(RawrResultOf<Dynamic>),
    /// The output was taken.
    Taken,
}

impl Stepper {
    fn new(step: item::Value) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Step {
                step: Some(step),
                frame: None,
                input: None,
                state: State::Idle,
                waker: None,
            })),
            awaiting: None,
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> std::task::Poll<RawrResultOf<Dynamic>> {
        use std::task::Poll::{Pending, Ready};
        loop {
            if let Some(awaiting) = self.awaiting.as_mut() {
                let output = match awaiting.poll(cx) {
                    Pending => return Pending,
                    Ready(output) => output,
                };
                self.awaiting = None;
                let mut step = lock(&self.shared);
                match output {
                    Ok(value) => {
                        step.input = Some(value);
                        step.state = State::Idle;
                    }
                    Err(error) => {
                        step.state = State::Taken;
                        return Ready(Err(error));
                    }
                }
            }
            let mut step = lock(&self.shared);
            match std::mem::replace(&mut step.state, State::Taken) {
                State::Idle => {
                    if owns() {
                        drop(step);
                        run(&self.shared);
                    } else {
                        step.state = State::Queued;
                        step.waker = Some(cx.waker().clone());
                        drop(step);
                        let shared = self.shared.clone();
                        send(Job::new(move || {
                            run(&shared);
                            if let Some(waker) = lock(&shared).waker.take() {
                                waker.wake();
                            }
                        }));
                        return Pending;
                    }
                }
                State::Queued => {
                    step.state = State::Queued;
                    step.waker = Some(cx.waker().clone());
                    return Pending;
                }
                State::Awaiting(future) => {
                    drop(step);
                    self.awaiting = Some(Box::new(Poller::from(&future)));
                }
                State::Done(output) => return Ready(output),
                State::Taken => {
                    return Ready(Err(failure("rawr: a future was polled after it ended")))
                }
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Run one step of the body in `shared` on this, the owner thread.
fn run(shared: &Mutex<Step>) {
    let (step, argument) = {
        let mut state = lock(shared);
        let argument = match (state.frame.take(), state.input.take()) {
            (Some(frame), input) => frame.resume(input.unwrap_or(Dynamic::UNIT)),
            (None, _) => Dynamic::UNIT,
        };
        let Some(step) = state.step.clone() else {
            state.state = State::Done(Err(failure("rawr: a future was polled after it ended")));
            return;
        };
        (step, argument)
    };
    let result = with_host(|host| host.invoke(&step, &mut [argument]))
        .unwrap_or_else(|| Err(failure("rawr: no program runs async code on this thread")));
    let mut state = lock(shared);
    let state = &mut *state;
    state.state = match result {
        Err(error) => State::Done(Err(error)),
        // A body that never awaits gives its output as it is.
        Ok(output) if !output.is::<Poll>() => State::Done(Ok(output)),
        Ok(output) => match output.cast::<Poll>() {
            Poll::Ready(value) => State::Done(Ok(value)),
            Poll::Pending { future, frame } => {
                state.frame = Some(frame);
                match future.try_cast::<Future>() {
                    Some(future) => State::Awaiting(future),
                    None => State::Done(Err(failure(
                        "rawr: an async body awaited a value that is not a future",
                    ))),
                }
            }
        },
    };
    if matches!(state.state, State::Done(_)) {
        state.step = None;
        state.frame = None;
    }
}

/// A native output made a Rawr value on the owner thread.
struct Slot {
    output: Mutex<(Option<RawrResultOf<Dynamic>>, Option<Waker>)>,
}

impl Slot {
    fn run(finish: Finish, waker: &Waker) -> Arc<Self> {
        let slot = Arc::new(Self {
            output: Mutex::new((None, Some(waker.clone()))),
        });
        if owns() {
            lock(&slot.output).0 = Some(finish());
        } else {
            let shared = slot.clone();
            send(Job::new(move || {
                let output = finish();
                let waker = {
                    let mut state = lock(&shared.output);
                    state.0 = Some(output);
                    state.1.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
            }));
        }
        slot
    }

    fn poll(&self, cx: &mut Context<'_>) -> std::task::Poll<RawrResultOf<Dynamic>> {
        let mut state = lock(&self.output);
        match state.0.take() {
            Some(output) => std::task::Poll::Ready(output),
            None => {
                state.1 = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    }
}

/// The program an owner thread runs async steps in.
pub trait Host {
    /// Call `function` with `arguments` in the current generation.
    fn invoke(&self, function: &item::Value, arguments: &mut [Dynamic]) -> RawrResultOf<Dynamic>;

    /// `value`'s `Debug` text as the program's types print it.
    fn debug(&self, value: &Dynamic) -> Option<String> {
        let _ = value;
        None
    }
}

thread_local! {
    static HOST: RefCell<Option<Rc<dyn Host>>> = const { RefCell::new(None) };
}

/// Work for the owner thread.
pub struct Job(Work);

impl Job {
    fn new(work: impl FnOnce() + MaybeSend + 'static) -> Self {
        Self(Box::new(work))
    }

    /// Run it, on the owner thread.
    pub fn run(self) {
        (self.0)();
    }
}

#[allow(clippy::type_complexity)]
static FORWARD: OnceLock<Box<dyn Fn(Job) + Send + Sync>> = OnceLock::new();

/// Make this thread the owner that runs async steps in `host`, from now on.
pub fn serve(host: Rc<dyn Host>) {
    HOST.with(|slot| *slot.borrow_mut() = Some(host));
}

/// Where a step polled on another thread goes: `forward` hands it to the
/// owner thread, which [`Job::run`]s it. Set once.
pub fn forward(forward: impl Fn(Job) + Send + Sync + 'static) {
    let _ = FORWARD.set(Box::new(forward));
}

/// Run `work` on the owner thread: here when this is it, and else as a
/// [`Job`] it runs next. A native value that leaves its slot for a future
/// goes back this way when the future ends on another thread (D513.6).
pub fn on_owner(work: impl FnOnce() + MaybeSend + 'static) {
    if owns() {
        work();
    } else {
        send(Job::new(work));
    }
}

/// Whether this thread runs async steps.
#[must_use]
pub fn owns() -> bool {
    HOST.with(|slot| slot.borrow().is_some())
}

fn with_host<T>(work: impl FnOnce(&dyn Host) -> T) -> Option<T> {
    let host = HOST.with(|slot| slot.borrow().clone())?;
    Some(work(host.as_ref()))
}

/// `value`'s `Debug` text through the owner's program, on its thread.
pub(crate) fn debug(value: &Dynamic) -> Option<String> {
    with_host(|host| host.debug(value)).flatten()
}

fn send(job: Job) {
    match FORWARD.get() {
        Some(forward) => forward(job),
        // No owner serves this process: the job runs where it is, and
        // fails there when it needs the program.
        None => job.run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_native_future_runs_where_it_is_polled_and_its_output_is_made_on_the_owner() {
        struct Plain;
        impl Host for Plain {
            fn invoke(&self, _: &item::Value, _: &mut [Dynamic]) -> RawrResultOf<Dynamic> {
                unreachable!()
            }
        }
        serve(Rc::new(Plain));
        let future = Future::join(vec![
            Future::native(|| Box::pin(async { Box::new(|| Ok(Dynamic::from(1_i64))) as Finish })),
            Future::ready(Dynamic::from(2_i64)),
        ]);
        let mut script = future.clone().into_script();
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let std::task::Poll::Ready(Ok(output)) = script.poll_output(&mut cx) else {
            panic!("the join is ready");
        };
        assert_eq!(format!("{output:?}"), "[1, 2]");
        // A copy shares the future, which runs once.
        let mut again = future.into_script();
        assert!(matches!(
            again.poll_output(&mut cx),
            std::task::Poll::Ready(Err(_))
        ));
    }
}
