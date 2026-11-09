//! tracing's callsite shape, small: a callsite static built with a const fn
//! from a metadata static, which registers by its `'static` address the
//! first time it is asked for its interest, as tracing-core's
//! `DefaultCallsite` does. The metadata points back at its callsite as a
//! `&dyn` trait object, as tracing's `Identifier` does. `counted!` declares
//! the two statics apart; `event!` declares them as tracing's `event!`
//! does, a cycle. `Note` keeps its callsite in an `Option`, a cycle rawr
//! refuses.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

/// The address of each callsite that registered, in order.
static REGISTRY: Mutex<Vec<usize>> = Mutex::new(Vec::new());

const UNREGISTERED: u8 = 0xFF;

/// What metadata points back at: a callsite, or nowhere.
trait Site: Sync {}

impl Site for Callsite {}

struct Nowhere;

impl Site for Nowhere {}

pub struct Metadata {
    name: &'static str,
    callsite: &'static dyn Site,
}

impl Metadata {
    /// Metadata that points back at its callsite, as tracing's does.
    pub const fn new(name: &'static str, callsite: &'static Callsite) -> Self {
        Self { name, callsite }
    }

    /// Metadata that names no callsite.
    pub const fn named(name: &'static str) -> Self {
        Self {
            name,
            callsite: &Nowhere,
        }
    }
}

/// A note of a callsite, which may name none.
pub struct Note {
    callsite: Option<&'static Callsite>,
}

impl Note {
    pub const fn new(callsite: &'static Callsite) -> Self {
        Self {
            callsite: Some(callsite),
        }
    }

    pub fn noted(&self) -> bool {
        self.callsite.is_some()
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
    let back = std::ptr::addr_eq(callsite.meta.callsite, callsite);
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

/// An event at its call site, with statics of its own at each call:
/// metadata first, then the callsite built from it.
#[macro_export]
macro_rules! counted {
    ($name:expr) => {{
        static META: $crate::Metadata = $crate::Metadata::named($name);
        static __CALLSITE: $crate::Callsite = $crate::Callsite::new(&META);
        let interest = __CALLSITE.interest();
        if interest.is_never() {
            String::new()
        } else {
            $crate::describe(&__CALLSITE)
        }
    }};
}

/// An event at its call site: tracing's `event!`, which names its callsite
/// static `__CALLSITE` at every call, and whose metadata points back at it.
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
