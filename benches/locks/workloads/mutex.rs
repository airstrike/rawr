//! B3: a script `std::sync::Mutex` that threads wait for. A guard whose
//! drop never calls release leaves its waiter to find the lock free at the
//! next 1 ms poll; the fix releases from the guard's drop and waits on the
//! release alone.
//!
//! - `mutex-contended`: threads taking one lock in a loop, half with a named
//!   guard, which never releases, and half with a temporary guard, which
//!   releases at its statement's end.
//! - `mutex-handoff`: a waiter parked on a held lock whose guard never
//!   releases. Its latency is the poll.
//!
//! There is no handoff by a guard that releases: no guard rawr holds
//! across other code calls release today. A `match` subject's guard and a
//! method receiver's temporary don't hold the lock at all (#2352 has the
//! `drop(..)` case), so a waiter never parks behind them. After the fix,
//! `mutex-handoff` is that case.

use crate::script::{self, Script};
use crate::workload::{About, Ran, Workload};

pub const CONTENDED: &str = "mutex-contended";
#[cfg(feature = "counters")]
pub const HANDOFF: &str = "mutex-handoff";

/// Four threads taking the lock 250 times with a named guard (which never
/// calls release), then four with a temporary guard (which does, at its
/// statement's end).
const CONTENDED_SOURCE: &str = r#"
use std::sync::{Arc, Mutex};
use std::thread;

const THREADS: u64 = 4;
const ROUNDS: u64 = 250;

fn named(total: &Arc<Mutex<u64>>) {
    let mut handles = Vec::new();
    for _ in 0..THREADS {
        let total = Arc::clone(total);
        handles.push(thread::spawn(move || {
            for _ in 0..ROUNDS {
                let mut held = total.lock().unwrap();
                *held += 1;
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
}

fn temporary(total: &Arc<Mutex<u64>>) {
    let mut handles = Vec::new();
    for _ in 0..THREADS {
        let total = Arc::clone(total);
        handles.push(thread::spawn(move || {
            for _ in 0..ROUNDS {
                *total.lock().unwrap() += 1;
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
}

fn main() {
    let total = Arc::new(Mutex::new(0u64));
    named(&total);
    temporary(&total);
    let held = total.lock().unwrap();
    println!("total {}", *held);
}
main();
"#;

/// Twenty handoffs. The holder holds the lock, starts a waiter and calls
/// `parked(before)`, which blocks on the counters' event for the count of
/// lock waits to pass `before`: the waiter is then in its wait, under the
/// lock's gate. The holder notes the time and lets its guard end; the
/// waiter notes when it has the lock. The guard is named, and its scope's
/// end calls no release.
#[cfg(feature = "counters")]
const HANDOFF_SOURCE: &str = r#"
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Instant;

const HANDOFFS: u64 = 20;

fn unreleased() -> Vec<u64> {
    let mut gaps = Vec::new();
    for _ in 0..HANDOFFS {
        let lock = Arc::new(Mutex::new(0u64));
        let (got, gotten) = mpsc::channel();
        let released = {
            let _held = lock.lock().unwrap();
            let waiting = Arc::clone(&lock);
            let before = waits();
            thread::spawn(move || {
                let _taken = waiting.lock().unwrap();
                got.send(Instant::now()).unwrap();
            });
            parked(before);
            Instant::now()
        };
        let taken: Instant = gotten.recv().unwrap();
        gaps.push(taken.duration_since(released).as_micros() as u64);
    }
    gaps
}

fn line(name: &str, gaps: &[u64]) {
    let mut text = String::from(name);
    for gap in gaps {
        text.push_str(&format!(" {gap}"));
    }
    println!("{text}");
}
"#;

pub fn contended() -> Workload {
    let script = Script::compile(rawr::Engine::new(), CONTENDED_SOURCE);
    let about = About {
        name: CONTENDED,
        smells: &["B3"],
        run: "4 threads x 250 locks with a named guard, then 4 x 250 with a temporary guard",
    };
    Workload::new(about, CONTENDED_SOURCE.to_owned(), move || {
        let printed = script.run();
        let total = script::values(&printed, "total");
        assert_eq!(total, [2000.0], "every thread's every lock counted");
        Ran::default()
    })
}

#[cfg(feature = "counters")]
pub fn handoff() -> Workload {
    let about = About {
        name: HANDOFF,
        smells: &["B3"],
        run: "20 handoffs to a parked waiter by a guard that never releases; the latency is the 1 ms poll",
    };
    handoffs(about, "unreleased")
}

/// The handoff workload whose holder is the fixture's function `kind`.
#[cfg(feature = "counters")]
fn handoffs(about: About, kind: &'static str) -> Workload {
    let source = format!("{HANDOFF_SOURCE}\nfn main() {{ line(\"gaps\", &{kind}()); }}\nmain();\n");
    let mut engine = rawr::Engine::new();
    engine.register_fn("waits", || rawr::counters::Site::CellWait.count() as i64);
    engine.register_fn("parked", |before: i64| {
        rawr::counters::wait_above(rawr::counters::Site::CellWait, before as u64)
    });
    let script = Script::compile(engine, &source);
    Workload::new(about, source, move || {
        let printed = script.run();
        let mut ran = Ran::default();
        ran.report("handoff", "us", script::values(&printed, "gaps"));
        ran
    })
}
