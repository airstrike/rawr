//! C: the async runtime's polls. Each poll of a script future as a std
//! future asks `served()`, which locks `DRIVER`, though `drive` sets it
//! once and never clears it; a script future native code takes as its own
//! type (`Future::into_native`, as a generated bridge's `Task::future`
//! does) hands its output over in a `Made<T>` cell when an owner serves it.
//!
//! `futures-undriven` is a process no owner thread drives, as a tokio
//! worker's is. `futures-driven` is one whose runtime entry drives the
//! owner thread (D527): the output is made there, by the job its queue
//! takes. Once driven a process stays driven, so it runs last.
//!
//! Each future is a ready one, which a bridge converts and a runtime polls
//! as it would a script's: what the lock fixes change is the poll's own
//! bookkeeping, which is the same for every kind of future.

use rawr::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use crate::workload::{About, Ran, Workload};

pub const UNDRIVEN: &str = "futures-undriven";
pub const DRIVEN: &str = "futures-driven";

/// Futures one run polls, of each kind.
const FUTURES: i64 = 1000;

/// What the workload runs, for its fixture identity.
const FIXTURE: &str = "1000 x Future::ready(n).into_native(as_int) polled to the end, \
                       then 1000 x Future::ready(n).into_script() polled to the end";

pub fn undriven() -> Workload {
    let about = About {
        name: UNDRIVEN,
        smells: &["C"],
        run:
            "1000 converted and 1000 script futures polled to their end on a thread no owner drives",
    };
    Workload::new(about, FIXTURE.to_owned(), polls)
}

pub fn driven() -> Workload {
    rawr::future::drive(Waker::noop());
    let about = About {
        name: DRIVEN,
        smells: &["C"],
        run:
            "1000 converted and 1000 script futures polled to their end on the driven owner thread",
    };
    Workload::new(about, FIXTURE.to_owned(), polls)
}

/// Poll every future of one run to its end, running the owner's queued
/// jobs between polls as the runtime entry's poll does.
fn polls() -> Ran {
    let mut context = Context::from_waker(Waker::noop());
    let mut total = 0;
    for n in 0..FUTURES {
        let future = Future::ready(rawr::Dynamic::from(n));
        future.carry(|_| None);
        let mut converted = future.into_native(|value: rawr::Dynamic| value.as_int().unwrap_or(-1));
        total += finish(Pin::new(&mut converted), &mut context);
    }
    for n in 0..FUTURES {
        let mut script = Future::ready(rawr::Dynamic::from(n)).into_script();
        let value = finish(Pin::new(&mut script), &mut context);
        total += value.into_inner().as_int().unwrap_or(-1);
    }
    assert_eq!(
        total,
        FUTURES * (FUTURES - 1),
        "every future gives its value"
    );
    Ran::default()
}

/// Poll `future` until it ends.
fn finish<F: std::future::Future + Unpin>(
    mut future: Pin<&mut F>,
    context: &mut Context<'_>,
) -> F::Output {
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(context) {
            return output;
        }
        rawr::future::drain();
    }
}
