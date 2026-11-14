//! The runner's side, found at run time: a runner that `rawr run` starts
//! names its process and the address of a C function in `RAWR_HOT`, which
//! takes the function this crate is told each generation through. Under
//! cargo the variable is absent, or names another process that this one
//! inherited it from, and nothing listens.

use std::ptr;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

/// The variable a runner sets as it starts.
const VARIABLE: &str = "RAWR_HOT";

/// The runner's function that takes [`notify`].
type Listen = extern "C" fn(extern "C" fn(u64));

/// What one listener is told: a generation's number.
type Callback = Box<dyn Fn(u64) + Send + Sync>;

/// One listener of the listeners [`LISTENERS`] holds.
struct Listener {
    callback: Callback,
    next: *mut Listener,
}

/// The listeners, newest first. None leaves, so the list only grows at its
/// head; a dropped stream's listener stays, and tells no one.
static LISTENERS: AtomicPtr<Listener> = AtomicPtr::new(ptr::null_mut());

/// Call `callback` with the number of each generation the program takes
/// from now, on the program's main thread, right after the swap. Under
/// cargo it is never called.
pub fn on_generation(callback: impl Fn(u64) + Send + Sync + 'static) {
    listen(Box::new(callback));
}

/// Tell `callback` each generation from now, when a runner runs this
/// process. `false`, with `callback` dropped, when none does.
pub(crate) fn listen(callback: Callback) -> bool {
    if !runner() {
        return false;
    }
    let listener = Box::into_raw(Box::new(Listener {
        callback,
        next: ptr::null_mut(),
    }));
    let mut head = LISTENERS.load(Ordering::Acquire);
    loop {
        // SAFETY: `listener` is the box made above, which no other thread
        // sees until the exchange below publishes it.
        unsafe { (*listener).next = head };
        match LISTENERS.compare_exchange_weak(head, listener, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(current) => head = current,
        }
    }
}

/// Whether a runner runs this process, having handed it [`notify`] once.
fn runner() -> bool {
    static LISTENING: OnceLock<bool> = OnceLock::new();
    *LISTENING.get_or_init(|| match runner_listen() {
        Some(listen) => {
            listen(notify);
            true
        }
        None => false,
    })
}

/// The runner's [`Listen`], when the variable names this process.
fn runner_listen() -> Option<Listen> {
    let value = std::env::var(VARIABLE).ok()?;
    let (process, address) = value.split_once(':')?;
    if process.parse::<u32>().ok()? != std::process::id() {
        return None;
    }
    let address = usize::from_str_radix(address, 16)
        .ok()
        .filter(|&address| address != 0)?;
    // SAFETY: the runner of this process wrote the address of its function
    // of `Listen`'s signature, which lives as long as the process.
    Some(unsafe { std::mem::transmute::<usize, Listen>(address) })
}

/// The runner's call for each generation the program takes.
extern "C" fn notify(generation: u64) {
    let mut listener = LISTENERS.load(Ordering::Acquire);
    // SAFETY: each listener is a box `listen` leaked, never freed, and
    // published whole before its pointer was.
    while let Some(current) = unsafe { listener.as_ref() } {
        (current.callback)(generation);
        listener = current.next;
    }
}
