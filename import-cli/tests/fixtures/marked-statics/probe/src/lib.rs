//! tracing's callsite shape, small: `event!` declares a callsite static
//! whose initializer declares a metadata static that points back at it,
//! both built with const fns, and the callsite registers by its `'static`
//! address the first time it is asked for its interest, as tracing-core's
//! `DefaultCallsite` does.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

/// The address of each callsite that registered, in order.
static REGISTRY: Mutex<Vec<usize>> = Mutex::new(Vec::new());

const UNREGISTERED: u8 = 0xFF;

pub struct Metadata {
    name: &'static str,
    callsite: &'static Callsite,
}

impl Metadata {
    pub const fn new(name: &'static str, callsite: &'static Callsite) -> Self {
        Self { name, callsite }
    }
}

pub struct Callsite {
    interest: AtomicU8,
    meta: &'static Metadata,
}

#[derive(Clone, Copy)]
pub struct Interest(u8);

impl Interest {
    pub fn is_never(&self) -> bool {
        self.0 == 0
    }
}

impl Callsite {
    pub const fn new(meta: &'static Metadata) -> Self {
        Self {
            interest: AtomicU8::new(UNREGISTERED),
            meta,
        }
    }

    /// Registers the callsite the first time, by its address.
    pub fn interest(&'static self) -> Interest {
        match self.interest.load(Ordering::Acquire) {
            UNREGISTERED => {
                REGISTRY
                    .lock()
                    .unwrap()
                    .push(std::ptr::from_ref(self).addr());
                self.interest.store(1, Ordering::Release);
                Interest(1)
            }
            interest => Interest(interest),
        }
    }
}

/// Which registration `callsite` is, of how many, its metadata's name, and
/// whether its metadata points back at it.
pub fn describe(callsite: &'static Callsite) -> String {
    let registry = REGISTRY.lock().unwrap();
    let address = std::ptr::from_ref(callsite).addr();
    let index = registry
        .iter()
        .position(|registered| *registered == address);
    let back = std::ptr::eq(callsite.meta.callsite, callsite);
    match index {
        Some(index) => format!(
            "{} registered {} of {} back {back}",
            callsite.meta.name,
            index + 1,
            registry.len()
        ),
        None => format!("{} unregistered back {back}", callsite.meta.name),
    }
}

/// An event at its call site: tracing's `event!`, which names its callsite
/// static `__CALLSITE` at every call.
#[macro_export]
macro_rules! event {
    ($name:expr) => {{
    static __CALLSITE: $crate::Callsite = $crate::callsite!(name: $name);
        let interest = __CALLSITE.interest();
        if interest.is_never() {
            String::new()
        } else {
            $crate::describe(&__CALLSITE)
        }
    }};
}

/// tracing's `callsite2!`: a metadata static naming the caller's
/// `__CALLSITE`, and the callsite built from it.
#[macro_export]
macro_rules! callsite {
    (name: $name:expr) => {{
        static META: $crate::Metadata = $crate::Metadata::new($name, &__CALLSITE);
        $crate::Callsite::new(&META)
    }};
}

/// Calls `step` with 1, 2, 3 and so on and prints what it gives: every
/// 250 ms while the file `PROBE_STOP` names doesn't exist, or else
/// `PROBE_COUNT` times.
pub fn run<F: Fn(i64) -> String + 'static>(step: F) {
    let stop = std::env::var_os("PROBE_STOP");
    let count: i64 = std::env::var("PROBE_COUNT").map_or(3000, |count| count.parse().unwrap());
    for tick in 1..=count {
        if stop
            .as_ref()
            .is_some_and(|path| std::path::Path::new(path).exists())
        {
            break;
        }
        println!("{}", step(tick));
        if stop.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}
