//! Native islands at run time (D501): a program whose `main` is native Rust
//! and calls into marked modules.
//!
//! The runner links the program's shadow crate, whose trampolines stand for
//! the marked functions (`package::island`). Functions defined here with
//! fixed symbol names are what the shadow crate calls, so it never depends
//! on this crate: [`__rawr_island_start_v1`] registers the native `main`,
//! and [`__rawr_island_call_v4`] runs one call of a marked function in the
//! current generation and returns its result in the wire format of
//! `program::model::island`.
//!
//! The thread that owns the program is the process's main thread, as under
//! `cargo run`: the runner [`publish`]es the first generation there, calls
//! the program's Rawr `main` (which registers the native one) and runs the
//! native `main` there, from [`take_entry`]. A call it makes runs the marked
//! code on that same thread, so a marked function can start winit's event
//! loop, which must run on the main thread (#1119). The runner's protocol
//! goes on beside it and offers each saved generation to that thread, which
//! admits it at its next call from native code (D553.3). A call from any
//! other thread runs there too, with an evaluation of its own (D490), in
//! the generation published last.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

use crate::program::model::island::{Natives, Reader};
use crate::reload::installation::Bootstrap;
use crate::{item, program, type_def, Dynamic, ImmutableString};

pub mod lend;
pub mod place;

/// The native `main` the program registered, for the runner to start.
static START: Mutex<Option<fn() -> i32>> = Mutex::new(None);
/// How the runner moves a native value out of the script value that holds
/// it, which [`moves`] names.
static MOVE: OnceLock<Move> = OnceLock::new();

/// The generation that calls from threads that don't own the program run
/// in, with what they look up in it.
#[cfg(feature = "sync")]
static ELSEWHERE: std::sync::RwLock<Option<(Arc<Calls>, Arc<Shared>)>> =
    std::sync::RwLock::new(None);

thread_local! {
    /// The generation native calls run in, on the thread that owns the
    /// program.
    static CURRENT: RefCell<Option<Rc<Current>>> = const { RefCell::new(None) };
}

/// How the runner keeps a native value native code gives a marked function,
/// which [`lodges`] names.
static LODGE: OnceLock<Lodge> = OnceLock::new();

/// The native value a script value holds, moved out of the runner's arena
/// as the value's last copy moves it; `None` when it holds none, and `Err`
/// when it can't be reached from this thread.
pub type Move = fn(&Dynamic) -> Option<Result<Box<dyn Any>, String>>;

/// Name how the runner moves a native value out of the script value that
/// holds it, so a marked function can give native code a value of a native
/// type, as the `iced::Error` of an `iced::Result` (#1118). Set once.
pub fn moves(moved: Move) {
    let _ = MOVE.set(moved);
}

/// The native value `value` holds, moved out for native code.
pub(crate) fn moved(value: &Dynamic) -> Option<Result<Box<dyn Any>, String>> {
    MOVE.get()?(value)
}

/// What a trampoline finds of a value's type, for the runner to keep it as
/// it keeps a value it made: its `Debug` and `Display` text, its `Clone`,
/// `PartialEq`, `Hash` and `PartialOrd`, each when the type has it.
pub type Traits = (
    Option<fn(&dyn Any) -> Option<String>>,
    Option<fn(&dyn Any) -> Option<String>>,
    Option<fn(&dyn Any) -> Option<Box<dyn Any>>>,
    Option<fn(&dyn Any, &dyn Any) -> Option<bool>>,
    Option<fn(&dyn Any) -> Option<u64>>,
    Option<fn(&dyn Any, &dyn Any) -> Option<std::cmp::Ordering>>,
);

/// A native value native code gives a marked function by value (#1139):
/// the value, its type's name and its [`Traits`].
pub type Given = (Box<dyn Any>, &'static str, Traits);

/// The script value that holds a [`Given`] value, kept in the runner's
/// arena on this thread as a value of the type the witness proves; `Err`
/// says why it can't be.
pub type Lodge = fn(Given, Option<&crate::host::runtime::Witness>) -> Result<Dynamic, String>;

/// Name how the runner keeps a native value native code gives a marked
/// function. Set once.
pub fn lodges(lodge: Lodge) {
    let _ = LODGE.set(lodge);
}

/// The script value that holds `given`, kept by the runner as a value of
/// the type `witness` proves.
pub(crate) fn lodged(
    given: Given,
    witness: Option<&crate::host::runtime::Witness>,
) -> Result<Dynamic, String> {
    let lodge = LODGE
        .get()
        .ok_or("no runner keeps native values on this thread")?;
    lodge(given, witness)
}

/// What makes a native trait's object of a program value (#1264): given
/// the trait's path from the crate's root and the value's handle, a box of
/// the stand-in rawr's copy of the crate declares for the value's type,
/// which then owns the handle; `None` when the stand-in's impl is of
/// another trait.
pub type Maker = fn(&str, u64) -> Option<Given>;

/// The stand-ins' makers each crate registered, by their type's path.
static MAKERS: Mutex<Vec<(&'static str, Maker)>> = Mutex::new(Vec::new());

/// The program values native code holds as a native trait's object, by
/// their handle (#1264).
#[derive(Default)]
struct Kept {
    next: u64,
    values: HashMap<u64, Dynamic>,
}

#[cfg(feature = "sync")]
static KEPT: Mutex<Option<Kept>> = Mutex::new(None);

#[cfg(not(feature = "sync"))]
thread_local! {
    static KEPT: RefCell<Option<Kept>> = const { RefCell::new(None) };
}

/// `change` of the kept program values.
fn kept<R>(change: impl FnOnce(&mut Kept) -> R) -> R {
    #[cfg(feature = "sync")]
    {
        let mut kept = KEPT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        change(kept.get_or_insert_with(Kept::default))
    }
    #[cfg(not(feature = "sync"))]
    KEPT.with_borrow_mut(|kept| change(kept.get_or_insert_with(Kept::default)))
}

/// Register the makers of a crate's stand-ins, each by its type's path
/// from the crate's root: the shadow crate's island module calls this
/// before its first call into marked code.
#[unsafe(no_mangle)]
pub fn __rawr_island_objects_v1(makers: &[(&'static str, Maker)]) {
    MAKERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend_from_slice(makers);
}

/// Let go of the program value a stand-in held: its box was dropped.
#[unsafe(no_mangle)]
pub fn __rawr_island_release_v1(handle: u64) {
    // Dropped once the table is free again.
    let released = kept(|kept| kept.values.remove(&handle));
    drop(released);
}

/// `value`, of the program's type at `owner`, as the native trait object
/// `Box<dyn trait_name>`: the value is kept under a handle, and the
/// stand-in rawr's copy of the crate declares for `owner` holds it and
/// calls the impl's functions through it, in the generation of each call.
/// `trait_name` is the trait's path from the crate's root.
pub(crate) fn object(value: Dynamic, owner: &str, trait_name: &str) -> Result<Dynamic, String> {
    let makers = MAKERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|(path, _)| *path == owner)
        .map(|(_, maker)| *maker)
        .collect::<Vec<_>>();
    let handle = kept(|kept| {
        kept.next += 1;
        kept.values.insert(kept.next, value);
        kept.next
    });
    // The crate's own path of the trait, without the crate's name, which
    // the stand-in's impl compares with its own.
    let below = trait_name
        .split_once("::")
        .map_or(trait_name, |(_, below)| below);
    match makers.iter().find_map(|maker| maker(below, handle)) {
        Some(given) => lodged(given, None),
        None => {
            __rawr_island_release_v1(handle);
            Err(format!(
                "rawr: native code has no stand-in for `{owner}` as a `dyn {trait_name}`; \
                 restart `rawr run`"
            ))
        }
    }
}

/// The program value of `handle` out of the table, for one call of the
/// impl's function it is the receiver of.
fn take_kept(handle: u64) -> Result<Dynamic, String> {
    kept(|kept| kept.values.remove(&handle)).ok_or_else(|| {
        "rawr: a native trait object's program value is in use by a call it made, or was dropped"
            .to_owned()
    })
}

/// Put the program value of `handle` back after the call it was taken for.
fn put_kept(handle: u64, value: Dynamic) {
    kept(|kept| kept.values.insert(handle, value));
}

/// Register the native `main` to start: the shadow crate's island module
/// calls this from the program's Rawr `main`.
#[unsafe(no_mangle)]
pub fn __rawr_island_start_v1(entry: fn() -> i32) {
    if let Ok(mut start) = START.lock() {
        *start = Some(entry);
    }
}

/// Run one call of a marked function on this thread: `path` names it from
/// the crate root, `arguments` holds its header and values, `given` the
/// native values it takes by value, which the runner keeps, and `lent` what
/// native code lends the call (D558.3), as ratatui's `&mut Frame<'_>`: each
/// its key, its type's name, its place, and the accessors the generated
/// runner reaches it by, a mutable one only when it is lent `&mut`. Only
/// those accessors claim the key, and each place stays borrowed while the
/// call runs.
///
/// The reply starts with `0` and holds the result and each argument the
/// call could change, each native value among them moved to `natives`, or
/// starts with `1` and holds a message to panic with.
///
/// [`__rawr_scope_claim_v1`] and [`__rawr_scope_release_v1`] are what the
/// accessors claim a lend by.
#[unsafe(no_mangle)]
pub fn __rawr_island_call_v4(
    path: &str,
    arguments: &[u8],
    given: &mut [Option<Given>],
    lent: &[(
        std::any::TypeId,
        &'static str,
        *mut (),
        Option<crate::scope::Accessor>,
        crate::scope::Accessor,
    )],
    natives: &mut Natives,
) -> Vec<u8> {
    let lent = lent.iter().map(|&lent| Loan(lent)).collect::<Vec<_>>();
    if current().is_some() {
        // The generation offered last is admitted first, so this call
        // runs in it.
        let _entry = crate::future::trampoline();
        if let Some(current) = current() {
            return current.answer(path, arguments, given, &lent, natives);
        }
    }
    #[cfg(feature = "sync")]
    if let Some((calls, shared)) = elsewhere() {
        let _entry = crate::future::entry();
        let invoke = |function: &item::Value, arguments: &mut [Dynamic]| {
            shared
                .authority
                .invoke_bound(function, arguments, shared.runtime.clone())
                .map_err(|error| error.to_string())
        };
        let call = Call {
            path,
            arguments,
            model: shared.authority.model(),
            invoke: &invoke,
        };
        return calls.answer(&call, given, &lent, natives);
    }
    failure(&format!("rawr: no runner serves the call of `{path}`"))
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

/// Make `bootstrap` the generation native calls run in, on this thread,
/// which owns the program from now on, and on every other thread. Async
/// steps run in it too (D513.3).
pub fn publish(bootstrap: Bootstrap) {
    let current = Rc::new(Current::new(bootstrap));
    crate::future::serve(current.clone());
    #[cfg(feature = "sync")]
    {
        crate::future::publish(current.shared.clone());
        *ELSEWHERE
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((current.calls.clone(), current.shared.clone()));
    }
    CURRENT.with(|slot| *slot.borrow_mut() = Some(current));
}

/// The future of the program's async `main`, which its runtime's entry
/// awaits (`__rawr_island_main_v1`).
#[cfg(feature = "sync")]
static MAIN: Mutex<Option<crate::future::Future>> = Mutex::new(None);

/// Hold `future`, the Rawr `main`'s, for the native entry its runtime's
/// attribute carries (D513.4). The runner then runs that entry, from
/// [`take_entry`].
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

/// The entry the program registered for the thread that owns it to run:
/// its native `main` (D501), or the runtime entry an async `main` gave
/// (D513.4), whose `block_on` drives the program (D527).
pub fn take_entry() -> Option<fn() -> i32> {
    START.lock().ok().and_then(|mut start| start.take())
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
    // SAFETY: the trampoline that made this call made `place` from its
    // argument's borrow for the call (`Call::lend`), on this thread, which
    // runs the call. The reply is made inside this lend, and only its
    // accessors claim `key`, reading the place as the type it is.
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

/// One published generation, on the thread that owns the program.
struct Current {
    bootstrap: Bootstrap,
    #[cfg(feature = "sync")]
    shared: Arc<Shared>,
    calls: Arc<Calls>,
}

/// What a generation's calls from native code look up, found once: each
/// function by whether it belongs to a type, and its path, and the crate's
/// script types.
struct Calls {
    items: HashMap<(bool, String), item::Value>,
    types: program::model::island::Types,
    /// What each native value a function takes by value is attested as.
    inputs: HashMap<(bool, String), Box<[Option<crate::host::runtime::Witness>]>>,
}

/// One call from native code: the function's path and the arguments its
/// trampoline wrote, with the generation's model and how it invokes a
/// function.
struct Call<'a> {
    path: &'a str,
    arguments: &'a [u8],
    model: &'a program::model::Model,
    invoke: &'a Invoke<'a>,
}

/// How a generation invokes one of its functions with its arguments.
type Invoke<'a> = dyn Fn(&item::Value, &mut [Dynamic]) -> Result<Dynamic, String> + 'a;

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
        Some(self.shared.clone() as crate::future::Program)
    }
}

/// `authority` with `runtime` as any thread may use it, with each script
/// type's own `Clone` found once, as a test calls it.
#[cfg(all(test, feature = "sync"))]
pub(crate) fn shared(
    authority: program::Authority,
    runtime: crate::Shared<crate::host::runtime::Registry>,
) -> crate::future::Program {
    share(authority, runtime)
}

/// `authority` with `runtime` as any thread may use it, with each script
/// type's own `Clone` found once.
#[cfg(feature = "sync")]
fn share(
    authority: program::Authority,
    runtime: crate::Shared<crate::host::runtime::Registry>,
) -> Arc<Shared> {
    let methods = |name: &str| {
        authority
            .items()
            .filter(|item| item.kind() == item::Kind::Method && item.key().name() == name)
            .filter_map(|item| Some((item.key().owner()?.clone(), item.clone())))
            .collect::<HashMap<_, _>>()
    };
    let clones = methods("Clone::clone");
    let equals = methods("PartialEq::eq");
    let displays = methods("Display::fmt");
    let defaults = authority
        .items()
        .filter(|item| {
            item.kind() == item::Kind::AssociatedFunction && item.key().name() == "Default::default"
        })
        .filter_map(|item| match item.key().owner()? {
            type_def::Identity::Script(owner) => {
                let mut path = owner.modules().join("::");
                if !path.is_empty() {
                    path.push_str("::");
                }
                path.push_str(owner.name());
                // Keyed as the runner names the type (`crossing_path`).
                Some((authority.model().crossing_path(&path), item.clone()))
            }
            type_def::Identity::Host(_) => None,
        })
        .collect();
    let members = authority
        .items()
        .filter(|item| {
            matches!(
                item.kind(),
                item::Kind::Method | item::Kind::AssociatedFunction
            ) && item.key().name().contains("::")
        })
        .filter_map(|item| match item.key().owner()? {
            type_def::Identity::Script(owner) => {
                let mut path = owner.modules().join("::");
                if !path.is_empty() {
                    path.push_str("::");
                }
                path.push_str(owner.name());
                let mut path = authority.model().crossing_path(&path);
                path.push_str("::");
                path.push_str(item.key().name());
                Some((path, item.clone()))
            }
            type_def::Identity::Host(_) => None,
        })
        .collect();
    let layouts = layouts(authority.items());
    Arc::new(Shared {
        authority,
        runtime,
        clones,
        equals,
        displays,
        defaults,
        members,
        layouts,
    })
}

/// The script type of `value`, a script struct or enum.
#[cfg(feature = "sync")]
fn script_identity(value: &Dynamic) -> Option<&type_def::Identity> {
    match &value.0 {
        crate::types::dynamic::Union::Struct(value, ..) => Some(value.identity()),
        crate::types::dynamic::Union::Enum(value, ..) => Some(value.identity()),
        _ => None,
    }
}

/// The function that writes a lifted closure, by its path, or a method's
/// owner and name: its closures are named `{name}{closure#N}`. `None` for
/// any other item.
#[cfg(feature = "sync")]
fn enclosing(key: &item::Key) -> Option<String> {
    let mut path = key.to_string();
    let at = path.find("{closure#")?;
    path.truncate(at);
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
    /// Each script type's own `PartialEq::eq`.
    equals: HashMap<type_def::Identity, item::Value>,
    /// Each script type's own `Display::fmt`.
    displays: HashMap<type_def::Identity, item::Value>,
    /// Each script type's own `impl Default`, by its path (D594).
    defaults: HashMap<String, item::Value>,
    /// Each function of a script type's trait impls, by its owner's path
    /// and the member (`Arc::Program<Message>::draw`), which a runner's
    /// impl for the owner's proxy calls (D594).
    members: HashMap<String, item::Value>,
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
        let clone = self.clones.get(script_identity(value)?)?;
        Some(self.invoke(clone, &mut [value.clone()]))
    }

    fn debug(&self, value: &Dynamic) -> Option<String> {
        self.authority.model().debug_text(value)
    }

    fn equal(&self, lhs: &Dynamic, rhs: &Dynamic) -> Option<crate::RawrResultOf<bool>> {
        let identity = script_identity(lhs)?;
        let Some(equal) = self.equals.get(identity) else {
            return Some(Ok(crate::host::value::structural_equal(lhs, rhs)));
        };
        Some(
            self.invoke(equal, &mut [lhs.clone(), rhs.clone()])
                .and_then(|equal| {
                    equal
                        .as_bool()
                        .map_err(|actual| crate::future::error(format!("rawr: `eq` gave {actual}")))
                }),
        )
    }

    fn display(&self, value: &Dynamic) -> Option<crate::RawrResultOf<String>> {
        let display = self.displays.get(script_identity(value)?)?;
        Some(
            self.invoke(
                display,
                &mut [value.clone(), Dynamic::from(ImmutableString::new())],
            )
            .map(|text| text.to_string()),
        )
    }

    fn default_of(&self, path: &str) -> Option<crate::RawrResultOf<Dynamic>> {
        if let Some(default) = self.defaults.get(path) {
            return Some(self.invoke(default, &mut []));
        }
        self.authority.model().default_by_path(path).map(|made| {
            made.map_err(|error| {
                crate::future::error(format!("rawr: `{path}`'s `Default`: {error}"))
            })
        })
    }

    fn member(&self, path: &str) -> Option<item::Value> {
        self.members.get(path).cloned()
    }

    fn member_witnesses(&self, path: &str) -> Vec<Option<crate::host::runtime::Witness>> {
        self.authority
            .model()
            .member_types(path)
            .unwrap_or_default()
            .iter()
            .map(|ty| ty.as_ref().map(|ty| self.runtime.witness_for(ty)))
            .collect()
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
        let calls = Arc::new(Calls::new(authority, bootstrap.catalog().runtime()));
        #[cfg(feature = "sync")]
        let shared = {
            let (authority, runtime) = bootstrap.catalog().shared();
            share(authority, runtime)
        };
        Self {
            bootstrap,
            #[cfg(feature = "sync")]
            shared,
            calls,
        }
    }

    fn answer(
        &self,
        path: &str,
        arguments: &[u8],
        given: &mut [Option<Given>],
        lent: &[Loan],
        natives: &mut Natives,
    ) -> Vec<u8> {
        let catalog = self.bootstrap.catalog();
        let invoke = |function: &item::Value, arguments: &mut [Dynamic]| {
            catalog
                .invoke_item(function, arguments)
                .map_err(|error| error.to_string())
        };
        let call = Call {
            path,
            arguments,
            model: catalog.authority().model(),
            invoke: &invoke,
        };
        self.calls.answer(&call, given, lent, natives)
    }
}

/// The calls from threads that don't own the program, in the generation
/// published last.
#[cfg(feature = "sync")]
fn elsewhere() -> Option<(Arc<Calls>, Arc<Shared>)> {
    ELSEWHERE.read().ok()?.clone()
}

impl Calls {
    fn new(
        authority: &program::Authority,
        runtime: &crate::Shared<crate::host::runtime::Registry>,
    ) -> Self {
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
        let inputs = authority
            .model()
            .island_inputs()
            .iter()
            .map(|(key, types)| {
                let witnesses = types
                    .iter()
                    .map(|ty| ty.as_ref().map(|ty| runtime.witness_for(ty)))
                    .collect();
                (key.clone(), witnesses)
            })
            .collect();
        Self {
            items,
            types: authority.model().island_types(),
            inputs,
        }
    }

    fn answer(
        &self,
        call: &Call<'_>,
        given: &mut [Option<Given>],
        lent: &[Loan],
        natives: &mut Natives,
    ) -> Vec<u8> {
        lend_all(lent, &mut Vec::with_capacity(lent.len()), &mut |lent| {
            self.call(call, given, lent, natives)
        })
        .unwrap_or_else(|message| failure(&message))
    }

    fn call(
        &self,
        call: &Call<'_>,
        given: &mut [Option<Given>],
        lent: &[crate::scope::Lent],
        natives: &mut Natives,
    ) -> Result<Vec<u8>, String> {
        let path = call.path;
        let malformed = |error: String| format!("rawr: the call of `{path}` is malformed: {error}");
        let mut reader = Reader {
            bytes: call.arguments,
        };
        let owned = reader.byte().map_err(malformed)? != 0;
        let count = u32::from_le_bytes(reader.array().map_err(malformed)?) as usize;
        let mutable = u64::from_le_bytes(reader.array().map_err(malformed)?);
        let item = self.items.get(&(owned, path.to_owned())).ok_or_else(|| {
            format!("rawr: the running program has no function `{path}`; restart `rawr run`")
        })?;
        let model = call.model;
        let inputs = self.inputs.get(&(owned, path.to_owned()));
        let mut values = Vec::with_capacity(count);
        // A stand-in's receiver is its program value, taken for the call
        // (#1264).
        let mut held = Vec::new();
        for index in 0..count {
            if reader.bytes.first() == Some(&program::model::island::HANDLE) {
                reader.byte().map_err(malformed)?;
                let handle = u64::from_le_bytes(reader.array().map_err(malformed)?);
                let value = match take_kept(handle) {
                    Ok(value) => value,
                    Err(message) => {
                        restore(&held, &mut values);
                        return Err(message);
                    }
                };
                held.push((index, handle));
                values.push(value);
                continue;
            }
            let witness = inputs.and_then(|inputs| inputs.get(index)?.as_ref());
            match model.island_read(&self.types, lent, given, witness, &mut reader, 0) {
                Ok(value) => values.push(value),
                Err(error) => {
                    restore(&held, &mut values);
                    return Err(malformed(error));
                }
            }
        }
        // A function changes its first `&mut` parameter in place and copies
        // the others back out after its value, as `[value, ..copied]`; a
        // method or an associated function copies all but its receiver
        // (`resolve::lower_nominal::copied_parameters`). A lent `&mut` is
        // one of them, as `draw(frame: &mut Frame, app: &mut App)` changes
        // `frame` in place and copies `app` out, and `draw(frame: &mut
        // Frame, state: &mut ListState)` copies out the lent `state`. They
        // are found before the call, which moves its arguments out.
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
        let invoked = (call.invoke)(item, &mut values);
        // The receiver changed in place, as a method's is, goes back.
        restore(&held, &mut values);
        let mut result = invoked.map_err(|error| format!("rawr: `{path}` failed: {error}"))?;
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
        model
            .island_write(&result, &mut reply, natives, 0)
            .map_err(unsent)?;
        for (index, value) in values.iter().enumerate() {
            if index < 64 && mutable & (1 << index) != 0 {
                model
                    .island_write(value, &mut reply, natives, 0)
                    .map_err(unsent)?;
            }
        }
        // Then each `&mut` native code lent: `()` when its place is still
        // the lent one, and else the value `*place = value` gave it, which
        // moves to native code (#1139).
        for index in references
            .iter()
            .filter(|index| **index >= 64 || mutable & (1 << **index) == 0)
        {
            let value = &values[*index];
            if value.read_lock::<crate::scope::Lent>().is_some() {
                reply.push(program::model::island::UNIT);
            } else {
                model
                    .island_write(value, &mut reply, natives, 0)
                    .map_err(unsent)?;
            }
        }
        Ok(reply)
    }
}

/// Put each stand-in's receiver in `values` back under its handle.
fn restore(held: &[(usize, u64)], values: &mut [Dynamic]) {
    for &(index, handle) in held {
        if let Some(value) = values.get_mut(index) {
            put_kept(handle, value.take());
        }
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
                .island_write(&value, &mut Vec::new(), &mut Vec::new(), 0)
                .unwrap_err();
            assert!(
                error.contains("keeps the `ratatui::Frame` native code lent it")
                    && error.contains("(E0515, E0521)"),
                "{error}"
            );
        }
    }

    /// A result crosses as its variant and its value (#1118), and a native
    /// value in it, as the `iced::Error` of an `iced::Result`, is moved out
    /// as the runner's [`super::moves`] moves it, its tag naming its place
    /// among the reply's natives.
    #[test]
    fn a_result_crosses_and_its_native_value_moves() {
        use crate::program::model::island::{ERR, NATIVE, OK, STR};

        #[derive(Clone)]
        struct Handle(u8);
        super::moves(|value| {
            value
                .read_lock::<Handle>()
                .map(|handle| Ok(Box::new(handle.0) as Box<dyn std::any::Any>))
        });
        let engine = Shared::new(Engine::new());
        let authority = engine
            .compile_authority("pub fn keep() {}")
            .unwrap_or_else(|error| panic!("{error}"));
        let model = authority.model();
        let mut natives = Vec::new();

        let mut out = Vec::new();
        let error = crate::result::err(Dynamic::from(crate::ImmutableString::from("no")));
        model
            .island_write(&error, &mut out, &mut natives, 0)
            .unwrap();
        let mut expected = vec![ERR, STR];
        expected.extend_from_slice(&2_u64.to_le_bytes());
        expected.extend_from_slice(b"no");
        assert_eq!(out, expected);

        let mut out = Vec::new();
        let ok = crate::result::ok(Dynamic::UNIT);
        let failed = crate::result::err(Dynamic::from(Handle(7)));
        model.island_write(&ok, &mut out, &mut natives, 0).unwrap();
        model
            .island_write(&failed, &mut out, &mut natives, 0)
            .unwrap();
        let mut expected = vec![OK, crate::program::model::island::UNIT, ERR, NATIVE];
        expected.extend_from_slice(&0_u64.to_le_bytes());
        assert_eq!(out, expected);
        let [moved] = natives.as_slice() else {
            panic!("{} natives", natives.len());
        };
        assert_eq!(moved.downcast_ref::<u8>(), Some(&7));
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
