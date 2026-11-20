//! C1: `served()` locks the driver slot on every poll off the owner
//! thread, only to ask whether it was ever set.
//!
//! This is a program whose async entry owns the program, and whose worker
//! threads poll the futures native code took as its own type, as a tokio
//! runtime's workers poll iced's `Task::future`. The owner's loop is the
//! runtime entry's (`island.rs`, the async `main`'s poll): each poll drives
//! with its waker, enters, and runs the jobs the workers queued. A worker's
//! future hands its output over on the owner, and the owner's job wakes it.
//!
//! The fix leaves every lock here but `served()`'s, and changes no count,
//! so this workload has no counted site: its signal is its time, in a
//! build without the counters.

use rawr::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread;

use crate::workload::{About, Ran, Workload};

pub const NAME: &str = "futures";

/// Worker threads, and the futures each polls to its end.
const WORKERS: usize = 4;
const FUTURES: i64 = 250;

/// What the workload runs, for its fixture identity.
const FIXTURE: &str =
    "4 worker threads x 250 Future::ready(n).into_native(as_int) polled to the end; \
                       the owner loops drive(waker), enter(), drain() per poll";

/// A waker that unparks one thread.
struct Unpark(thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

pub fn workload() -> Workload {
    // The process's owner is driven from the first run on, as a program's
    // is from its entry's first poll.
    rawr::future::drive(&Waker::from(Arc::new(Unpark(thread::current()))));
    let about = About {
        name: NAME,
        smells: &["C1"],
        run: "4 workers x 250 converted futures polled to their end while the owner runs the entry's poll loop",
    };
    Workload::new(about, FIXTURE.to_owned(), run)
}

fn run() -> Ran {
    let finished = Arc::new(AtomicUsize::new(0));
    let owner = thread::current();
    let workers: Vec<thread::JoinHandle<i64>> = (0..WORKERS)
        .map(|_| {
            let finished = Arc::clone(&finished);
            let owner = owner.clone();
            thread::spawn(move || {
                let total = poll_futures();
                finished.fetch_add(1, Ordering::Release);
                owner.unpark();
                total
            })
        })
        .collect();
    let waker = Waker::from(Arc::new(Unpark(owner)));
    loop {
        rawr::future::drive(&waker);
        {
            let _entered = rawr::future::enter();
            rawr::future::drain();
        }
        if finished.load(Ordering::Acquire) == WORKERS {
            break;
        }
        thread::park();
    }
    for worker in workers {
        let total = worker
            .join()
            .expect("a worker polls its futures to their end");
        assert_eq!(
            total,
            FUTURES * (FUTURES - 1) / 2,
            "every future gives its value"
        );
    }
    Ran::default()
}

/// One worker's futures, each polled until it ends, parking between polls
/// until the owner's job wakes it.
fn poll_futures() -> i64 {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut total = 0;
    for n in 0..FUTURES {
        let mut converted = Future::ready(rawr::Dynamic::from(n))
            .into_native(|value: rawr::Dynamic| value.as_int().unwrap_or(-1));
        loop {
            let polled = std::future::Future::poll(Pin::new(&mut converted), &mut context);
            if let Poll::Ready(value) = polled {
                total += value;
                break;
            }
            thread::park();
        }
    }
    total
}
