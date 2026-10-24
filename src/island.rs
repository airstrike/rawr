//! Native islands at run time (D501): a program whose `main` is native Rust
//! and calls into marked modules.
//!
//! The runner links the program's shadow crate, whose trampolines stand for
//! the marked functions (`package::island`). Two functions defined here
//! with fixed symbol names are what the shadow crate calls, so it never
//! depends on this crate: [`__rawr_island_start_v1`] registers the native
//! `main`, and [`__rawr_island_call_v1`] runs one call of a marked function
//! in the current generation and returns its result in the wire format of
//! `program::model::island`.
//!
//! The interpreter lives on the runner's protocol thread. The runner
//! [`publish`]es each generation there, calls the program's Rawr `main`
//! (which registers the native one) and [`start`]s the native `main` on a
//! thread of its own. A call from that thread is handed to the protocol
//! thread as a [`Call`], which answers it between the runner's own messages:
//! a generation swapped in by a save is the one the next call runs in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};

use crate::program::model::island::Reader;
use crate::reload::installation::Bootstrap;
use crate::{item, program, type_def, Dynamic, ImmutableString};

/// The native `main` the program registered, until the runner starts it.
static START: Mutex<Option<fn() -> i32>> = Mutex::new(None);
/// Whether the native `main` runs.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Hands a call from a native thread to the thread that serves it.
#[allow(clippy::type_complexity)]
static SERVER: OnceLock<Box<dyn Fn(Call) + Send + Sync>> = OnceLock::new();

thread_local! {
    /// The generation native calls run in, on the thread that serves them.
    static CURRENT: RefCell<Option<Rc<Current>>> = const { RefCell::new(None) };
}

/// Register the native `main` to start: the shadow crate's island module
/// calls this from the program's Rawr `main`.
#[unsafe(no_mangle)]
pub fn __rawr_island_start_v1(entry: fn() -> i32) {
    if let Ok(mut start) = START.lock() {
        *start = Some(entry);
    }
}

/// Run one call of a marked function: `path` names it from the crate root
/// and `arguments` holds its header and values. The reply starts with `0`
/// and holds the result and each argument the call could change, or starts
/// with `1` and holds a message to panic with.
#[unsafe(no_mangle)]
pub fn __rawr_island_call_v1(path: &str, arguments: &[u8]) -> Vec<u8> {
    // On the thread that serves calls (native code called from marked code),
    // the call runs here and now.
    if let Some(current) = current() {
        return current.answer(path, arguments);
    }
    let Some(forward) = SERVER.get() else {
        return failure(&format!("rawr: no runner serves the call of `{path}`"));
    };
    let (reply, answer) = mpsc::sync_channel(1);
    forward(Call {
        path: path.into(),
        arguments: arguments.to_vec(),
        reply,
    });
    answer.recv().unwrap_or_else(|_| {
        failure(&format!(
            "rawr: the runner stopped before `{path}` returned"
        ))
    })
}

/// One call from native code waiting for the thread that serves it.
pub struct Call {
    path: Box<str>,
    arguments: Vec<u8>,
    reply: mpsc::SyncSender<Vec<u8>>,
}

impl Call {
    /// Run this call in the generation published on this thread and send
    /// its reply.
    pub fn answer(self) {
        let reply = match current() {
            Some(current) => current.answer(&self.path, &self.arguments),
            None => failure(&format!(
                "rawr: no generation is published for `{}`",
                self.path
            )),
        };
        let _ = self.reply.send(reply);
    }
}

/// Make `bootstrap` the generation native calls run in, on this thread,
/// which serves them from now on.
pub fn publish(bootstrap: Bootstrap) {
    let current = Rc::new(Current::new(bootstrap));
    CURRENT.with(|slot| *slot.borrow_mut() = Some(current));
}

/// Call the published generation's Rawr `fn main`. `None` when nothing is
/// published or it has no `main`.
pub fn invoke_main() -> Option<Result<Dynamic, program::Error>> {
    current()?.bootstrap.invoke_main()
}

/// Whether the program's native `main` runs, so each new generation must
/// be published for its calls.
#[must_use]
pub fn running() -> bool {
    RUNNING.load(Ordering::Acquire)
}

/// Start the native `main` the program registered, if it did: `false`
/// when it registered none. It runs on a thread of its own and the process
/// exits with its status when it returns (101 when it panics). Each call
/// it makes into marked code goes to `forward`, which hands it to this
/// thread to [`Call::answer`].
pub fn start(forward: impl Fn(Call) + Send + Sync + 'static) -> bool {
    let Some(entry) = START.lock().ok().and_then(|mut start| start.take()) else {
        return false;
    };
    if SERVER.set(Box::new(forward)).is_err() {
        return false;
    }
    RUNNING.store(true, Ordering::Release);
    let spawned = std::thread::Builder::new()
        .name("main".into())
        // The stack a process's own main thread usually has.
        .stack_size(8 << 20)
        .spawn(move || {
            let status = std::panic::catch_unwind(entry).unwrap_or(101);
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
            std::process::exit(status);
        });
    if let Err(error) = spawned {
        eprintln!("rawr: cannot start the program's main thread: {error}");
        std::process::exit(101);
    }
    true
}

fn current() -> Option<Rc<Current>> {
    CURRENT.with(|slot| slot.borrow().clone())
}

fn failure(message: &str) -> Vec<u8> {
    let mut reply = Vec::with_capacity(1 + message.len());
    reply.push(1);
    reply.extend_from_slice(message.as_bytes());
    reply
}

/// One published generation with what its calls look up, found once.
struct Current {
    bootstrap: Bootstrap,
    /// Each function by whether it belongs to a type, and its path.
    items: HashMap<(bool, String), item::Value>,
    types: program::model::island::Types,
}

impl Current {
    fn new(bootstrap: Bootstrap) -> Self {
        let authority = bootstrap.catalog().authority();
        let join = |modules: &[ImmutableString], names: &[&str]| {
            modules
                .iter()
                .map(ImmutableString::as_str)
                .chain(names.iter().copied())
                .collect::<Vec<_>>()
                .join("::")
        };
        let mut items = HashMap::new();
        for item in authority.items() {
            if !matches!(
                item.kind(),
                item::Kind::Function | item::Kind::AssociatedFunction | item::Kind::Method
            ) {
                continue;
            }
            let key = item.key();
            let entry = match key.owner() {
                None => (false, join(key.modules(), &[key.name()])),
                Some(type_def::Identity::Script(owner)) => {
                    (true, join(owner.modules(), &[owner.name(), key.name()]))
                }
                Some(type_def::Identity::Host(_)) => continue,
            };
            items.insert(entry, item.clone());
        }
        let types = authority.model().island_types();
        Self {
            bootstrap,
            items,
            types,
        }
    }

    fn answer(&self, path: &str, arguments: &[u8]) -> Vec<u8> {
        self.call(path, arguments)
            .unwrap_or_else(|message| failure(&message))
    }

    fn call(&self, path: &str, arguments: &[u8]) -> Result<Vec<u8>, String> {
        let malformed = |error: String| format!("rawr: the call of `{path}` is malformed: {error}");
        let mut reader = Reader { bytes: arguments };
        let owned = reader.byte().map_err(malformed)? != 0;
        let count = u32::from_le_bytes(reader.array().map_err(malformed)?) as usize;
        let mutable = u64::from_le_bytes(reader.array().map_err(malformed)?);
        let item = self.items.get(&(owned, path.to_owned())).ok_or_else(|| {
            format!("rawr: the running program has no function `{path}`; restart `rawr run`")
        })?;
        let catalog = self.bootstrap.catalog();
        let model = catalog.authority().model();
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(
                model
                    .island_read(&self.types, &mut reader, 0)
                    .map_err(malformed)?,
            );
        }
        let result = catalog
            .invoke_item(item, &mut values)
            .map_err(|error| format!("rawr: `{path}` failed: {error}"))?;
        let mut reply = vec![0];
        let unsent = |error: String| {
            format!("rawr: `{path}` gave back a value native code can't take: {error}")
        };
        model.island_write(&result, &mut reply, 0).map_err(unsent)?;
        for (index, value) in values.iter().enumerate() {
            if index < 64 && mutable & (1 << index) != 0 {
                model.island_write(value, &mut reply, 0).map_err(unsent)?;
            }
        }
        Ok(reply)
    }
}
