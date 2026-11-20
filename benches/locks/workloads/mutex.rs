//! B5: a script `std::sync::Mutex` that threads contend for. Its waiter
//! wakes once a millisecond to catch a guard dropped without a release;
//! the fix wakes it from the guard's drop alone.
//!
//! `mutex-contended` is throughput: threads taking one lock in a loop, with
//! a guard released at its scope's end and a temporary guard. `mutex-handoff`
//! is latency: a waiter blocked on a held lock, and how long after the
//! holder's guard ends at its scope it has the lock. A guard given to
//! `drop(..)` is left out: rawr never releases its lock today, so a later
//! `lock()` waits forever (benches/README.md).

use crate::script::{self, Script};
use crate::workload::{About, Ran, Workload};

pub const CONTENDED: &str = "mutex-contended";
pub const HANDOFF: &str = "mutex-handoff";

/// Four threads, each taking the lock 250 times, twice: once with a named
/// guard, once with a temporary one. A `thread::Builder` starts the
/// second set, as programs that name their threads do (C1).
const CONTENDED_SOURCE: &str = r#"
use std::sync::{Arc, Mutex};
use std::thread;

const THREADS: u64 = 4;
const ROUNDS: u64 = 250;

fn named() -> u64 {
    let total = Arc::new(Mutex::new(0u64));
    let mut handles = Vec::new();
    for _ in 0..THREADS {
        let total = Arc::clone(&total);
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
    let done = *total.lock().unwrap();
    done
}

fn temporary() -> u64 {
    let total = Arc::new(Mutex::new(0u64));
    let mut handles = Vec::new();
    for n in 0..THREADS {
        let total = Arc::clone(&total);
        let builder = thread::Builder::new().name(format!("worker-{n}"));
        handles.push(builder.spawn(move || {
            for _ in 0..ROUNDS {
                *total.lock().unwrap() += 1;
            }
        }).unwrap());
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let done = *total.lock().unwrap();
    done
}

fn main() {
    println!("named {}", named());
    println!("temporary {}", temporary());
}
main();
"#;

/// Twenty handoffs: the holder waits for the waiter to block (it said so,
/// then 2 ms passed), notes the time and lets its guard's scope end; the
/// waiter notes when it has the lock.
const HANDOFF_SOURCE: &str = r#"
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const HANDOFFS: u64 = 20;

fn scoped() -> Vec<u64> {
    let mut gaps = Vec::new();
    for _ in 0..HANDOFFS {
        let lock = Arc::new(Mutex::new(0u64));
        let (got, gotten) = mpsc::channel();
        let released = {
            let _held = lock.lock().unwrap();
            let (ready, readied) = mpsc::channel();
            let waiting = Arc::clone(&lock);
            let got = got.clone();
            thread::spawn(move || {
                ready.send(()).unwrap();
                let _taken = waiting.lock().unwrap();
                got.send(Instant::now()).unwrap();
            });
            readied.recv().unwrap();
            thread::sleep(Duration::from_millis(2));
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

fn main() {
    line("scoped", &scoped());
}
main();
"#;

pub fn contended() -> Workload {
    let script = Script::compile(CONTENDED_SOURCE);
    let about = About {
        name: CONTENDED,
        smells: &["B5", "C1"],
        run: "4 threads x 250 locks with a named guard, then 4 Builder threads x 250 with a temporary guard",
    };
    Workload::new(about, CONTENDED_SOURCE.to_owned(), move || {
        let printed = script.run();
        for name in ["named", "temporary"] {
            let total = script::values(&printed, name);
            assert_eq!(total, [1000.0], "every thread's every lock counted");
        }
        Ran::default()
    })
}

pub fn handoff() -> Workload {
    let script = Script::compile(HANDOFF_SOURCE);
    let about = About {
        name: HANDOFF,
        smells: &["B5"],
        run: "20 handoffs of a held lock to a blocked waiter at the guard's scope end",
    };
    Workload::new(about, HANDOFF_SOURCE.to_owned(), move || {
        let printed = script.run();
        let mut ran = Ran::default();
        ran.report("handoff.scoped", "us", script::values(&printed, "scoped"));
        ran
    })
}
