//! The signal a program opts into with the `rawr-hot` crate (#1903): each
//! generation of its marked code the program takes, told on the thread
//! that owns the program right after the swap, before its windows redraw.
//! A retained-mode framework, as iced, builds its view only after a
//! message, so a program that turns the signal into one shows a view-only
//! save at once.
//!
//! `rawr-hot` is a dependency of the program, built by cargo beside the
//! runner, so it shares no Rust item with this crate. The runner names this
//! process and the address of [`listen`] in [`VARIABLE`] as it starts, and
//! `rawr-hot` reads it and hands [`listen`] its C function. Under cargo the
//! variable names no runner, and `rawr-hot` never calls anything.

use std::ptr;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// The variable that holds `"{process id}:{address of listen, in hex}"`.
pub const VARIABLE: &str = "RAWR_HOT";

/// What `rawr-hot` gives [`listen`]: told each generation's number.
type Notify = extern "C" fn(u64);

/// One `rawr-hot` in the process, of the listeners [`LISTENERS`] holds.
struct Listener {
    notify: Notify,
    next: *mut Listener,
}

/// The listeners, newest first. One joins per copy of `rawr-hot` the
/// program links, and none leaves, so the list only grows at its head.
static LISTENERS: AtomicPtr<Listener> = AtomicPtr::new(ptr::null_mut());

/// The number of generations taken.
static TAKEN: AtomicU64 = AtomicU64::new(0);

/// Name this process's [`listen`] in [`VARIABLE`]. A runner calls it first
/// in `main`, before it spawns any thread, as the variable is set then.
pub fn publish() {
    let listen: extern "C" fn(Notify) = listen;
    let value = format!("{}:{:x}", std::process::id(), listen as usize);
    // SAFETY: the runner calls this before any other thread runs, so
    // nothing reads the environment while it changes.
    unsafe { std::env::set_var(VARIABLE, value) };
}

/// Tell `notify` the number of each generation the program takes from now.
extern "C" fn listen(notify: Notify) {
    let listener = Box::into_raw(Box::new(Listener {
        notify,
        next: ptr::null_mut(),
    }));
    let mut head = LISTENERS.load(Ordering::Acquire);
    loop {
        // SAFETY: `listener` is the box made above, which no other thread
        // sees until the exchange below publishes it.
        unsafe { (*listener).next = head };
        match LISTENERS.compare_exchange_weak(head, listener, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(current) => head = current,
        }
    }
}

/// The program took a new generation: tell every listener its number. The
/// runner calls it on the thread that owns the program, right after the
/// generation is admitted.
pub fn taken() {
    let generation = TAKEN.fetch_add(1, Ordering::Relaxed) + 1;
    let mut listener = LISTENERS.load(Ordering::Acquire);
    // SAFETY: each listener is a box `listen` leaked, never freed, and
    // published whole before its pointer was.
    while let Some(current) = unsafe { listener.as_ref() } {
        (current.notify)(generation);
        listener = current.next;
    }
}
