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
//! (which registers the native one) and [`start`]s the native `main`. It
//! runs on the process's main thread when the runner kept that thread for
//! it ([`keep_main_thread`]), as winit and other windowing crates need, and
//! on a thread of its own otherwise. A call from that thread is handed to
//! the protocol thread as a [`Call`], which answers it between the runner's
//! own messages: a generation swapped in by a save is the one the next call
//! runs in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};

use crate::program::model::island::Reader;
use crate::reload::installation::Bootstrap;
use crate::{item, program, type_def, Dynamic, ImmutableString};

pub mod lend;
pub mod place;

/// The native `main` the program registered, until the runner starts it.
static START: Mutex<Option<fn() -> i32>> = Mutex::new(None);
/// Whether the native `main` runs.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Hands the native `main` to the process's main thread, when the runner
/// kept that thread for it.
static MAIN_THREAD: OnceLock<mpsc::SyncSender<fn() -> i32>> = OnceLock::new();
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
    __rawr_island_call_v2(path, arguments, &[])
}

/// [`__rawr_island_call_v1`] with the arguments native code lends for the
/// call (D558.3), as ratatui's `&mut Frame<'_>`: each its key, its type's
/// name, its place, and the accessors the generated runner reaches it by,
/// a mutable one only when it is lent `&mut`. Only those accessors claim
/// the key. The trampoline's thread waits for the reply, and each place
/// stays borrowed until then.
///
/// [`__rawr_scope_claim_v1`] and [`__rawr_scope_release_v1`] are what the
/// accessors claim a lend by.
#[unsafe(no_mangle)]
pub fn __rawr_island_call_v2(
    path: &str,
    arguments: &[u8],
    lent: &[(
        std::any::TypeId,
        &'static str,
        *mut (),
        Option<crate::scope::Accessor>,
        crate::scope::Accessor,
    )],
) -> Vec<u8> {
    let lent = lent.iter().map(|&lent| Loan(lent)).collect::<Vec<_>>();
    // On the thread that serves calls (native code called from marked code),
    // the call runs here and now.
    if let Some(current) = current() {
        let _entry = crate::future::entry();
        return current.answer(path, arguments, &lent);
    }
    let Some(forward) = SERVER.get() else {
        return failure(&format!("rawr: no runner serves the call of `{path}`"));
    };
    let (reply, answer) = mpsc::sync_channel(1);
    forward(Call(Work::Native {
        path: path.into(),
        arguments: arguments.to_vec(),
        lent,
        reply,
    }));
    answer.recv().unwrap_or_else(|_| {
        failure(&format!(
            "rawr: the runner stopped before `{path}` returned"
        ))
    })
}

/// Work from another thread waiting for the thread that serves it: a call
/// from native code, or a step of an async body (`future::Job`).
pub struct Call(Work);

enum Work {
    Native {
        path: Box<str>,
        arguments: Vec<u8>,
        lent: Vec<Loan>,
        reply: mpsc::SyncSender<Vec<u8>>,
    },
    Step(crate::future::Job),
}

/// One argument a trampoline lends: its key, name, place and accessors.
#[derive(Clone, Copy)]
struct Loan(
    (
        std::any::TypeId,
        &'static str,
        *mut (),
        Option<crate::scope::Accessor>,
        crate::scope::Accessor,
    ),
);

/// Claim the lend `nonce` names under `key` for one access by a
/// trampoline's accessor, which ends it with [`__rawr_scope_release_v1`].
/// `Err` holds a [`crate::scope::Error`]'s code.
#[unsafe(no_mangle)]
pub fn __rawr_scope_claim_v1(
    nonce: u64,
    key: std::any::TypeId,
    shared: bool,
) -> Result<*mut (), u8> {
    crate::scope::claim(crate::scope::nonce(nonce), key, shared)
        .map(crate::scope::Claim::keep)
        .map_err(crate::scope::Error::code)
}

/// End an access [`__rawr_scope_claim_v1`] began.
#[unsafe(no_mangle)]
pub fn __rawr_scope_release_v1(nonce: u64, shared: bool) {
    crate::scope::end_claim(crate::scope::nonce(nonce), shared);
}

// SAFETY: a trampoline lends only a `Send` value mutably and a `Sync` value
// shared (`Call::lend` in the shadow crate's support), and its thread waits
// for the reply, so the place is reached by one thread at a time.
unsafe impl Send for Loan {}

impl Call {
    /// Run this call in the generation published on this thread and send
    /// its reply.
    pub fn answer(self) {
        match self.0 {
            Work::Native {
                path,
                arguments,
                lent,
                reply,
            } => {
                let _entry = crate::future::entry();
                let answer = match current() {
                    Some(current) => current.answer(&path, &arguments, &lent),
                    None => failure(&format!("rawr: no generation is published for `{path}`")),
                };
                let _ = reply.send(answer);
            }
            Work::Step(job) => job.run(),
        }
    }
}

/// Make `bootstrap` the generation native calls run in, on this thread,
/// which serves them from now on. Async steps run in it too (D513.3).
pub fn publish(bootstrap: Bootstrap) {
    let current = Rc::new(Current::new(bootstrap));
    crate::future::serve(current.clone());
    #[cfg(feature = "sync")]
    crate::future::publish(current.shared.clone());
    CURRENT.with(|slot| *slot.borrow_mut() = Some(current));
}

/// The future of the program's async `main`, which its runtime's entry
/// awaits (`__rawr_island_main_v1`).
#[cfg(feature = "sync")]
static MAIN: Mutex<Option<crate::future::Future>> = Mutex::new(None);

/// Hold `future`, the Rawr `main`'s, for the native entry its runtime's
/// attribute carries (D513.4). The runner then [`start`]s that entry.
#[cfg(feature = "sync")]
pub fn await_main(future: crate::future::Future) {
    if let Ok(mut main) = MAIN.lock() {
        *main = Some(future);
    }
}

/// The Rawr `main`'s future as the native entry awaits it: its status is
/// the process's, as `main`'s output gives Rust's (0, or 1 for an `Err`),
/// and 101 when it fails.
#[cfg(feature = "sync")]
#[unsafe(no_mangle)]
pub fn __rawr_island_main_v1() -> std::pin::Pin<Box<dyn std::future::Future<Output = i32> + Send>> {
    let Some(future) = MAIN.lock().ok().and_then(|mut main| main.take()) else {
        return Box::pin(async {
            eprintln!("rawr: the program's async `main` gave no future");
            101
        });
    };
    let mut script = future.into_script();
    // The entry's `block_on` polls this on the thread that owns the program
    // (D527): each poll runs the steps other threads queued, then `main`'s,
    // all in the runtime's context.
    Box::pin(std::future::poll_fn(move |cx| {
        crate::future::drive(cx.waker());
        let _entered = crate::future::enter();
        crate::future::drain();
        script.poll_output(cx).map(|output| match output {
            Ok(value) => match value.try_cast::<Result<Dynamic, Dynamic>>() {
                Some(Err(error)) => {
                    eprintln!("Error: {error:?}");
                    1
                }
                _ => 0,
            },
            Err(error) => {
                eprintln!("error: {}", crate::future::message(&error));
                101
            }
        })
    }))
}

/// Call the published generation's Rawr `fn main`. `None` when nothing is
/// published or it has no `main`.
pub fn invoke_main() -> Option<Result<Dynamic, program::Error>> {
    current()?.bootstrap.invoke_main()
}

/// Check `value`, which native code gave back where a script type was
/// erased to [`crate::Value`] (D513.2), against the exact type `witness`
/// proves, in the generation published on this thread.
pub fn validate(witness: &crate::host::runtime::Witness, value: &Dynamic) -> Result<(), String> {
    let current = current().ok_or("no generation is published on this thread")?;
    current
        .bootstrap
        .catalog()
        .validate_witness(witness, value)
        .map_err(|error| error.to_string())
}

/// Whether the program's native `main` runs, so each new generation must
/// be published for its calls.
#[must_use]
pub fn running() -> bool {
    RUNNING.load(Ordering::Acquire)
}

/// The runtime entry an async `main` registered (D513.4), for the thread
/// that owns the program to run: its `block_on` drives the program (D527).
pub fn take_entry() -> Option<fn() -> i32> {
    START.lock().ok().and_then(|mut start| start.take())
}

/// The process's main thread, kept for the program's native `main`.
///
/// A runner calls [`keep_main_thread`] on the main thread first, runs its
/// protocol on another thread, and then [`MainThread::serve`]s here.
pub struct MainThread(mpsc::Receiver<fn() -> i32>);

/// Keep the calling thread, which must be the process's main thread, for
/// the program's native `main`: [`start`] then hands it here instead of
/// spawning a thread. winit's event loop, and other windowing crates', must
/// run on the main thread.
#[must_use]
pub fn keep_main_thread() -> MainThread {
    let (send, receive) = mpsc::sync_channel(1);
    let _ = MAIN_THREAD.set(send);
    MainThread(receive)
}

impl MainThread {
    /// Wait for the native `main` and run it here. The process exits with
    /// its status when it returns. Another thread ends the process when no
    /// native `main` comes.
    pub fn serve(self) -> ! {
        match self.0.recv() {
            Ok(entry) => run(entry),
            Err(_) => loop {
                std::thread::park();
            },
        }
    }
}

/// Run the native `main` and exit with its status (101 when it panics).
fn run(entry: fn() -> i32) -> ! {
    let status = std::panic::catch_unwind(entry).unwrap_or(101);
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(status)
}

/// Start the native `main` the program registered, if it did: `false`
/// when it registered none. It runs on the process's main thread when the
/// runner kept it ([`keep_main_thread`]), and on a thread of its own
/// otherwise. The process exits with its status when it returns (101 when
/// it panics). Each call it makes into marked code goes to `forward`,
/// which hands it to this thread to [`Call::answer`].
pub fn start(forward: impl Fn(Call) + Send + Sync + 'static) -> bool {
    let Some(entry) = START.lock().ok().and_then(|mut start| start.take()) else {
        return false;
    };
    let forward = std::sync::Arc::new(forward);
    let steps = forward.clone();
    crate::future::forward(move |job| steps(Call(Work::Step(job))));
    if SERVER.set(Box::new(move |call| forward(call))).is_err() {
        return false;
    }
    RUNNING.store(true, Ordering::Release);
    if let Some(main_thread) = MAIN_THREAD.get()
        && main_thread.send(entry).is_ok()
    {
        return true;
    }
    let spawned = std::thread::Builder::new()
        .name("main".into())
        // The stack a process's own main thread usually has.
        .stack_size(8 << 20)
        .spawn(move || run(entry));
    if let Err(error) = spawned {
        eprintln!("rawr: cannot start the program's main thread: {error}");
        std::process::exit(101);
    }
    true
}

/// Run `body` with each of `lent` lent, outermost first, and the script
/// values that stand for them. Every lend ends before this returns.
fn lend_all<R>(
    lent: &[Loan],
    values: &mut Vec<crate::scope::Lent>,
    body: &mut dyn FnMut(&[crate::scope::Lent]) -> R,
) -> R {
    let Some((Loan((key, name, place, mutable, shared)), rest)) = lent.split_first() else {
        return body(values);
    };
    // SAFETY: the trampoline that sent this call made `place` from its
    // argument's borrow for the call (`Call::lend`): a `&mut` of a `Send`
    // value when it has a mutable accessor, and a `&` of a `Sync` one
    // otherwise. Its thread waits for the reply, which is made inside this
    // lend, and only its accessors claim `key`, reading the place as the
    // type it is.
    unsafe {
        crate::scope::lend_raw(*key, *place, mutable.is_some(), |nonce| {
            values.push(crate::scope::Lent::new(
                nonce,
                name,
                *mutable,
                Some(*shared),
            ));
            lend_all(rest, values, body)
        })
    }
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
    #[cfg(feature = "sync")]
    shared: crate::future::Program,
    /// Each function by whether it belongs to a type, and its path.
    items: HashMap<(bool, String), item::Value>,
    types: program::model::island::Types,
}

impl crate::future::Host for Current {
    fn invoke(
        &self,
        function: &item::Value,
        arguments: &mut [Dynamic],
    ) -> crate::RawrResultOf<Dynamic> {
        self.bootstrap
            .catalog()
            .invoke_item(function, arguments)
            .map_err(|error| crate::future::error(error.to_string()))
    }

    #[cfg(feature = "sync")]
    fn share(&self) -> Option<crate::future::Program> {
        Some(self.shared.clone())
    }
}

/// `authority` with `runtime` as any thread may use it, with each script
/// type's own `Clone` found once.
#[cfg(feature = "sync")]
pub(crate) fn shared(
    authority: program::Authority,
    runtime: crate::Shared<crate::host::runtime::Registry>,
) -> crate::future::Program {
    let clones = authority
        .items()
        .filter(|item| item.kind() == item::Kind::Method && item.key().name() == "Clone::clone")
        .filter_map(|item| Some((item.key().owner()?.clone(), item.clone())))
        .collect();
    let layouts = layouts(authority.items());
    std::sync::Arc::new(Shared {
        authority,
        runtime,
        clones,
        layouts,
    })
}

/// The function that writes a lifted closure, by package, module path and
/// name: its closures are named `{name}{closure#N}`. `None` for any other
/// item.
#[cfg(feature = "sync")]
fn enclosing(key: &item::Key) -> Option<String> {
    let at = key.name().find("{closure#")?;
    let mut path = String::from(key.package());
    for module in key.modules() {
        path.push_str("::");
        path.push_str(module);
    }
    path.push_str("::");
    path.push_str(&key.name()[..at]);
    Some(path)
}

/// The closures each function writes, in key order, found once for a
/// generation.
#[cfg(feature = "sync")]
fn layouts<'a>(
    items: impl Iterator<Item = &'a item::Value>,
) -> HashMap<String, Box<[item::Value]>> {
    let mut layouts = HashMap::<String, Vec<item::Value>>::new();
    for item in items {
        if let Some(enclosing) = enclosing(item.key()) {
            layouts.entry(enclosing).or_default().push(item.clone());
        }
    }
    layouts
        .into_iter()
        .map(|(enclosing, mut closures)| {
            closures.sort_by(|a, b| a.key().cmp(b.key()));
            (enclosing, closures.into_boxed_slice())
        })
        .collect()
}

/// Whether two generations' closures of one function are the same: the
/// same keys, each with the same signature and captures (D553.5). A
/// closure inserted before another renumbers it, and the other's body
/// would run a sibling's.
#[cfg(feature = "sync")]
fn same_closures(one: Option<&[item::Value]>, other: Option<&[item::Value]>) -> bool {
    match (one, other) {
        (Some(one), Some(other)) => {
            one.len() == other.len() && one.iter().zip(other).all(|(a, b)| a.same_item(b))
        }
        _ => false,
    }
}

/// The crate root's `fn main`, from `fn` to its closing brace, without its
/// comments and with each run of whitespace one space: an edit that only
/// moves or reflows it changes nothing. The closures it writes are part of
/// it, so an edit to one restarts too. `None` without a `fn main`.
#[cfg(feature = "sync")]
fn main_text(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let is_ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let start = (0..bytes.len()).find(|&at| {
        text.is_char_boundary(at)
            && text[at..].starts_with("fn")
            && (at == 0 || !is_ident(bytes[at - 1]))
            && text[at + 2..].trim_start().starts_with("main")
            && text[at + 2..].trim_start()[4..]
                .trim_start()
                .starts_with(['(', '<'])
            && bytes.get(at + 2).is_some_and(u8::is_ascii_whitespace)
    })?;
    let mut output = String::new();
    let mut depth = 0_usize;
    let mut at = start;
    let mut space = false;
    while at < bytes.len() {
        let rest = &text[at..];
        let byte = bytes[at];
        let skip = if rest.starts_with("//") {
            rest.find('\n').unwrap_or(rest.len())
        } else if let Some(comment) = rest.strip_prefix("/*") {
            comment.find("*/").map_or(rest.len(), |end| end + 4)
        } else {
            0
        };
        if skip > 0 {
            space = true;
            at += skip;
            continue;
        }
        if byte.is_ascii_whitespace() {
            space = true;
            at += 1;
            continue;
        }
        if space && !output.is_empty() {
            output.push(' ');
        }
        space = false;
        let length = if byte == b'"' || (byte == b'r' && rest[1..].starts_with(['"', '#'])) {
            // A string: its text counts as it is.
            let hashes = rest[usize::from(byte == b'r')..]
                .bytes()
                .take_while(|&b| b == b'#')
                .count();
            let open = usize::from(byte == b'r') + hashes + 1;
            if byte == b'r' || hashes > 0 {
                let close = format!("\"{}", "#".repeat(hashes));
                rest[open..]
                    .find(&close)
                    .map_or(rest.len(), |end| open + end + close.len())
            } else {
                let mut end = 1;
                while end < rest.len() && bytes[at + end] != b'"' {
                    end += if bytes[at + end] == b'\\' { 2 } else { 1 };
                }
                (end + 1).min(rest.len())
            }
        } else if byte == b'\'' && rest[1..].starts_with('\\') {
            rest[2..].find('\'').map_or(rest.len(), |end| end + 3)
        } else if byte == b'\'' && rest[1..].chars().nth(1) == Some('\'') {
            1 + rest[1..].chars().next().map_or(0, char::len_utf8) + 1
        } else {
            rest.chars().next().map_or(1, char::len_utf8)
        };
        output.push_str(&rest[..length]);
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(output);
                }
            }
            _ => {}
        }
        at += length;
    }
    Some(output)
}

/// Why `candidate` can't replace `current` while a marked `main` runs
/// (D553.6), in one line for `rawr run` to restart with. `None` when it
/// swaps in: a body edit, or a new function or type.
///
/// - A script type whose fields or variants changed: rawr can't yet see
///   which script values native code stores as `rawr::Value`.
/// - A callable a marked `main`'s own frame gave native code, which no
///   longer rebinds: `main` never runs again to give a new one.
/// - A program that never called from native code into script code: no
///   entry would ever admit the new generation.
#[cfg(feature = "sync")]
#[must_use]
pub fn refusal(
    current: &crate::reload::Generation,
    candidate: &crate::reload::Candidate,
) -> Option<String> {
    let path = |key: &type_def::Key| {
        key.modules()
            .iter()
            .map(ImmutableString::as_str)
            .chain([key.name()])
            .collect::<Vec<_>>()
            .join("::")
    };
    for (identity, fingerprint) in current.schemas().iter() {
        let type_def::Identity::Script(key) = identity else {
            continue;
        };
        match candidate.schemas().get(identity) {
            Some(next) if next == fingerprint => {}
            Some(_) => return Some(format!("`{}` changed its fields", path(key))),
            None => return Some(format!("`{}` was removed", path(key))),
        }
    }
    if !crate::future::called_from_native() {
        return Some("the source changed".to_owned());
    }
    // `main` runs once: an edit to it would need it to run again.
    if main_text(current.source().entry_source().text())
        != main_text(candidate.source().entry_source().text())
    {
        return Some("`main` changed".to_owned());
    }
    let held = crate::future::held();
    if held.is_empty() {
        return None;
    }
    let items = candidate
        .items()
        .map(|item| (item.key(), item))
        .collect::<HashMap<_, _>>();
    let closures = layouts(candidate.items());
    for (function, program) in held {
        let key = function.key();
        let same = items.get(key).is_some_and(|next| next.same_item(&function))
            && enclosing(key).is_none_or(|enclosing| {
                same_closures(
                    closures.get(&enclosing).map(|closures| &**closures),
                    program.closures(key),
                )
            });
        if !same {
            let name = key.name();
            return Some(match name.find("{closure#") {
                Some(at) => format!("a closure `{}` gave native code changed", &name[..at]),
                None => format!("`{name}`, which `main` gave native code, changed"),
            });
        }
    }
    None
}

/// The published program as any thread may use it (D490): calls with an
/// evaluation of their own, checks of values native code gives, and each
/// script type's own `Clone`.
#[cfg(feature = "sync")]
struct Shared {
    authority: program::Authority,
    runtime: crate::Shared<crate::host::runtime::Registry>,
    clones: HashMap<type_def::Identity, item::Value>,
    /// The closures each function writes (D553.5).
    layouts: HashMap<String, Box<[item::Value]>>,
}

#[cfg(feature = "sync")]
impl crate::future::Remote for Shared {
    fn invoke(
        &self,
        function: &item::Value,
        arguments: &mut [Dynamic],
    ) -> crate::RawrResultOf<Dynamic> {
        self.authority
            .invoke_bound(function, arguments, self.runtime.clone())
            .map_err(|error| crate::future::error(error.to_string()))
    }

    fn validate(
        &self,
        witness: &crate::host::runtime::Witness,
        value: &Dynamic,
    ) -> Result<(), String> {
        witness
            .ty()
            .validate(value, self.authority.model(), &self.runtime)
            .map_err(|error| error.to_string())
    }

    fn clone_value(&self, value: &Dynamic) -> Option<crate::RawrResultOf<Dynamic>> {
        let identity = match &value.0 {
            crate::types::dynamic::Union::Struct(value, ..) => value.identity(),
            crate::types::dynamic::Union::Enum(value, ..) => value.identity(),
            _ => return None,
        };
        let clone = self.clones.get(identity)?;
        Some(self.invoke(clone, &mut [value.clone()]))
    }

    fn debug(&self, value: &Dynamic) -> Option<String> {
        self.authority.model().debug_text(value)
    }

    fn rebinds(&self, function: &item::Value, from: &dyn crate::future::Remote) -> bool {
        let key = function.key();
        self.authority.rebind(function).is_ok()
            && (enclosing(key).is_none() || same_closures(self.closures(key), from.closures(key)))
    }

    fn closures(&self, closure: &item::Key) -> Option<&[item::Value]> {
        self.layouts
            .get(&enclosing(closure)?)
            .map(|closures| &**closures)
    }
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
        #[cfg(feature = "sync")]
        let shared = {
            let (authority, runtime) = bootstrap.catalog().shared();
            shared(authority, runtime)
        };
        Self {
            bootstrap,
            #[cfg(feature = "sync")]
            shared,
            items,
            types,
        }
    }

    fn answer(&self, path: &str, arguments: &[u8], lent: &[Loan]) -> Vec<u8> {
        lend_all(lent, &mut Vec::with_capacity(lent.len()), &mut |lent| {
            self.call(path, arguments, lent)
        })
        .unwrap_or_else(|message| failure(&message))
    }

    fn call(
        &self,
        path: &str,
        arguments: &[u8],
        lent: &[crate::scope::Lent],
    ) -> Result<Vec<u8>, String> {
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
                    .island_read(&self.types, lent, &mut reader, 0)
                    .map_err(malformed)?,
            );
        }
        let mut result = catalog
            .invoke_item(item, &mut values)
            .map_err(|error| format!("rawr: `{path}` failed: {error}"))?;
        // A function changes its first `&mut` parameter in place and copies
        // the others back out after its value, as `[value, ..copied]`; a
        // method or an associated function copies all but its receiver
        // (`resolve::lower_nominal::copied_parameters`). A lent `&mut` is
        // one of them, as `draw(frame: &mut Frame, app: &mut App)` changes
        // `frame` in place and copies `app` out.
        let references = values
            .iter()
            .enumerate()
            .filter(|(index, value)| {
                (*index < 64 && mutable & (1 << index) != 0)
                    || value
                        .read_lock::<crate::scope::Lent>()
                        .is_some_and(|lent| lent.writable())
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let copied = match item.kind() {
            item::Kind::Function => references.get(1..).unwrap_or_default(),
            item::Kind::Method => references
                .iter()
                .position(|index| *index != 0)
                .map_or(&[][..], |at| &references[at..]),
            _ => &references[..],
        };
        if !copied.is_empty() {
            let returned = result
                .into_array()
                .ok()
                .filter(|returned| returned.len() == copied.len() + 1)
                .ok_or_else(|| {
                    format!("rawr: `{path}` gave back no copies of its `&mut` arguments")
                })?;
            let mut returned = returned.into_iter();
            result = returned.next().unwrap_or(Dynamic::UNIT);
            for (index, value) in copied.iter().zip(returned) {
                values[*index] = value;
            }
        }
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

#[cfg(all(test, feature = "sync"))]
mod tests {
    use crate::{host, Dynamic, Engine, Shared};

    /// A value native code lent a call can't go back to native code in the
    /// result or in an argument it takes back, which would keep the borrow
    /// past the call (D558.3).
    #[test]
    fn a_lent_value_given_back_is_refused_by_the_borrow_rule() {
        let engine = Shared::new(Engine::new());
        let authority = engine
            .compile_authority("pub fn keep() {}")
            .unwrap_or_else(|error| panic!("{error}"));
        let lent = crate::scope::Lent::new(crate::scope::nonce(1), "ratatui::Frame", None, None);
        for value in [
            Dynamic::from(lent),
            Dynamic::from_array(vec![Dynamic::from(1_i64), Dynamic::from(lent)]),
        ] {
            let error = authority
                .model()
                .island_write(&value, &mut Vec::new(), 0)
                .unwrap_err();
            assert!(
                error.contains("keeps the `ratatui::Frame` native code lent it")
                    && error.contains("(E0515, E0521)"),
                "{error}"
            );
        }
    }

    /// Native code that clones or prints a script value it holds as
    /// `rawr::Value` gets the script type's own `Clone` and its derived
    /// `Debug` text (review 8).
    #[test]
    fn a_value_clones_and_prints_as_its_script_type_does() {
        let engine = Shared::new(Engine::new());
        let authority = engine
            .compile_authority(
                r#"
                struct Counter { n: i64 }
                impl Clone for Counter {
                    fn clone(&self) -> Self { Counter { n: self.n + 1 } }
                }
                #[derive(Debug, Clone)]
                struct Job { id: i64, name: String, tags: Vec<String>, next: Option<Step> }
                #[derive(Debug, Clone)]
                enum Step { Wait(i64), Done { at: i64 } }
                pub fn counter() -> Counter { Counter { n: 1 } }
                pub fn job() -> Job {
                    Job { id: 7, name: "seven".to_string(), tags: vec!["a".to_string()], next: Some(Step::Done { at: 3 }) }
                }
                pub fn count(counter: Counter) -> i64 { counter.n }
                "#,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let runtime = Shared::new(host::runtime::Registry::new());
        let call = |name: &str, arguments: &mut [Dynamic]| {
            let item = authority
                .items()
                .find(|item| item.key().name() == name)
                .unwrap()
                .clone();
            authority
                .invoke_bound(&item, arguments, runtime.clone())
                .unwrap_or_else(|error| panic!("{error}"))
        };
        let counter = call("counter", &mut []);
        let job = call("job", &mut []);
        crate::future::publish(super::shared(authority.clone(), runtime.clone()));
        let copy = crate::Value::from(counter).clone();
        assert_eq!(call("count", &mut [copy.into_inner()]).as_int().unwrap(), 2);
        assert_eq!(
            format!("{:?}", crate::Value::from(job)),
            r#"Job { id: 7, name: "seven", tags: ["a"], next: Some(Done { at: 3 }) }"#
        );
    }
}

/// A generation compiled on the protocol thread is admitted on the thread
/// that owns the program (D553.9, check 2): what a compiled program holds
/// moves between threads.
#[cfg(all(test, feature = "sync"))]
#[test]
fn a_compiled_program_moves_to_the_owner_thread() {
    fn moves<T: Send>() {}
    moves::<crate::Shared<crate::Engine>>();
    moves::<crate::AST>();
    moves::<crate::reload::Candidate>();
    moves::<std::sync::Arc<crate::reload::Generation>>();
    moves::<crate::host::facade::Definition>();
    moves::<crate::package::import::runner::account::Observation>();
}

/// What of `main` a running program compares: its text without comments
/// or layout, strings and characters as written, up to its closing brace.
#[cfg(all(test, feature = "sync"))]
#[test]
fn main_text_ignores_layout_and_comments_and_stops_at_its_brace() {
    let one = "// rawr\nuse x::y;\n\nfn main() {\n    // a comment\n    let s = \"}\";\n    let c = '}';\n    run(|t| { t + 1 }, 40);\n}\n\nfn step() {}\n";
    let moved = "// rawr\nuse x::y;\nfn helper() -> i64 { 2 }\n\nfn  main()  {\n    let s = \"}\"; /* note */\n    let c = '}';\n    run(|t| {\n        t + 1\n    }, 40);\n}\n";
    let edited = one.replace("40", "50");
    let text = main_text(one).unwrap();
    assert!(text.starts_with("fn main() {"), "{text}");
    assert!(text.ends_with("40); }"), "{text}");
    assert_eq!(main_text(moved), main_text(one));
    assert_ne!(main_text(&edited), main_text(one));
    assert_eq!(main_text("fn domain() {}"), None);
}
