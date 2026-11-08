//! Scoped loans (D558.3): a value that carries a lifetime, lent to script
//! code for the length of one native call.
//!
//! Native code that calls script code may hand it a borrow, as ratatui's
//! `terminal.draw(|frame| ui::draw(frame, &mut app))` hands a marked `draw`
//! its `&mut Frame<'_>`. Rawr can't store such a value: only `'static`
//! values go in a runner's arena. A lend instead puts the borrow on this
//! thread's frame stack for the length of one call and gives the call a
//! [`Nonce`]. Script code holds the nonce as a [`Lent`] value, and each
//! native method it calls reaches the value through a closure that must
//! accept it at every lifetime. So nothing the closure returns or stores
//! can name the borrow, and once the lend returns its nonce is
//! [`Error::Expired`] for good.
//!
//! A lend is found by a key, the [`TypeId`] of a type that stands for it,
//! and an access claims it under the same key. In one crate the key is a
//! [`Family`] of types indexed by one lifetime, reached by [`lend`] and
//! [`with`]. A native island's trampoline can't name a family of the
//! generated runner, which forbids unsafe code, so it lends under a key of
//! its own and hands the runner accessors that take that key ([`Lent`]).
//!
//! This generalises D481's `Loan` of a fixed frame of fields and D552.4's
//! `island::lend` of one `'static` value. Miri passes the helper under
//! Stacked and Tree Borrows (D558, check (b′)).
//!
//! What must not compile. A borrow of the lent value returned out of
//! [`with`]:
//! ```compile_fail
//! use rawr::scope::{self, Family};
//! struct Words;
//! impl Family for Words { type Of<'x> = std::str::SplitWhitespace<'x>; }
//! let text = String::from("a b");
//! let mut words = text.split_whitespace();
//! scope::lend::<Words, _>(&mut words, |n| scope::with::<Words, _>(n, |w| w.next()));
//! ```
//!
//! Stored in a local outside the closure:
//! ```compile_fail,E0521
//! use rawr::scope::{self, Family};
//! struct Words;
//! impl Family for Words { type Of<'x> = std::str::SplitWhitespace<'x>; }
//! let text = String::from("a b");
//! let mut words = text.split_whitespace();
//! let mut kept = None;
//! scope::lend::<Words, _>(&mut words, |n| scope::with::<Words, _>(n, |w| kept = w.next()));
//! drop(kept);
//! ```
//!
//! Two lends' values swapped, so each would outlive its own frame:
//! ```compile_fail,E0521
//! use rawr::scope::{self, Family};
//! struct Words;
//! impl Family for Words { type Of<'x> = std::str::SplitWhitespace<'x>; }
//! let (one, two) = (String::from("a"), String::from("b"));
//! let (mut p, mut q) = (one.split_whitespace(), two.split_whitespace());
//! scope::lend::<Words, _>(&mut p, |n| {
//!     scope::lend::<Words, _>(&mut q, |m| {
//!         scope::with::<Words, _>(n, |p| scope::with::<Words, _>(m, |q| std::mem::swap(p, q)))
//!     })
//! });
//! ```
//!
//! Sent to another thread:
//! ```compile_fail,E0521
//! use rawr::scope::{self, Family};
//! struct Words;
//! impl Family for Words { type Of<'x> = std::str::SplitWhitespace<'x>; }
//! let text = String::from("a b");
//! let mut words = text.split_whitespace();
//! scope::lend::<Words, _>(&mut words, |n| {
//!     scope::with::<Words, _>(n, |w| std::thread::spawn(move || w.next().is_some()))
//! });
//! ```
//!
//! A region's value, named by the function that gives it ([`Given`]),
//! that borrows a local which ends before the region does:
//! ```compile_fail,E0597
//! use rawr::scope::{Family, Inputs, Region};
//! struct Text;
//! impl Family for Text { type Of<'x> = (&'x str,); }
//! impl Inputs for Text { fn name() -> &'static str { "impl Iterator" } }
//! fn words<'x>((text,): (&'x str,)) -> impl Iterator<Item = &'x str> + use<'x> {
//!     text.split_whitespace()
//! }
//! let long = String::from("a b");
//! let mut region = Region::new();
//! region.insert_given(Text, words, words((&long,)));
//! {
//!     let short = String::from("c d");
//!     region.insert_given(Text, words, words((&short,)));
//! }
//! drop(region);
//! ```

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::marker::PhantomData;
use std::ops::DerefMut;
use std::sync::atomic::{AtomicU64, Ordering};

/// A family of types indexed by one lifetime: `Parser<'x>` for every `'x`.
/// The family itself is `'static`, so its [`TypeId`] keys its lends.
pub trait Family: 'static {
    /// The family's type at the lifetime `'x`.
    type Of<'x>: 'x;
}

/// The identity of one lend. Never reused, so a stale nonce never names a
/// later lend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Nonce(u64);

impl Nonce {
    /// The nonce as a number, as a trampoline's accessor takes it.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }
}

/// Why a lent value can't be reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No lend on this thread has this nonce: the call it was lent for has
    /// returned, or it was lent on another thread.
    Expired,
    /// The lend lends another value.
    Mismatch,
    /// The value is already reached further up this stack in a way this
    /// access conflicts with (E0499, E0502).
    Busy,
    /// A mutable access to a value lent shared (E0596).
    ReadOnly,
    /// The value was moved out of its scope frame's region (E0382).
    Moved,
}

impl Error {
    /// The error as a number, as a trampoline's accessor returns it.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Expired => 1,
            Self::Mismatch => 2,
            Self::Busy => 3,
            Self::ReadOnly => 4,
            Self::Moved => 5,
        }
    }

    /// The error a number stands for; an unknown one is a mismatch.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Expired,
            3 => Self::Busy,
            4 => Self::ReadOnly,
            5 => Self::Moved,
            _ => Self::Mismatch,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Expired => {
                "this value is borrowed from native code for one call only, and that call has \
                 returned: a borrow can't be used after it ends (E0597)"
            }
            Self::Mismatch => "the lent value is not of the type this call takes",
            Self::Busy => {
                "cannot borrow this value: a call that borrows it mutably is still running \
                 (E0499, E0502)"
            }
            Self::ReadOnly => {
                "cannot borrow this value as mutable: native code lent it shared (E0596)"
            }
            Self::Moved => "use of a value moved out of the scope that borrows it (E0382)",
        })
    }
}

impl std::error::Error for Error {}

/// An accessor of a lent value, as a trampoline makes one: a `fn(u64, &mut
/// dyn FnMut(&mut T)) -> Result<(), u8>` that claims the lend whose nonce
/// it takes and hands the visitor the value.
pub type Accessor = &'static (dyn Any + Sync);

/// A lent value as script code holds it: the nonce of its lend, the Rust
/// name of its type, the accessors a trampoline made for it, and what
/// script code reads of it through its type's traits. Copying it copies
/// the claim, not the value, and every copy expires with the lend.
#[derive(Clone, Copy)]
pub struct Lent {
    nonce: Nonce,
    name: &'static str,
    mutable: Option<Accessor>,
    shared: Option<Accessor>,
    reads: Option<&'static Reads>,
}

/// How script code reads a lent value through its type's traits, as the
/// site that lends it makes this for the value's type: the value reached
/// shared as `dyn Any`, and its `Display` text, `Debug` text and `==` with
/// another value of its type, each `None` where the type lacks the trait.
pub struct Reads {
    reach: Reach,
    display: fn(&dyn Any) -> Option<String>,
    debug: fn(&dyn Any) -> Option<String>,
    equal: fn(&dyn Any, &dyn Any) -> Option<bool>,
}

/// How a lend's site reaches its value shared, as `dyn Any`, for the
/// visitor, by the lend's nonce ([`with_ref`]).
pub type Reach = fn(Nonce, &mut dyn FnMut(&dyn Any)) -> Result<(), Error>;

impl Reads {
    /// The reads of a value `reach` reaches by its lend's nonce, with its
    /// type's `display`, `debug` and `equal`.
    #[must_use]
    pub const fn new(
        reach: Reach,
        display: fn(&dyn Any) -> Option<String>,
        debug: fn(&dyn Any) -> Option<String>,
        equal: fn(&dyn Any, &dyn Any) -> Option<bool>,
    ) -> Self {
        Self {
            reach,
            display,
            debug,
            equal,
        }
    }
}

impl std::fmt::Display for Lent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.display() {
            Ok(Some(text)) => f.write_str(&text),
            Ok(None) => write!(f, "<{}>", self.name),
            Err(error) => write!(f, "<{}: {error}>", self.name),
        }
    }
}

impl std::fmt::Debug for Lent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.debug() {
            Ok(Some(text)) => f.write_str(&text),
            Ok(None) => write!(f, "lent `{}`", self.name),
            Err(error) => write!(f, "lent `{}`: {error}", self.name),
        }
    }
}

impl Lent {
    /// A lent value of the lend `nonce`, whose Rust type is named `name`,
    /// reached by a `mutable` accessor when it was lent `&mut`, and a
    /// `shared` one.
    #[must_use]
    pub const fn new(
        nonce: Nonce,
        name: &'static str,
        mutable: Option<Accessor>,
        shared: Option<Accessor>,
    ) -> Self {
        Self {
            nonce,
            name,
            mutable,
            shared,
            reads: None,
        }
    }

    /// The lent value, read through its type's traits by `reads`.
    #[must_use]
    pub const fn with_reads(self, reads: &'static Reads) -> Self {
        Self {
            reads: Some(reads),
            ..self
        }
    }

    /// The value's `Display` text, or `None` when its type has none.
    pub fn display(&self) -> Result<Option<String>, Error> {
        self.read(|reads, value| (reads.display)(value))
    }

    /// The value's `Debug` text, or `None` when its type has none.
    pub fn debug(&self) -> Result<Option<String>, Error> {
        self.read(|reads, value| (reads.debug)(value))
    }

    /// `self == other` of a value of its type, as a runner's arena holds
    /// one, when the type is `PartialEq`.
    pub fn equal_native(&self, other: &dyn Any) -> Result<Option<bool>, Error> {
        self.read(|reads, value| (reads.equal)(value, other))
    }

    /// `self == other` of two lent values of one `PartialEq` type.
    pub fn equal(&self, other: &Self) -> Result<Option<bool>, Error> {
        let mut equal = Ok(None);
        self.read(|_, value| {
            equal = other.equal_native(value);
            Some(())
        })?;
        equal
    }

    /// What `read` gives of the value reached shared, or `None` when its
    /// site made no reads.
    fn read<R>(
        &self,
        read: impl FnOnce(&Reads, &dyn Any) -> Option<R>,
    ) -> Result<Option<R>, Error> {
        let Some(reads) = self.reads else {
            return Ok(None);
        };
        let mut read = Some(read);
        let mut output = None;
        (reads.reach)(self.nonce, &mut |value| {
            if let Some(read) = read.take() {
                output = read(reads, value);
            }
        })?;
        Ok(output)
    }

    /// The nonce of the lend.
    #[must_use]
    pub const fn nonce(&self) -> Nonce {
        self.nonce
    }

    /// The Rust name of the lent value's type.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Whether the value was lent `&mut`.
    #[must_use]
    pub const fn writable(&self) -> bool {
        self.mutable.is_some()
    }

    /// The accessor of type `A` that reaches the value mutably or not:
    /// `fn(u64, &mut dyn FnMut(&mut T)) -> Result<(), u8>` for `&mut T`,
    /// and `fn(u64, &mut dyn FnMut(&T)) -> Result<(), u8>` for `&T`.
    /// [`Error::ReadOnly`] for a mutable access to a value lent shared, and
    /// [`Error::Mismatch`] when the value is not a `T`.
    pub fn accessor<A: Copy + 'static>(&self, mutable: bool) -> Result<A, Error> {
        let accessor = if mutable {
            self.mutable.ok_or(if self.shared.is_some() {
                Error::ReadOnly
            } else {
                Error::Mismatch
            })?
        } else {
            self.shared.ok_or(Error::Mismatch)?
        };
        let accessor: &dyn Any = accessor;
        accessor.downcast_ref::<A>().copied().ok_or(Error::Mismatch)
    }
}

struct Frame {
    nonce: Nonce,
    key: TypeId,
    /// A pointer to the lent value, valid while the frame is on the stack.
    /// Written through only when `writable`.
    place: *mut (),
    /// Lent by `&mut`, not `&`.
    writable: bool,
    /// A mutable access is running on it.
    exclusive: bool,
    /// How many shared accesses are running on it.
    shared: usize,
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
    /// The regions kept past the call that opened them ([`Region::keep`]),
    /// each lent by its nonce until its keeper lets it go. A nonce is on
    /// one list or the other, never both.
    static KEPT: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

/// The frame `nonce` names, on the stack or kept, for `visit`.
fn frame_of<R>(nonce: Nonce, visit: impl FnOnce(Option<&mut Frame>) -> R) -> R {
    FRAMES.with_borrow_mut(|frames| {
        if let Some(frame) = frames.iter_mut().rev().find(|frame| frame.nonce == nonce) {
            return visit(Some(frame));
        }
        KEPT.with_borrow_mut(|kept| visit(kept.iter_mut().find(|frame| frame.nonce == nonce)))
    })
}

/// Pops a frame, and every frame above it, when its lend ends, however it
/// ends.
struct Pop(Nonce);

impl Drop for Pop {
    fn drop(&mut self) {
        FRAMES.with(|frames| {
            let mut frames = frames.borrow_mut();
            if let Some(at) = frames.iter().rposition(|frame| frame.nonce == self.0) {
                frames.truncate(at);
            }
        });
    }
}

/// Ends one access of a frame.
fn release(nonce: Nonce, shared: bool) {
    frame_of(nonce, |frame| {
        if let Some(frame) = frame {
            if shared {
                frame.shared = frame.shared.saturating_sub(1);
            } else {
                frame.exclusive = false;
            }
        }
    });
}

/// A nonce no lend has had, and none will.
fn fresh() -> Nonce {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    Nonce(NEXT.fetch_add(1, Ordering::Relaxed))
}

fn push<R>(key: TypeId, place: *mut (), writable: bool, body: impl FnOnce(Nonce) -> R) -> R {
    push_as(fresh(), key, place, writable, body)
}

fn push_as<R>(
    nonce: Nonce,
    key: TypeId,
    place: *mut (),
    writable: bool,
    body: impl FnOnce(Nonce) -> R,
) -> R {
    FRAMES.with(|frames| {
        frames.borrow_mut().push(Frame {
            nonce,
            key,
            place,
            writable,
            exclusive: false,
            shared: 0,
        });
    });
    let _pop = Pop(nonce);
    body(nonce)
}

/// Run `body` with `value` lent, by the nonce `body` receives, to [`with`]
/// and [`with_ref`] on this thread. `value` stays mutably borrowed until
/// `body` returns or unwinds, so its owner can't touch it meanwhile, and
/// the frame is gone before that borrow ends.
pub fn lend<F: Family, R>(value: &mut F::Of<'_>, body: impl FnOnce(Nonce) -> R) -> R {
    push(
        TypeId::of::<F>(),
        std::ptr::from_mut(value).cast(),
        true,
        body,
    )
}

/// [`lend`] of a shared borrow: the value is reached by [`with_ref`], and
/// [`with`] on it is [`Error::ReadOnly`].
pub fn lend_ref<F: Family, R>(value: &F::Of<'_>, body: impl FnOnce(Nonce) -> R) -> R {
    push(
        TypeId::of::<F>(),
        std::ptr::from_ref(value).cast_mut().cast(),
        false,
        body,
    )
}

/// A lend under a `key` only the code that reaches it knows, as a native
/// island's trampoline lends its `&mut Frame<'_>` to the thread that runs
/// script code (`island::__rawr_island_call_v4`). The value is reached by
/// [`claim`]ing the key.
///
/// # Safety
///
/// `place` points to a value that outlives this call. It was made from a
/// `&mut` when `writable`, and from a `&` otherwise. No other reference to
/// the value is used until this call returns, except shared ones when it is
/// not `writable`. When this thread is not the one that borrowed it, the
/// value's type is `Send` (`writable`) or `Sync`, and the borrowing thread
/// waits for this call to return. Every [`claim`] of `key` treats the place
/// as the type it is.
pub unsafe fn lend_raw<R>(
    key: TypeId,
    place: *mut (),
    writable: bool,
    body: impl FnOnce(Nonce) -> R,
) -> R {
    push(key, place, writable, body)
}

/// One access of a lend: its place, held until [`Claim::release`] or a
/// drop.
pub struct Claim {
    nonce: Nonce,
    place: *mut (),
    shared: bool,
}

impl Claim {
    /// The lent value's place. It is valid until this claim ends, for reads
    /// and, for a claim that isn't shared, writes.
    #[must_use]
    pub const fn place(&self) -> *mut () {
        self.place
    }

    /// End the claim when the caller ends it: the place stays claimed until
    /// [`release`] of the same nonce and sharing.
    #[must_use]
    pub fn keep(self) -> *mut () {
        let place = self.place;
        std::mem::forget(self);
        place
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        release(self.nonce, self.shared);
    }
}

/// Claim the lend `nonce` names, under `key`, for one access: shared, or
/// mutable when not `shared`.
pub fn claim(nonce: Nonce, key: TypeId, shared: bool) -> Result<Claim, Error> {
    frame_of(nonce, |frame| {
        let frame = frame.ok_or(Error::Expired)?;
        if frame.key != key {
            return Err(Error::Mismatch);
        }
        if frame.exclusive {
            return Err(Error::Busy);
        }
        if shared {
            frame.shared += 1;
        } else {
            if !frame.writable {
                return Err(Error::ReadOnly);
            }
            if frame.shared > 0 {
                return Err(Error::Busy);
            }
            frame.exclusive = true;
        }
        Ok(Claim {
            nonce,
            place: frame.place,
            shared,
        })
    })
}

/// End a claim [`Claim::keep`] kept.
pub fn end_claim(nonce: Nonce, shared: bool) {
    release(nonce, shared);
}

/// The nonce `bits` stands for, as an accessor takes it.
#[must_use]
pub const fn nonce(bits: u64) -> Nonce {
    Nonce(bits)
}

/// Run `f` on the value the lend `nonce` names. `f` must accept the value
/// at every lifetime `'x`, so its result `R`, chosen outside the binder,
/// can't name `'x`, and it can't store anything that does.
pub fn with<F: Family, R>(
    nonce: Nonce,
    f: impl for<'x> FnOnce(&mut F::Of<'x>) -> R,
) -> Result<R, Error> {
    let claim = claim(nonce, TypeId::of::<F>(), false)?;
    // SAFETY: the frame is on this thread's stack, so the lend that pushed
    // it is still running and holds the `&mut F::Of<'a>` this pointer came
    // from: only `lend::<F>` pushes a frame under `F`'s key, and the claim
    // isn't shared, so the frame was lent `&mut`. It is now claimed
    // exclusively, so no other reference made from this pointer is live,
    // and none is made until the claim drops. The reference lives only for
    // `f`. `f` is generic over its lifetime, so it can only keep the value
    // inside, and can write in only values that outlive every `'x`, hence
    // `'a`.
    let value = unsafe { &mut *claim.place().cast::<F::Of<'_>>() };
    Ok(f(value))
}

/// Run `f` on a shared borrow of the value the lend `nonce` names. Any
/// number may run at once, nested on this stack, but none while a [`with`]
/// runs on the value, and no [`with`] while one runs (E0502). `f` is
/// generic over `'x` as in [`with`].
pub fn with_ref<F: Family, R>(
    nonce: Nonce,
    f: impl for<'x> FnOnce(&F::Of<'x>) -> R,
) -> Result<R, Error> {
    let claim = claim(nonce, TypeId::of::<F>(), true)?;
    // SAFETY: as in `with`, the lend is running and only `lend::<F>` or
    // `lend_ref::<F>` used this key. No mutable access runs on the frame,
    // and none starts until the count this claim added drops, so every
    // reference made from this pointer meanwhile is shared and nothing
    // writes through it. `f` is generic over `'x`.
    let value = unsafe { &*claim.place().cast_const().cast::<F::Of<'_>>() };
    Ok(f(value))
}

/// What a scope frame's [`Region`] keeps of a [`Family`]'s values: the Rust
/// name of its type, and how to print and copy one. A generated runner
/// implements it for each imported type that carries a lifetime.
pub trait Regional: Family {
    /// The Rust name of the family's type, for messages.
    fn name() -> &'static str;

    /// The value's `Debug` text, when its type has one.
    fn debug(_value: &Self::Of<'_>) -> Option<String> {
        None
    }

    /// The value's `Display` text, when its type has one.
    fn display(_value: &Self::Of<'_>) -> Option<String> {
        None
    }

    /// A copy of the value, when its type is `Clone`.
    fn duplicate<'x>(_value: &Self::Of<'x>) -> Option<Self::Of<'x>> {
        None
    }

    /// The value at a shorter lifetime, when its type is covariant in
    /// `'x`, as rustc's variance says: a value an enclosing frame's region
    /// holds then joins a call made in an inner one ([`Region::take`]). A
    /// runner's impl is `Some(value)`, which rustc checks.
    fn shorten<'s, 'l: 's>(_value: &'s Self::Of<'l>) -> Option<&'s Self::Of<'s>> {
        None
    }

    /// `a == b`, when the type is `PartialEq`.
    fn equal<'x>(_a: &Self::Of<'x>, _b: &Self::Of<'x>) -> Option<bool> {
        None
    }

    /// `a == b` of a value of the type at `'static` that a runner's arena
    /// holds, when the type is `PartialEq`, as `event ==
    /// Event::End(TagEnd::Item)` compares a frame's event with a new one.
    fn equal_native<'x>(_a: &Self::Of<'x>, _b: &dyn Any) -> Option<bool> {
        None
    }
}

/// One stored value of a region, its family erased. Only [`Slot`]
/// implements it, so a family's [`TypeId`] names exactly one stored type
/// for a region's `'r`.
trait Erased<'r> {
    fn family(&self) -> TypeId;
    fn debug(&self) -> Option<String>;
    fn display(&self) -> Option<String>;
    fn duplicate(&self) -> Option<Box<dyn Erased<'r> + 'r>>;
    fn equal(&self, other: &(dyn Erased<'r> + 'r)) -> Option<bool>;
    fn equal_native(&self, other: &dyn Any) -> Option<bool>;
    /// The value as its type at `'static`, its type erased
    /// ([`Region::take_native`]).
    ///
    /// # Safety
    ///
    /// As for [`Region::take_native`].
    unsafe fn into_native(self: Box<Self>) -> Box<dyn Any>;
}

struct Slot<'r, F: Regional>(F::Of<'r>);

impl<'r, F: Regional> Erased<'r> for Slot<'r, F> {
    fn family(&self) -> TypeId {
        TypeId::of::<F>()
    }

    fn debug(&self) -> Option<String> {
        F::debug(&self.0)
    }

    fn display(&self) -> Option<String> {
        F::display(&self.0)
    }

    fn duplicate(&self) -> Option<Box<dyn Erased<'r> + 'r>> {
        F::duplicate(&self.0)
            .map(|value| Box::new(Slot::<'r, F>(value)) as Box<dyn Erased<'r> + 'r>)
    }

    fn equal(&self, other: &(dyn Erased<'r> + 'r)) -> Option<bool> {
        if other.family() != TypeId::of::<F>() {
            return None;
        }
        // SAFETY: as in `Region::get`: a slot of `F`'s family at this
        // region's `'r` is a `Slot<'r, F>`.
        let other = unsafe { &*std::ptr::from_ref(other).cast::<Slot<'r, F>>() };
        F::equal(&self.0, &other.0)
    }

    fn equal_native(&self, other: &dyn Any) -> Option<bool> {
        F::equal_native(&self.0, other)
    }

    unsafe fn into_native(self: Box<Self>) -> Box<dyn Any> {
        let value = std::mem::ManuallyDrop::new(self.0);
        // SAFETY: the types differ only in their lifetime, which has no
        // layout, and the value is moved: the original is forgotten. That
        // it is read no longer than what it borrows lives is the caller's.
        Box::new(unsafe {
            std::ptr::read(std::ptr::from_ref::<F::Of<'r>>(&value).cast::<F::Of<'static>>())
        })
    }
}

/// The values one scope frame holds (D558 tier 1).
///
/// A call whose output borrows what it takes, as `Parser::new(&text)`'s
/// does, runs the rest of its Rust scope inside a native frame. The frame
/// owns what the output borrows and a region at that borrow's lifetime
/// `'r`. The output, and every value made from it that carries `'r`, is
/// stored here and named by a [`Scoped`] handle, which script code holds.
/// The region is lent to the rest of the scope by [`open`] and reached by
/// [`with`] as the family [`RegionF`]; it ends with its frame, and every
/// handle into it is then [`Error::Expired`]. Nothing is ever cast to
/// `'static`: values come back at the region's own `'r`, which is
/// invariant (D558, check (b′)).
pub struct Region<'r> {
    nonce: Nonce,
    /// The guards crate methods gave of values here, each kept beside the
    /// value it borrows ([`Region::yoke`]). Declared before `slots`, so
    /// they drop first.
    holds: RefCell<Vec<Hold>>,
    slots: Vec<Option<Box<dyn Erased<'r> + 'r>>>,
    /// Where script strings a call in the region borrows live for `'r`,
    /// in a region a slot opens at the lifetime of a script value it lent
    /// (D594).
    pins: Option<&'r Pins>,
    _r: PhantomData<fn(&'r ()) -> &'r ()>,
}

/// A guard a region keeps beside the value it borrows ([`Region::yoke`]):
/// a [`Guard`], its target's type erased.
struct Hold {
    index: u32,
    id: Nonce,
    guard: Box<dyn Any>,
}

/// A guard whose target is `U`, at a lifetime only its region knows.
struct Guard<U: ?Sized + 'static>(Box<dyn DerefMut<Target = U>>);

/// What a place holds of a guard its region keeps ([`Region::yoke`]): the
/// guard ends when this drops, as Rust drops it at the end of its
/// statement or block, or when its value is next reached. A region that is
/// busy when this drops ends the guard at that next reach, or when the
/// region does.
#[must_use]
pub struct Held<U: ?Sized + 'static> {
    nonce: Nonce,
    index: u32,
    id: Nonce,
    target: PhantomData<fn() -> Box<U>>,
}

impl<U: ?Sized + 'static> Held<U> {
    /// Whether the guard lasts: the method gave one, and nothing ended it.
    #[must_use]
    pub fn held(&self) -> bool {
        with::<RegionF, _>(self.nonce, |region| region.target(self).is_ok()).unwrap_or(false)
    }

    /// `body` of the guard's target, while the guard lasts.
    pub fn with<R>(&self, body: impl FnOnce(&mut U) -> R) -> Result<R, Error> {
        with::<RegionF, _>(self.nonce, |region| region.target(self).map(body))?
    }
}

impl<U: ?Sized + 'static> Drop for Held<U> {
    fn drop(&mut self) {
        let _ = with::<RegionF, _>(self.nonce, |region| {
            region.unhold(self.index, Some(self.id))
        });
    }
}

impl Default for Region<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'r> Region<'r> {
    /// An empty region, with the nonce its [`open`] lends it by.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nonce: fresh(),
            holds: RefCell::new(Vec::new()),
            slots: Vec::new(),
            pins: None,
            _r: PhantomData,
        }
    }

    /// An empty region at the lifetime `'r` of a script value native code
    /// lent a script function, as `view(&state)` lends `state` (D594): a
    /// script string a call in it borrows is pinned in `pins`, which
    /// outlive `'r`, so a value the call makes can keep the borrow.
    #[must_use]
    pub fn anchored(pins: &'r Pins) -> Self {
        Self {
            pins: Some(pins),
            ..Self::new()
        }
    }

    /// The pins of a region [`Region::anchored`] made.
    #[must_use]
    pub const fn pins(&self) -> Option<&'r Pins> {
        self.pins
    }

    /// The nonce the region is lent by while it is open.
    #[must_use]
    pub const fn nonce(&self) -> Nonce {
        self.nonce
    }

    /// Keep the region past the call that opened it, as a slot keeps the
    /// region of `view(&state)` once its output is taken out (#1217): a
    /// script value native code took of the call, as loupe's `Loupe {
    /// content }` inside the view's element, may hold one of its values,
    /// and native code reaches it after the view returns. A region
    /// [`Region::anchored`] on pins lives as long as they do: they are let
    /// go when the owner is next reached `&mut`, when rustc would have
    /// ended every borrow of the view. A region anchored on pins that are
    /// never let go ([`Pins::constant`], [`Pins::lasting`]) lives until the
    /// call at `site` opens the next one. A region of no pins, or that
    /// holds nothing, ends here.
    pub fn keep(self, site: &'static Site) {
        let Some(pins) = self.pins else {
            return;
        };
        if self.slots.iter().all(Option::is_none) {
            return;
        }
        let kept = KeptRegion::new(self, site);
        let mut regions = pins
            .regions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // What the site kept before ends once the lock is let go, so its
        // values' drops can keep regions of their own.
        let ended = if pins.lasting {
            let (ended, held) = std::mem::take(&mut *regions)
                .into_iter()
                .partition::<Vec<_>, _>(|held| std::ptr::eq(held.site, site));
            *regions = held;
            ended
        } else {
            Vec::new()
        };
        regions.push(kept);
        drop(regions);
        drop(ended);
    }

    /// Store `value` and give the handle script code holds for it.
    pub fn insert<F: Regional>(&mut self, value: F::Of<'r>) -> Scoped {
        self.slots.push(Some(Box::new(Slot::<'r, F>(value))));
        Scoped {
            nonce: self.nonce,
            index: u32::try_from(self.slots.len() - 1).unwrap_or(u32::MAX),
            name: F::name(),
        }
    }

    fn slot(&self, index: u32) -> Result<&(dyn Erased<'r> + 'r), Error> {
        self.unhold(index, None);
        self.slots
            .get(index as usize)
            .ok_or(Error::Mismatch)?
            .as_deref()
            .ok_or(Error::Moved)
    }

    fn slot_of<F: Regional>(&self, handle: Scoped) -> Result<&(dyn Erased<'r> + 'r), Error> {
        if handle.nonce != self.nonce {
            return Err(Error::Mismatch);
        }
        let slot = self.slot(handle.index)?;
        if slot.family() != TypeId::of::<F>() {
            return Err(Error::Mismatch);
        }
        Ok(slot)
    }

    /// The value `handle` names, as the family `F`.
    pub fn get<F: Regional>(&self, handle: Scoped) -> Result<&F::Of<'r>, Error> {
        let slot = self.slot_of::<F>(handle)?;
        // SAFETY: only `insert::<F>` makes a slot whose family is `F`'s,
        // and it stores a `Slot<'r, F>` at this region's `'r`, which never
        // changes (the region is invariant in it). So the box holds that
        // type, and the reference lives no longer than `&self`.
        Ok(unsafe { &(*std::ptr::from_ref(slot).cast::<Slot<'r, F>>()).0 })
    }

    /// The value `handle` names, mutably, as the family `F`.
    pub fn get_mut<F: Regional>(&mut self, handle: Scoped) -> Result<&mut F::Of<'r>, Error> {
        self.slot_of::<F>(handle)?;
        let slot = self.slots[handle.index as usize]
            .as_deref_mut()
            .ok_or(Error::Moved)?;
        // SAFETY: as in `get`; the `&mut self` borrow makes it unique.
        Ok(unsafe { &mut (*std::ptr::from_mut(slot).cast::<Slot<'r, F>>()).0 })
    }

    /// Move the value `handle` names out of the region. Every handle to it
    /// is then [`Error::Moved`]. A value of a region that encloses this
    /// one is moved out of that region at this one's `'r` when its type
    /// shortens ([`Regional::shorten`]), as Rust passes two `let`s'
    /// borrows to one call that lives as long as the shorter (#1329).
    pub fn take<F: Regional>(&mut self, handle: Scoped) -> Result<F::Of<'r>, Error> {
        if handle.nonce != self.nonce {
            return self.take_enclosing::<F>(handle);
        }
        self.slot_of::<F>(handle)?;
        let slot = self.slots[handle.index as usize]
            .take()
            .ok_or(Error::Moved)?;
        // SAFETY: as in `get`: the box holds a `Slot<'r, F>`, so it is
        // turned back into the box it was made as.
        let slot = unsafe { Box::from_raw(Box::into_raw(slot).cast::<Slot<'r, F>>()) };
        Ok(slot.0)
    }

    /// Move the value `handle` names out of the region as its type at
    /// `'static`, its type erased, for native code that takes it: a native
    /// trait object's `view(&self) -> Element<'_, Message>` gives native
    /// code the element its marked impl made so (#1321). Every handle to it
    /// is then [`Error::Moved`].
    ///
    /// # Safety
    ///
    /// The value borrows `'r`: the caller keeps the region's pins, and the
    /// region itself kept in them ([`Region::keep`]), for as long as the
    /// value lives, and native code reads it at no lifetime longer than
    /// theirs.
    pub unsafe fn take_native(&mut self, handle: Scoped) -> Result<Box<dyn Any>, Error> {
        if handle.nonce != self.nonce {
            return Err(Error::Mismatch);
        }
        self.slot(handle.index)?;
        let slot = self.slots[handle.index as usize]
            .take()
            .ok_or(Error::Moved)?;
        // SAFETY: as this function's own.
        Ok(unsafe { slot.into_native() })
    }

    /// [`Region::take`] of a value of the region `handle` names, which
    /// was opened further down this thread's stack than this one, so it
    /// ends after this one does.
    fn take_enclosing<F: Regional>(&self, handle: Scoped) -> Result<F::Of<'r>, Error> {
        if !encloses(handle.nonce, self.nonce) {
            return Err(Error::Mismatch);
        }
        with::<RegionF, _>(handle.nonce, |outer| {
            if F::shorten(outer.get::<F>(handle)?).is_none() {
                return Err(Error::Mismatch);
            }
            let value = std::mem::ManuallyDrop::new(outer.take::<F>(handle)?);
            // SAFETY: the types differ only in their lifetime, which has
            // no layout. The outer region's frame is below this one's on
            // the stack, so its `'r` outlives this region's, and `F::Of` is
            // covariant in it (`shorten` compiled as `Some(value)`), so its
            // value is one at this region's `'r`. It is moved: the original
            // is forgotten and its slot is empty.
            Ok(unsafe {
                std::ptr::read(std::ptr::from_ref::<F::Of<'_>>(&value).cast::<F::Of<'r>>())
            })
        })?
    }

    /// Store `value`, which the function `wrapper` gives, under its family
    /// [`Given`] at `inputs`. Both are taken by value so that rustc infers
    /// the function's item type, which no Rust code can write.
    pub fn insert_given<I: Inputs, G: 'static + for<'x> Gives<'x, I>>(
        &mut self,
        _inputs: I,
        _wrapper: G,
        value: <G as Gives<'r, I>>::Out,
    ) -> Scoped {
        self.insert::<Given<G, I>>(value)
    }

    /// The value `handle` names, as what the function `wrapper` gives.
    pub fn get_given<I: Inputs, G: 'static + for<'x> Gives<'x, I>>(
        &self,
        _inputs: I,
        _wrapper: G,
        handle: Scoped,
    ) -> Result<&<G as Gives<'r, I>>::Out, Error> {
        self.get::<Given<G, I>>(handle)
    }

    /// The value `handle` names, mutably, as what the function `wrapper`
    /// gives.
    pub fn get_mut_given<I: Inputs, G: 'static + for<'x> Gives<'x, I>>(
        &mut self,
        _inputs: I,
        _wrapper: G,
        handle: Scoped,
    ) -> Result<&mut <G as Gives<'r, I>>::Out, Error> {
        self.get_mut::<Given<G, I>>(handle)
    }

    /// Move the value `handle` names out of the region, as what the
    /// function `wrapper` gives.
    pub fn take_given<I: Inputs, G: 'static + for<'x> Gives<'x, I>>(
        &mut self,
        _inputs: I,
        _wrapper: G,
        handle: Scoped,
    ) -> Result<<G as Gives<'r, I>>::Out, Error> {
        self.take::<Given<G, I>>(handle)
    }

    /// Keep the guard `reach` makes of the value `handle` names beside it,
    /// as a crate method's `&mut` of a frame's value borrows it, as
    /// quick-xml's `reader.config_mut()` does (#1333): the method is called
    /// once, and its place reaches the guard through the [`Held`] this
    /// gives. Any other reach of the value ends the guard first, as rustc
    /// ends the borrow at its last use, and so does a later guard of it. A
    /// `reach` that gives none, as an `Option<&mut T>` method's `None`,
    /// keeps nothing.
    pub fn yoke<F: Regional, U: ?Sized + 'static>(
        &mut self,
        handle: Scoped,
        reach: impl for<'a> FnOnce(&'a mut F::Of<'r>) -> Option<Box<dyn DerefMut<Target = U> + 'a>>,
    ) -> Result<Held<U>, Error> {
        let value = std::ptr::from_mut(self.get_mut::<F>(handle)?);
        let id = fresh();
        // SAFETY: the value is boxed in its slot, so it stays where it is
        // while the slot holds it, and the guard ends before the slot is
        // reached any other way (`slot`, which every reach of a value goes
        // through) or dropped (the field order of `Region`). Only the
        // lifetime changes, which has no layout, and the guard is reached
        // only through `target`, while the region is borrowed `&mut`.
        let guard =
            reach(unsafe { &mut *value }).map(|guard| unsafe {
                std::mem::transmute::<
                    Box<dyn DerefMut<Target = U> + '_>,
                    Box<dyn DerefMut<Target = U>>,
                >(guard)
            });
        if let Some(guard) = guard {
            self.holds.get_mut().push(Hold {
                index: handle.index,
                id,
                guard: Box::new(Guard(guard)),
            });
        }
        Ok(Held {
            nonce: self.nonce,
            index: handle.index,
            id,
            target: PhantomData,
        })
    }

    /// [`Region::yoke`] of the value `handle` names, as what the function
    /// `wrapper` gives.
    pub fn yoke_given<I: Inputs, G: 'static + for<'x> Gives<'x, I>, U: ?Sized + 'static>(
        &mut self,
        _inputs: I,
        _wrapper: G,
        handle: Scoped,
        reach: impl for<'a> FnOnce(
            &'a mut <G as Gives<'r, I>>::Out,
        ) -> Option<Box<dyn DerefMut<Target = U> + 'a>>,
    ) -> Result<Held<U>, Error> {
        self.yoke::<Given<G, I>, U>(handle, reach)
    }

    /// The target of the guard `held` names, while it lasts.
    fn target<U: ?Sized + 'static>(&mut self, held: &Held<U>) -> Result<&mut U, Error> {
        if held.nonce != self.nonce {
            return Err(Error::Mismatch);
        }
        let hold = self
            .holds
            .get_mut()
            .iter_mut()
            .find(|hold| hold.id == held.id)
            .ok_or(Error::Expired)?;
        let guard = hold
            .guard
            .downcast_mut::<Guard<U>>()
            .ok_or(Error::Mismatch)?;
        Ok(&mut *guard.0)
    }

    /// End the guard kept beside the value at `index`, the one `id` names
    /// when it is given. The guard drops once the list is let go, so its
    /// drop may reach the region.
    fn unhold(&self, index: u32, id: Option<Nonce>) {
        let ended = {
            let Ok(mut holds) = self.holds.try_borrow_mut() else {
                return;
            };
            holds
                .iter()
                .position(|hold| hold.index == index && id.is_none_or(|id| hold.id == id))
                .map(|at| holds.swap_remove(at))
        };
        drop(ended);
    }

    /// A copy of the value `handle` names, in a new slot, when its type is
    /// `Clone`.
    pub fn duplicate(&mut self, handle: Scoped) -> Result<Option<Scoped>, Error> {
        if handle.nonce != self.nonce {
            return Err(Error::Mismatch);
        }
        let Some(copy) = self.slot(handle.index)?.duplicate() else {
            return Ok(None);
        };
        self.slots.push(Some(copy));
        Ok(Some(Scoped {
            nonce: self.nonce,
            index: u32::try_from(self.slots.len() - 1).unwrap_or(u32::MAX),
            name: handle.name,
        }))
    }

    fn equal(&self, a: Scoped, b: Scoped) -> Result<Option<bool>, Error> {
        if a.nonce != self.nonce || b.nonce != self.nonce {
            return Err(Error::Mismatch);
        }
        Ok(self.slot(a.index)?.equal(self.slot(b.index)?))
    }

    fn equal_native(&self, a: Scoped, b: &dyn Any) -> Result<Option<bool>, Error> {
        if a.nonce != self.nonce {
            return Err(Error::Mismatch);
        }
        Ok(self.slot(a.index)?.equal_native(b))
    }

    fn text(&self, handle: Scoped, display: bool) -> Result<Option<String>, Error> {
        if handle.nonce != self.nonce {
            return Err(Error::Mismatch);
        }
        let slot = self.slot(handle.index)?;
        Ok(if display {
            slot.display()
        } else {
            slot.debug()
        })
    }
}

/// Whether the lend `outer` names is further down this thread's stack
/// than the one `inner` names, so it ends after that one does.
fn encloses(outer: Nonce, inner: Nonce) -> bool {
    FRAMES.with_borrow(|frames| {
        let at = |nonce: Nonce| frames.iter().rposition(|frame| frame.nonce == nonce);
        matches!((at(outer), at(inner)), (Some(outer), Some(inner)) if outer < inner)
    })
}

/// The handle among `handles` whose region is highest on this thread's
/// stack, so it ends first: a call that takes values of several frames'
/// regions makes its output there, as Rust's output lives as long as its
/// shortest borrow (#1329). The first, of equals.
pub fn innermost(handles: impl IntoIterator<Item = Scoped>) -> Option<Scoped> {
    FRAMES.with_borrow(|frames| {
        let at = |handle: &Scoped| {
            frames
                .iter()
                .rposition(|frame| frame.nonce == handle.nonce)
                .map_or(0, |at| at + 1)
        };
        handles.into_iter().fold(None, |inner, handle| match inner {
            Some(inner) if at(&inner) >= at(&handle) => Some(inner),
            _ => Some(handle),
        })
    })
}

/// A [`Region`] as a family, so it is lent whole.
pub struct RegionF;

impl Family for RegionF {
    type Of<'x> = Region<'x>;
}

/// The inputs of a function whose output a [`Given`] family names, as one
/// family of tuples: `(Element<'x, ..>, u32)` of `explain(self, color)`.
pub trait Inputs: Family {
    /// The Rust name of the type the function gives, for messages.
    fn name() -> &'static str;
}

/// What a function `G` gives for its inputs `I` at `'x`.
///
/// A crate's opaque `impl Trait` type that borrows has no Rust name, as
/// what `element.explain(color)` gives doesn't. A runner names it as the
/// output of a function of its own, `fn given<'x>(inputs: (Element<'x,
/// ..>, u32)) -> impl Widget<..> + use<'x>`, whose `'x` is late-bound, so
/// the function's item type is `'static` and gives a type at each `'x`.
pub trait Gives<'x, I: Family> {
    /// The function's output at `'x`.
    type Out: 'x;
}

impl<'x, G, I: Family, O: 'x> Gives<'x, I> for G
where
    G: FnOnce(I::Of<'x>) -> O,
{
    type Out = O;
}

/// The family of what the function `G` gives for its inputs `I`, at each
/// lifetime. A runner can't write `G`, so it reaches a region's values of
/// it through [`Region::insert_given`] and the methods beside it, which
/// take the inputs' family and the function by value.
pub struct Given<G, I>(PhantomData<fn() -> (G, I)>);

impl<G: 'static + for<'x> Gives<'x, I>, I: Family> Family for Given<G, I> {
    type Of<'x> = <G as Gives<'x, I>>::Out;
}

impl<G: 'static + for<'x> Gives<'x, I>, I: Inputs> Regional for Given<G, I> {
    fn name() -> &'static str {
        I::name()
    }
}

thread_local! {
    /// The regions [`open_anchored`] opened on this thread, innermost last.
    static ANCHORS: RefCell<Vec<Nonce>> = const { RefCell::new(Vec::new()) };
    /// The regions [`open_joined`] opened on this thread, innermost last.
    static JOINS: RefCell<Vec<Nonce>> = const { RefCell::new(Vec::new()) };
}

/// Pops a region off the stack of [`ANCHORS`] or [`JOINS`] when the region
/// closes, however it closes.
struct Unanchor(&'static std::thread::LocalKey<RefCell<Vec<Nonce>>>);

impl Drop for Unanchor {
    fn drop(&mut self) {
        self.0.with(|anchors| anchors.borrow_mut().pop());
    }
}

/// The call that keeps a region ([`Region::keep`]), as a slot's `static`
/// names it: a region kept in pins that are never let go lives until the
/// same call keeps the next one.
pub struct Site {
    /// A byte, so each site's `static` has an address of its own.
    _distinct: u8,
}

impl Site {
    /// A call's site.
    #[must_use]
    pub const fn new() -> Self {
        Self { _distinct: 0 }
    }
}

impl Default for Site {
    fn default() -> Self {
        Self::new()
    }
}

/// A region [`Region::keep`] kept, lent by its nonce on the thread that
/// kept it until it drops.
struct KeptRegion {
    thread: std::thread::ThreadId,
    nonce: Nonce,
    site: &'static Site,
    /// The region, as `Box::leak` made it.
    region: std::ptr::NonNull<Region<'static>>,
}

impl KeptRegion {
    fn new(region: Region<'_>, site: &'static Site) -> Self {
        let nonce = region.nonce;
        // SAFETY: only the lifetime changes, which has no layout. The
        // region is anchored on pins its values borrow at `'r`, and it is
        // kept in those pins, which drop it before anything else they hold
        // and before they can be let go (`Pins::clear`, the field order of
        // `Pins`). Its values are never reached at `'static`: `with` and
        // `with_ref` of `RegionF` lend them at every `'x` alone.
        let region = unsafe { std::mem::transmute::<Region<'_>, Region<'static>>(region) };
        let region = std::ptr::NonNull::from(Box::leak(Box::new(region)));
        let place = region.as_ptr().cast();
        KEPT.with_borrow_mut(|kept| {
            kept.push(Frame {
                nonce,
                key: TypeId::of::<RegionF>(),
                place,
                writable: true,
                exclusive: false,
                shared: 0,
            });
        });
        Self {
            thread: std::thread::current().id(),
            nonce,
            site,
            region,
        }
    }
}

// SAFETY: a `KeptRegion`'s region is reached and dropped only on `thread`:
// `KEPT` is that thread's, and `Drop` leaks the region on any other.
unsafe impl Send for KeptRegion {}
// SAFETY: as for `Send`; `&KeptRegion` reaches nothing of the region.
unsafe impl Sync for KeptRegion {}

impl Drop for KeptRegion {
    fn drop(&mut self) {
        if std::thread::current().id() != self.thread {
            return;
        }
        // Its handles expire now. A claim still running on it, which no
        // borrow rustc allows leaves, keeps the region it reaches: it leaks.
        let claimed = KEPT.with_borrow_mut(|kept| {
            let at = kept.iter().position(|frame| frame.nonce == self.nonce)?;
            let frame = kept.swap_remove(at);
            Some(frame.exclusive || frame.shared > 0)
        });
        if claimed == Some(false) {
            // SAFETY: `Box::leak` made the pointer, and it is turned
            // back once, here, on its thread, with no claim on it and its
            // frame gone, so nothing reaches it again.
            drop(unsafe { Box::from_raw(self.region.as_ptr()) });
        }
    }
}

/// [`open`] of a region [`Region::anchored`] made: while `body` runs, a
/// call whose output borrows a script string it takes makes the output in
/// this region, which the slot that opened it gives native code at its
/// lifetime, instead of opening a frame of its own (D594).
pub fn open_anchored<R>(region: &mut Region<'_>, body: impl FnOnce() -> R) -> R {
    ANCHORS.with(|anchors| anchors.borrow_mut().push(region.nonce));
    let _unanchor = Unanchor(&ANCHORS);
    open(region, body)
}

/// The nonce of the innermost region [`open_anchored`] opened on this
/// thread, while its body runs.
#[must_use]
pub fn anchor() -> Option<Nonce> {
    ANCHORS.with(|anchors| anchors.borrow().last().copied())
}

/// [`open`] of a scope frame's region [`Region::anchored`] on pins of the
/// frame's own (#1335): while `body`, the rest of the frame's scope, runs,
/// a later `let` of a call whose output borrows a script string it takes
/// makes the output in this region, with the string pinned there, instead
/// of opening a frame of its own ([`joined`]). Both values are then at one
/// lifetime, as rustc infers one for two `let`s a call takes together,
/// which a type invariant in it, as iced's `Text<'a>` is, needs.
pub fn open_joined<R>(region: &mut Region<'_>, body: impl FnOnce() -> R) -> R {
    JOINS.with(|joins| joins.borrow_mut().push(region.nonce));
    let _unjoin = Unanchor(&JOINS);
    open(region, body)
}

/// The nonce of the region a call whose output borrows a script string it
/// takes makes its output in: the innermost slot's region ([`anchor`]),
/// else the innermost scope frame's [`open_joined`] opened on this thread.
#[must_use]
pub fn joined() -> Option<Nonce> {
    anchor().or_else(|| JOINS.with(|joins| joins.borrow().last().copied()))
}

/// The tag of text the program lends at `'static`: a string literal, or a
/// `const` of `&str`, passed where a crate's call borrows it for as long as
/// its output lives. The call takes it pinned for the runner's life and
/// gives a plain value, so no scope frame opens for it.
const STATIC_TEXT: crate::types::dynamic::Tag = 0x5374;

/// `text`, marked as lent at `'static` ([`lent_static`]): the resolver
/// marks an argument so only where rustc's `'static` holds for it.
pub fn lend_static(mut text: crate::Dynamic) -> crate::Dynamic {
    text.set_tag(STATIC_TEXT);
    text
}

/// Whether `value` is text [`lend_static`] marked.
#[must_use]
pub fn lent_static(value: &crate::Dynamic) -> bool {
    value.tag() == STATIC_TEXT
}

/// Script strings native code borrows for as long as the script value
/// they belong to is lent (D594): the strings a script `view(&state)`
/// gives native widgets that borrow them for `'a` of `&'a State`. Each is
/// held by its shared handle, so pinning copies no text, and one held
/// already is not held twice. Native values such widgets borrow are held
/// the same way, by their [`Rc`](std::rc::Rc) ([`Pins::hold`]). They are
/// let go when the owner is next reached `&mut`, when no borrow of them can
/// remain.
///
/// A view whose state is no program value, as `view(_: &())` of iced's
/// `run((), view)`, borrows only what outlives every call: the program's
/// string literals and constants, as rustc allows. Its slot pins them in
/// pins of its own that live as long as the runner ([`Pins::constant`]),
/// which hold one string per text, so a view called again, or a reload
/// that keeps a literal, pins nothing new.
#[derive(Default)]
pub struct Pins {
    /// The regions of the calls lent these pins, kept past each call
    /// ([`Region::keep`]). First, so they drop before what they borrow.
    regions: std::sync::Mutex<Vec<KeptRegion>>,
    strings: std::sync::Mutex<Vec<crate::ImmutableString>>,
    values: std::sync::Mutex<Vec<Kept>>,
    /// Slices of pinned strings ([`Pins::pin_strs`]).
    slices: std::sync::Mutex<Vec<&'static [&'static str]>>,
    /// Whether a string equal to one held is that one ([`Pins::constant`]).
    by_text: bool,
    /// Whether they are never let go ([`Pins::lasting`]).
    lasting: bool,
}

/// A native value [`Pins::hold`] keeps, with the thread whose `Rc` it is.
struct Kept {
    thread: std::thread::ThreadId,
    value: std::mem::ManuallyDrop<std::rc::Rc<dyn std::any::Any>>,
}

// SAFETY: a `Kept` touches its `Rc`'s count only on `thread`: `hold` makes
// it there, and `Drop` drops it there and leaks it on any other thread. Its
// value is never reached through it, so no other thread can see the value.
unsafe impl Send for Kept {}
// SAFETY: as for `Send`; `&Kept` reaches nothing but the `Rc`'s address.
unsafe impl Sync for Kept {}

impl Drop for Kept {
    fn drop(&mut self) {
        if std::thread::current().id() == self.thread {
            // SAFETY: dropped once, here, on the thread whose `Rc` it is.
            unsafe { std::mem::ManuallyDrop::drop(&mut self.value) }
        }
    }
}

impl Pins {
    /// Nothing pinned.
    #[must_use]
    pub const fn new() -> Self {
        Self::made(false, false)
    }

    /// Pins kept for as long as the runner lives, as a call that takes a
    /// `&'static str` pins its text in: never let go, so a region kept in
    /// them lives until its call keeps the next one ([`Region::keep`]).
    #[must_use]
    pub const fn lasting() -> Self {
        Self::made(false, true)
    }

    /// Pins a slot keeps for as long as the runner lives, for a view lent
    /// no program value: they hold one string per text, since what such a
    /// view borrows is the program's literals and constants.
    #[must_use]
    pub const fn constant() -> Self {
        Self::made(true, true)
    }

    const fn made(by_text: bool, lasting: bool) -> Self {
        Self {
            regions: std::sync::Mutex::new(Vec::new()),
            strings: std::sync::Mutex::new(Vec::new()),
            values: std::sync::Mutex::new(Vec::new()),
            slices: std::sync::Mutex::new(Vec::new()),
            by_text,
            lasting,
        }
    }

    /// `text`, borrowed for as long as these pins are.
    pub fn pin(&self, text: crate::ImmutableString) -> &str {
        let mut pinned = self
            .strings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = match pinned
            .iter()
            .position(|held| held.ptr_eq(&text) || (self.by_text && held.as_str() == text.as_str()))
        {
            Some(index) => index,
            None => {
                pinned.push(text);
                pinned.len() - 1
            }
        };
        let text: *const str = pinned[index].as_str();
        // SAFETY: the text lives in the string's shared allocation, which a
        // handle in `pinned` keeps alive until `clear` takes `&mut self`
        // or the pins drop, and neither can happen while `&self` is
        // borrowed. Moving a handle within the vector doesn't move the
        // allocation. Nothing writes a held string in place: only this
        // vector reaches its handle, and `ImmutableString::make_mut` of any
        // other handle copies while this one shares the allocation.
        unsafe { &*text }
    }

    /// `value`, borrowed for as long as these pins are: a runner's native
    /// value that a value made in a view borrows, as `qr_code(&state.data)`
    /// borrows `data` for `'a` of `&'a State` (D568). The runner shares it
    /// by its `Rc`, so nothing is copied, and takes it back whole once the
    /// pins let it go.
    pub fn hold<T: 'static>(&self, value: std::rc::Rc<T>) -> &T {
        let thread = std::thread::current().id();
        let pointer: *const T = std::rc::Rc::as_ptr(&value);
        let mut kept = self
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !kept.iter().any(|held| {
            held.thread == thread && std::ptr::addr_eq(std::rc::Rc::as_ptr(&held.value), pointer)
        }) {
            kept.push(Kept {
                thread,
                value: std::mem::ManuallyDrop::new(value),
            });
        }
        // SAFETY: the value lives in the `Rc`'s allocation, which a `Kept`
        // in these pins keeps alive until `clear` takes `&mut self` or the
        // pins drop, and neither can happen while `&self` is borrowed; a
        // `Kept` dropped on another thread leaks it. Moving a `Kept` within
        // the vector doesn't move the allocation. Nothing writes the value
        // while the pins share it: `Rc::get_mut` and `Rc::try_unwrap` fail
        // while they hold a count, and a `&T` allows only what `T`'s own
        // interior mutability allows.
        unsafe { &*pointer }
    }

    /// `keeper`, held for as long as these pins are: a runner's count on a
    /// native value native code borrows where it is, as a stand-in's `fn
    /// state(&self) -> &State` borrows the receiver's field (#1442). Held
    /// once however often it is kept.
    pub fn keep(&self, keeper: std::rc::Rc<dyn std::any::Any>) {
        let thread = std::thread::current().id();
        let mut kept = self
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !kept
            .iter()
            .any(|held| held.thread == thread && std::rc::Rc::ptr_eq(&held.value, &keeper))
        {
            kept.push(Kept {
                thread,
                value: std::mem::ManuallyDrop::new(keeper),
            });
        }
    }

    /// `texts`, each pinned ([`Pins::pin`]), as one `&'static [&'static
    /// str]` a crate takes: serde's `deserialize_struct(name, FIELDS,
    /// visitor)` takes the field names so. Only pins that live as long as
    /// the runner lend one, as a slot's `static` [`Pins::constant`] does,
    /// and equal slices of them are one slice, so a view called again
    /// leaks nothing new.
    pub fn pin_strs(
        &'static self,
        texts: impl IntoIterator<Item = crate::ImmutableString>,
    ) -> &'static [&'static str] {
        let pinned = texts
            .into_iter()
            .map(|text| self.pin(text))
            .collect::<Vec<&'static str>>();
        let mut slices = self
            .slices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(held) = slices.iter().find(|held| {
            held.len() == pinned.len()
                && held
                    .iter()
                    .zip(&pinned)
                    .all(|(held, text)| std::ptr::eq(*held, *text))
        }) {
            return held;
        }
        let slice: &'static [&'static str] = Box::leak(pinned.into_boxed_slice());
        slices.push(slice);
        slice
    }

    /// Let everything pinned go.
    pub fn clear(&mut self) {
        // The regions first: their values borrow what else is pinned.
        self.regions
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.slices
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.strings
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.values
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// How many strings and values are pinned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.strings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
            + self
                .values
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
    }

    /// Whether nothing is pinned, and no region is kept in them.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
            && self
                .regions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
    }
}

/// Run `body` with `region` lent, by its own nonce, to [`with`] and
/// [`with_ref`] of [`RegionF`] on this thread: the rest of a scope whose
/// values it holds. Every [`Scoped`] handle into it expires when `body`
/// returns or unwinds, unless the region is then kept ([`Region::keep`]).
pub fn open<R>(region: &mut Region<'_>, body: impl FnOnce() -> R) -> R {
    let nonce = region.nonce;
    push_as(
        nonce,
        TypeId::of::<RegionF>(),
        std::ptr::from_mut(region).cast(),
        true,
        |_| body(),
    )
}

/// A value in a scope frame's [`Region`], as script code holds it: the
/// region's nonce and the value's slot. Copying it copies the handle, not
/// the value, and every copy expires with the frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Scoped {
    nonce: Nonce,
    index: u32,
    name: &'static str,
}

impl Scoped {
    /// The nonce of the region that holds the value.
    #[must_use]
    pub const fn nonce(&self) -> Nonce {
        self.nonce
    }

    /// The Rust name of the value's type.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The value's `Debug` text, or `None` when its type has none.
    pub fn debug(&self) -> Result<Option<String>, Error> {
        with_ref::<RegionF, _>(self.nonce, |region| region.text(*self, false))?
    }

    /// The value's `Display` text, or `None` when its type has none.
    pub fn display(&self) -> Result<Option<String>, Error> {
        with_ref::<RegionF, _>(self.nonce, |region| region.text(*self, true))?
    }

    /// `self == other`, when both are in one region and their type is
    /// `PartialEq`.
    pub fn equal(&self, other: &Self) -> Result<Option<bool>, Error> {
        if self.nonce != other.nonce {
            return Ok(None);
        }
        with_ref::<RegionF, _>(self.nonce, |region| region.equal(*self, *other))?
    }

    /// `self == other` of the type's value at `'static`, as a runner's arena
    /// holds it, when the type is `PartialEq`.
    pub fn equal_native(&self, other: &dyn Any) -> Result<Option<bool>, Error> {
        with_ref::<RegionF, _>(self.nonce, |region| region.equal_native(*self, other))?
    }

    /// A copy of the value, in its region, when its type is `Clone`.
    pub fn duplicate(&self) -> Result<Option<Self>, Error> {
        with::<RegionF, _>(self.nonce, |region| region.duplicate(*self))?
    }
}

impl std::fmt::Display for Scoped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.display() {
            Ok(Some(text)) => f.write_str(&text),
            Ok(None) => write!(f, "<{}>", self.name),
            Err(error) => write!(f, "<{}: {error}>", self.name),
        }
    }
}

impl std::fmt::Debug for Scoped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.debug() {
            Ok(Some(text)) => f.write_str(&text),
            Ok(None) => write!(f, "<{}>", self.name),
            Err(error) => write!(f, "<{}: {error}>", self.name),
        }
    }
}

/// The rest of a Rust scope, which a scope frame runs inside itself: it
/// takes the value the frame made, and its result is the scope's.
/// `break`, `continue`, `return` and `?` leave it as `Err` values, which
/// the frame passes on as they are.
pub type Rest<'a> = dyn FnMut(crate::Dynamic) -> crate::RawrResult + 'a;

#[cfg(feature = "sync")]
type Open = Box<dyn FnOnce(&mut Rest<'_>) -> crate::RawrResult + Send + Sync>;
#[cfg(not(feature = "sync"))]
type Open = Box<dyn FnOnce(&mut Rest<'_>) -> crate::RawrResult>;

/// A scope frame a call made but has not opened (D558 tier 1): what a call
/// whose output borrows its inputs gives. A `let` that binds it opens it
/// with the rest of its block, and a statement that drops it opens it with
/// nothing after. Copies share the one frame, which opens once.
#[derive(Clone)]
pub struct Opening {
    name: &'static str,
    open: crate::Shared<crate::Locked<Option<Open>>>,
    copies: Copies,
}

/// What a frame's call changed of the `&mut` arrays it took copies of, as
/// quick-xml's `read_event_into(&mut buf)` fills its buffer: given back
/// when the frame ends and its output no longer borrows them, for the
/// statement after the frame to store (D558 slice 2).
#[derive(Clone, Default)]
pub struct Copies(crate::Shared<crate::Locked<Vec<crate::Dynamic>>>);

impl Copies {
    /// No copies yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The copies, in the order the call takes them, as the frame ends.
    pub fn set(&self, copies: Vec<crate::Dynamic>) {
        if let Some(mut held) = crate::func::locked_write(&self.0) {
            *held = copies;
        }
    }

    /// The copy at `index`, or `()` when the frame never ended.
    pub fn get(&self, index: usize) -> crate::Dynamic {
        crate::func::locked_read(&self.0)
            .and_then(|held| held.get(index).cloned())
            .unwrap_or(crate::Dynamic::UNIT)
    }
}

impl std::fmt::Debug for Opening {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<scope of `{}`>", self.name)
    }
}

impl Opening {
    /// A frame `open` runs: it owns what the call took, makes the output,
    /// and runs the rest of the scope with it.
    pub fn new(
        name: &'static str,
        open: impl FnOnce(&mut Rest<'_>) -> crate::RawrResult + crate::func::SendSync + 'static,
    ) -> Self {
        Self {
            name,
            open: crate::Shared::new(crate::Locked::new(Some(Box::new(open)))),
            copies: Copies::new(),
        }
    }

    /// The frame, whose call gives back its copies through `copies`.
    #[must_use]
    pub fn with_copies(mut self, copies: Copies) -> Self {
        self.copies = copies;
        self
    }

    /// What the frame's call changed of the arrays it took, once it ends.
    #[must_use]
    pub fn copies(&self) -> &Copies {
        &self.copies
    }

    /// The Rust path of the call that made it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Open the frame and run `rest` inside it.
    pub fn open(&self, rest: &mut Rest<'_>) -> crate::RawrResult {
        let open = crate::func::locked_write(&self.open)
            .and_then(|mut open| open.take())
            .ok_or_else(|| {
                crate::EvalAltResult::ErrorRuntime(
                    format!("the scope `{}` made is already open", self.name).into(),
                    crate::Position::NONE,
                )
            })?;
        open(rest)
    }
}

/// Whether `value` is a scope frame a call made and no `let` has opened.
#[inline]
#[must_use]
pub fn is_opening(value: &crate::Dynamic) -> bool {
    value.is_variant() && value.is::<Opening>()
}

/// `value`, or, when it is a scope frame no `let` bound, what the frame
/// makes, with the frame opened and closed at once: a statement that drops
/// the call's value, as `reader.config_mut().trim_text(true);` does.
pub fn settle(value: crate::Dynamic) -> crate::RawrResult {
    if !is_opening(&value) {
        return Ok(value);
    }
    value.cast::<Opening>().open(&mut |made| Ok(made))
}

/// How a [`Projected`] reaches its field, as a variant's generated lender
/// makes one for each field it lends through `&T`.
pub struct Projection {
    /// The Rust name of the field's type.
    pub name: &'static str,
    /// Reach the field through the value `parent` holds and lend it to
    /// `body`, shared, for `body`'s length.
    pub open: fn(&crate::Dynamic, &mut dyn FnMut(Lent) -> crate::RawrResult) -> crate::RawrResult,
    /// The field's `Debug` text, or its `Display` text when `display`, when
    /// its type has it.
    pub text: fn(&crate::Dynamic, bool) -> Option<String>,
    /// A value of its own cloned from the field by its type's `Clone`,
    /// attested by the field's witness when it has one.
    pub clone: fn(&crate::Dynamic, Option<&crate::host::runtime::Witness>) -> crate::RawrResult,
}

/// A field a pattern through `&T` borrows from a crate value (#782, #783):
/// the value it is reached through and how. It holds no reference to the
/// field. Each operation reaches the value, claims it and lends the field
/// for that operation alone, as a place projects into its root (D651). So
/// the borrow lives as long as Rust's borrow of the field may, past the arm
/// that bound it, and expires with the value it is reached through. The
/// borrow checker keeps that value from moving or changing while the
/// borrow lives.
#[derive(Clone)]
pub struct Projected {
    parent: crate::Dynamic,
    projection: &'static Projection,
    witness: Option<crate::host::runtime::Witness>,
}

impl Projected {
    /// The field `projection` reaches through `parent`, of the type
    /// `witness` attests, when the call that made it has one.
    #[must_use]
    pub const fn new(
        parent: crate::Dynamic,
        projection: &'static Projection,
        witness: Option<crate::host::runtime::Witness>,
    ) -> Self {
        Self {
            parent,
            projection,
            witness,
        }
    }

    /// The Rust name of the field's type.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.projection.name
    }

    /// Run `body` with the field lent, shared, for its length.
    pub fn open(&self, body: &mut dyn FnMut(Lent) -> crate::RawrResult) -> crate::RawrResult {
        (self.projection.open)(&self.parent, body)
    }

    /// A value of its own cloned from the field, as `.clone()` of a `&T`
    /// gives a `T`.
    pub fn cloned(&self) -> crate::RawrResult {
        (self.projection.clone)(&self.parent, self.witness.as_ref())
    }
}

impl std::fmt::Debug for Projected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.projection.text)(&self.parent, false) {
            Some(text) => f.write_str(&text),
            None => write!(f, "lent `{}`", self.name()),
        }
    }
}

impl std::fmt::Display for Projected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.projection.text)(&self.parent, true) {
            Some(text) => f.write_str(&text),
            None => write!(f, "lent `{}`", self.name()),
        }
    }
}

/// Run `call` with each [`Projected`] among `args` lent for the call alone,
/// as the [`Lent`] native code reaches it by, and the arguments as they were
/// after it.
pub fn opened(
    args: &mut [&mut crate::Dynamic],
    call: &mut dyn FnMut(&mut [&mut crate::Dynamic]) -> crate::RawrResult,
) -> crate::RawrResult {
    let Some(at) = args.iter().position(|value| value.is::<Projected>()) else {
        return call(args);
    };
    let projected = args[at].clone_cast::<Projected>();
    projected.open(&mut |lent| {
        let held = std::mem::replace(&mut *args[at], crate::Dynamic::from(lent));
        let result = opened(args, call);
        *args[at] = held;
        result
    })
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::{Error, Family, Lent, Nonce};

    /// A borrowing parser: words of its source, each a slice of it.
    struct Parser<'a> {
        rest: &'a str,
    }

    impl<'a> Parser<'a> {
        fn new(source: &'a str) -> Self {
            Self { rest: source }
        }

        fn next(&mut self) -> Option<&'a str> {
            let trimmed = self.rest.trim_start();
            if trimmed.is_empty() {
                self.rest = trimmed;
                return None;
            }
            let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            let (word, rest) = trimmed.split_at(end);
            self.rest = rest;
            Some(word)
        }
    }

    struct ParserF;

    impl Family for ParserF {
        type Of<'x> = Parser<'x>;
    }

    struct CountF;

    impl Family for CountF {
        type Of<'x> = usize;
    }

    fn depth() -> usize {
        super::FRAMES.with(|frames| frames.borrow().len())
    }

    fn next_owned(nonce: Nonce) -> Result<Option<String>, Error> {
        super::with::<ParserF, _>(nonce, |p| p.next().map(str::to_owned))
    }

    /// The loop D558.3 runs: script holds only the nonce.
    #[test]
    fn a_loop_steps_a_lent_parser() {
        let source = String::from("  alpha beta\tgamma ");
        let mut parser = Parser::new(&source);
        let words = super::lend::<ParserF, _>(&mut parser, |n| {
            let mut words = Vec::new();
            while let Some(word) = next_owned(n).unwrap() {
                words.push(word);
            }
            words
        });
        assert_eq!(words, ["alpha", "beta", "gamma"]);
        assert_eq!(depth(), 0);
    }

    #[test]
    fn nested_lends_reach_each_frame_and_a_value_in_use_is_busy() {
        let source = String::from("one two three");
        let mut parser = Parser::new(&source);
        let mut count = 0_usize;
        super::lend::<ParserF, _>(&mut parser, |p| {
            super::lend::<CountF, _>(&mut count, |c| {
                assert_eq!(depth(), 2);
                while next_owned(p).unwrap().is_some() {
                    super::with::<CountF, _>(c, |c| *c += 1).unwrap();
                }
                super::with::<CountF, _>(c, |_| {
                    assert_eq!(super::with::<CountF, _>(c, |_| ()), Err(Error::Busy));
                    assert_eq!(super::with_ref::<CountF, _>(c, |_| ()), Err(Error::Busy));
                })
                .unwrap();
                assert_eq!(super::with::<ParserF, _>(c, |_| ()), Err(Error::Mismatch));
            });
            assert_eq!(depth(), 1);
        });
        assert_eq!(count, 3);
        assert_eq!(depth(), 0);
    }

    #[test]
    fn shared_access_is_counted_and_a_shared_lend_is_read_only() {
        let mut count = 4_usize;
        super::lend::<CountF, _>(&mut count, |n| {
            super::with_ref::<CountF, _>(n, |outer| {
                let inner = super::with_ref::<CountF, _>(n, |inner| *inner).unwrap();
                assert_eq!(inner, *outer);
                assert_eq!(super::with::<CountF, _>(n, |_| ()), Err(Error::Busy));
            })
            .unwrap();
            super::with::<CountF, _>(n, |count| *count += 1).unwrap();
        });
        assert_eq!(count, 5);
        let shared = 7_usize;
        super::lend_ref::<CountF, _>(&shared, |n| {
            assert_eq!(super::with::<CountF, _>(n, |_| ()), Err(Error::ReadOnly));
            assert_eq!(super::with_ref::<CountF, _>(n, |value| *value), Ok(7));
        });
    }

    #[test]
    fn a_panic_pops_every_frame_and_frees_the_value() {
        let source = String::from("left right");
        let mut parser = Parser::new(&source);
        let mut seen = None;
        let caught = catch_unwind(AssertUnwindSafe(|| {
            super::lend::<ParserF, _>(&mut parser, |n| {
                seen = Some(n);
                super::with::<ParserF, _>(n, |p| {
                    p.next();
                    panic!("inside a with");
                })
            })
        }));
        assert!(caught.is_err());
        assert_eq!(depth(), 0);
        assert_eq!(next_owned(seen.unwrap()), Err(Error::Expired));
        assert_eq!(parser.next(), Some("right"));
        // Caught inside the lend, the value is free again.
        super::lend::<ParserF, _>(&mut parser, |n| {
            let caught = catch_unwind(AssertUnwindSafe(|| {
                super::with::<ParserF, _>(n, |_| panic!("in a with"))
            }));
            assert!(caught.is_err());
            assert_eq!(next_owned(n), Ok(None));
        });
    }

    #[test]
    fn a_stale_nonce_and_another_threads_nonce_are_expired() {
        let source = String::from("only");
        let stale = {
            let mut parser = Parser::new(&source);
            super::lend::<ParserF, _>(&mut parser, |n| n)
        };
        assert_eq!(next_owned(stale), Err(Error::Expired));
        let mut again = Parser::new(&source);
        super::lend::<ParserF, _>(&mut again, |fresh| {
            assert_ne!(fresh, stale);
            assert_eq!(next_owned(stale), Err(Error::Expired));
            let there = std::thread::spawn(move || next_owned(fresh))
                .join()
                .unwrap();
            assert_eq!(there, Err(Error::Expired));
            assert_eq!(next_owned(fresh).unwrap().as_deref(), Some("only"));
        });
    }

    impl super::Regional for ParserF {
        fn name() -> &'static str {
            "Parser"
        }
    }

    /// A word as a region holds it: `&'x str`, printed and copied.
    struct WordF;

    impl Family for WordF {
        type Of<'x> = &'x str;
    }

    impl super::Regional for WordF {
        fn name() -> &'static str {
            "&str"
        }

        fn debug(value: &Self::Of<'_>) -> Option<String> {
            Some(format!("{value:?}"))
        }

        fn display(value: &Self::Of<'_>) -> Option<String> {
            Some((*value).to_owned())
        }

        fn duplicate<'x>(value: &Self::Of<'x>) -> Option<Self::Of<'x>> {
            Some(*value)
        }
    }

    /// A frame's region: values made from its parser stay at its lifetime,
    /// handles reach them by family, a moved value is gone, and every
    /// handle expires with the frame (D558 tier 1).
    #[test]
    fn a_region_holds_a_frame_s_values_until_the_frame_ends() {
        use super::{Region, RegionF, Scoped};
        let source = String::from("one two");
        let mut region = Region::new();
        let parser = region.insert::<ParserF>(Parser::new(&source));
        let (word, copy) = super::open(&mut region, || {
            let word = super::with::<RegionF, _>(parser.nonce(), |region| {
                let word = region.get_mut::<ParserF>(parser).unwrap().next().unwrap();
                region.insert::<WordF>(word)
            })
            .unwrap();
            assert_eq!(word.debug(), Ok(Some("\"one\"".to_owned())));
            assert_eq!(format!("{word}"), "one");
            assert_eq!(
                super::with::<RegionF, _>(parser.nonce(), |region| region
                    .get::<WordF>(parser)
                    .err())
                .unwrap(),
                Some(Error::Mismatch)
            );
            let copy = word.duplicate().unwrap().unwrap();
            super::with::<RegionF, _>(parser.nonce(), |region| {
                assert_eq!(region.take::<WordF>(word), Ok("one"));
                assert_eq!(region.get::<WordF>(word).err(), Some(Error::Moved));
                assert_eq!(region.get::<WordF>(copy).copied(), Ok("one"));
            })
            .unwrap();
            assert_eq!(depth(), 1);
            (word, copy)
        });
        assert_eq!(depth(), 0);
        let expired = |handle: Scoped| handle.debug().err();
        assert_eq!(expired(word), Some(Error::Expired));
        assert_eq!(expired(copy), Some(Error::Expired));
        assert_eq!(format!("{copy:?}"), format!("<&str: {}>", Error::Expired));
    }

    /// Counts under a name a region's value borrows, and how often one was
    /// lent mutably.
    struct Tally<'a> {
        name: &'a str,
        counts: Vec<u32>,
        lent: usize,
    }

    struct TallyF;

    impl Family for TallyF {
        type Of<'x> = Tally<'x>;
    }

    impl super::Regional for TallyF {
        fn name() -> &'static str {
            "Tally"
        }
    }

    /// The count at `index` of the tally `handle` names, kept beside it.
    fn yoked(handle: super::Scoped, index: usize) -> super::Held<u32> {
        super::with::<super::RegionF, _>(handle.nonce(), |region| {
            region.yoke::<TallyF, _>(handle, |tally| {
                tally.lent += 1;
                tally
                    .counts
                    .get_mut(index)
                    .map(|count| Box::new(count) as Box<dyn std::ops::DerefMut<Target = u32>>)
            })
        })
        .unwrap()
        .unwrap()
    }

    /// A region keeps a guard beside the value it borrows (#1333): each
    /// access reaches the one guard, another reach of the value or the
    /// handle's drop ends it, and a method that gives none keeps nothing.
    #[test]
    fn a_region_keeps_a_guard_beside_its_value() {
        use super::{Region, RegionF};
        let name = String::from("tally");
        let mut region = Region::new();
        let tally = region.insert::<TallyF>(Tally {
            name: &name,
            counts: vec![0, 0],
            lent: 0,
        });
        let holds =
            |nonce| super::with::<RegionF, _>(nonce, |region| region.holds.borrow().len()).unwrap();
        super::open(&mut region, || {
            let held = yoked(tally, 1);
            assert!(held.held());
            held.with(|count| *count += 2).unwrap();
            held.with(|count| *count *= 5).unwrap();
            let reached = super::with::<RegionF, _>(tally.nonce(), |region| {
                let tally = region.get::<TallyF>(tally).unwrap();
                (tally.name.to_owned(), tally.counts.clone(), tally.lent)
            })
            .unwrap();
            assert_eq!(reached, ("tally".to_owned(), vec![0, 10], 1));
            assert!(!held.held());
            assert_eq!(held.with(|_| ()), Err(Error::Expired));
            drop(yoked(tally, 0));
            assert_eq!(holds(tally.nonce()), 0);
            let missing = yoked(tally, 2);
            assert!(!missing.held());
            assert_eq!(holds(tally.nonce()), 0);
        });
        assert_eq!(region.get::<TallyF>(tally).unwrap().lent, 3);
    }

    /// A region kept past its call is reached by its handles for as long
    /// as its pins hold it: pins let go end it, and pins never let go end
    /// it when its site keeps the next one (#1217).
    #[test]
    fn a_kept_region_lives_as_long_as_its_pins_hold_it() {
        use super::{Pins, Region, Scoped, Site};
        fn kept(pins: &Pins, text: &str, site: &'static Site) -> Scoped {
            let mut region = Region::anchored(pins);
            let word = region.insert::<WordF>(pins.pin(text.into()));
            region.keep(site);
            word
        }
        static ONE: Site = Site::new();
        static TWO: Site = Site::new();
        let mut pins = Pins::new();
        let word = kept(&pins, "one", &ONE);
        let again = kept(&pins, "two", &ONE);
        assert_eq!(depth(), 0);
        assert_eq!(word.debug(), Ok(Some("\"one\"".to_owned())));
        assert_eq!(again.debug(), Ok(Some("\"two\"".to_owned())));
        pins.clear();
        assert_eq!(word.debug(), Err(Error::Expired));
        assert_eq!(again.debug(), Err(Error::Expired));
        let lasting = Pins::lasting();
        let first = kept(&lasting, "one", &ONE);
        let other = kept(&lasting, "two", &TWO);
        let next = kept(&lasting, "three", &ONE);
        assert_eq!(first.debug(), Err(Error::Expired));
        assert_eq!(other.debug(), Ok(Some("\"two\"".to_owned())));
        assert_eq!(next.debug(), Ok(Some("\"three\"".to_owned())));
        // A region of no pins ends with its call.
        let mut region = Region::new();
        let word = region.insert::<WordF>("four");
        region.keep(&ONE);
        assert_eq!(word.debug(), Err(Error::Expired));
    }

    /// A frame opens once; what its call gives back is there once it ends.
    #[test]
    fn an_opening_opens_once_and_gives_back_its_copies() {
        use super::{Copies, Opening};
        let copies = Copies::new();
        let given = copies.clone();
        let opening = Opening::new("fill", move |rest| {
            let made = rest(crate::Dynamic::from(1_i64))?;
            given.set(vec![crate::Dynamic::from(2_i64)]);
            Ok(made)
        })
        .with_copies(copies);
        let kept = opening.clone();
        let made = opening.open(&mut |value| Ok(value)).unwrap();
        assert_eq!(made.as_int(), Ok(1));
        assert_eq!(kept.copies().get(0).as_int(), Ok(2));
        assert!(kept.copies().get(1).is_unit());
        assert!(kept.open(&mut |value| Ok(value)).is_err());
        assert!(super::is_opening(&crate::Dynamic::from(kept)));
        assert_eq!(
            super::settle(crate::Dynamic::from(3_i64)).unwrap().as_int(),
            Ok(3)
        );
    }

    /// The key a trampoline's lend of a `Parser` and its accessors share.
    struct Key;

    type Visit<'v> = &'v mut dyn FnMut(&mut Parser<'_>);
    type Access = fn(u64, &mut dyn FnMut(&mut Parser<'_>)) -> Result<(), u8>;

    /// What a trampoline generates for a `&mut Parser<'_>` it lends.
    fn access(nonce: u64, visit: Visit<'_>) -> Result<(), u8> {
        let claim =
            super::claim(super::nonce(nonce), TypeId::of::<Key>(), false).map_err(Error::code)?;
        // SAFETY: only the test's lend uses `Key`, and it lends a `Parser`
        // mutably; the claim is exclusive while `visit` runs.
        visit(unsafe { &mut *claim.place().cast::<Parser<'_>>() });
        Ok(())
    }

    static ACCESS: Access = access;

    /// A trampoline's lend from another thread, reached through the
    /// accessor the lent value carries, as a generated runner reaches it.
    #[test]
    fn a_raw_lend_from_another_thread_is_reached_through_its_accessor() {
        let source = String::from("a b");
        let mut parser = Parser::new(&source);
        let place = std::ptr::from_mut(&mut parser).cast::<()>() as usize;
        let word = std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    // SAFETY: `place` is the `&mut Parser<'_>` this test
                    // holds, `Parser` is `Send`, the test waits for this
                    // thread before it uses `parser` again, and `access`
                    // reads `Key`'s place as a `Parser`.
                    unsafe {
                        super::lend_raw(TypeId::of::<Key>(), place as *mut (), true, |n| {
                            let lent = Lent::new(n, "Parser", Some(&ACCESS), None);
                            assert_eq!(lent.accessor::<Access>(false).err(), Some(Error::Mismatch));
                            let access = lent.accessor::<Access>(true).unwrap();
                            let mut word = None;
                            access(lent.nonce().bits(), &mut |parser| {
                                word = parser.next().map(str::to_owned);
                            })
                            .unwrap();
                            word
                        })
                    }
                })
                .join()
                .unwrap()
        });
        assert_eq!(word.as_deref(), Some("a"));
        assert_eq!(parser.next(), Some("b"));
        // The lend has ended: its accessor finds nothing.
        let mut reached = false;
        let expired = ACCESS(1, &mut |_| reached = true);
        assert_eq!(expired.map_err(Error::from_code), Err(Error::Expired));
        assert!(!reached);
    }

    /// A native value a view borrows is held by its `Rc` once however
    /// often it is pinned, borrowed in place, and let go by `clear`, so
    /// its owner gets it back whole.
    #[test]
    fn pins_hold_a_native_value_in_place_until_cleared() {
        let value = std::rc::Rc::new(String::from("data"));
        let mut pins = super::Pins::new();
        let held = pins.hold(value.clone());
        assert!(std::ptr::eq(held, &*value));
        assert!(std::ptr::eq(pins.hold(value.clone()), held));
        assert_eq!(pins.len(), 1);
        assert_eq!(std::rc::Rc::strong_count(&value), 2);
        pins.clear();
        assert!(pins.is_empty());
        assert_eq!(std::rc::Rc::try_unwrap(value).as_deref(), Ok("data"));
    }

    /// Pins dropped on another thread leak a held value rather than touch
    /// its count there.
    #[cfg(feature = "sync")]
    #[test]
    fn pins_dropped_on_another_thread_leak_what_they_hold() {
        let value = std::rc::Rc::new(7_u8);
        let pins = super::Pins::new();
        let _ = pins.hold(value.clone());
        std::thread::scope(|scope| scope.spawn(move || drop(pins)).join().unwrap());
        assert_eq!(std::rc::Rc::strong_count(&value), 2);
    }

    /// The lengths of a text's words, as an opaque type that borrows it:
    /// what a region holds by [`super::Given`].
    struct Text;

    impl Family for Text {
        type Of<'x> = (&'x str,);
    }

    impl super::Inputs for Text {
        fn name() -> &'static str {
            "impl Iterator<Item = usize>"
        }
    }

    fn lengths<'x>((text,): (&'x str,)) -> impl Iterator<Item = usize> + use<'x> {
        text.split_whitespace().map(str::len)
    }

    /// The first word of a text, as another opaque type of the same inputs.
    fn first<'x>((text,): (&'x str,)) -> impl Iterator<Item = &'x str> + use<'x> {
        text.split_whitespace().take(1)
    }

    /// A region holds what a function of its own gives, at the region's
    /// lifetime, by the function's item type: reached, stepped and moved
    /// out through a lent region, refused as another family, and held
    /// apart from what another function of the same inputs gives.
    #[test]
    fn a_region_holds_an_opaque_output_by_the_function_that_gives_it() {
        use super::{Region, RegionF};
        let source = String::from("one three fifteen");
        let mut region = Region::new();
        let handle = region.insert_given(Text, lengths, lengths((&source,)));
        let other = region.insert_given(Text, first, first((&source,)));
        assert_eq!(handle.name(), "impl Iterator<Item = usize>");
        let rest = super::open(&mut region, || {
            super::with::<RegionF, _>(handle.nonce(), |region| {
                let next = region.get_mut_given(Text, lengths, handle).unwrap().next();
                assert_eq!(next, Some(3));
                assert_eq!(region.get::<WordF>(handle).err(), Some(Error::Mismatch));
                assert_eq!(
                    region.get_given(Text, first, handle).err(),
                    Some(Error::Mismatch)
                );
                let rest = region
                    .take_given(Text, lengths, handle)
                    .unwrap()
                    .collect::<Vec<_>>();
                assert_eq!(
                    region.get_given(Text, lengths, handle).err(),
                    Some(Error::Moved)
                );
                rest
            })
            .unwrap()
        });
        assert_eq!(rest, [5, 7]);
        let other = region.take_given(Text, first, other).unwrap();
        assert_eq!(other.collect::<Vec<_>>(), ["one"]);
    }

    fn reach_count(nonce: Nonce, visit: &mut dyn FnMut(&dyn std::any::Any)) -> Result<(), Error> {
        super::with_ref::<CountF, _>(nonce, |count| visit(count))
    }

    fn display_count(value: &dyn std::any::Any) -> Option<String> {
        value.downcast_ref::<usize>().map(ToString::to_string)
    }

    fn debug_count(value: &dyn std::any::Any) -> Option<String> {
        value
            .downcast_ref::<usize>()
            .map(|count| format!("Count({count})"))
    }

    fn equal_count(value: &dyn std::any::Any, other: &dyn std::any::Any) -> Option<bool> {
        Some(value.downcast_ref::<usize>()? == other.downcast_ref::<usize>()?)
    }

    static COUNT_READS: super::Reads =
        super::Reads::new(reach_count, display_count, debug_count, equal_count);

    /// A lent value is read through its type's traits while its lend runs,
    /// as script code formats a `&T` native code lent it and compares it
    /// with a value of its type and with another lent value. Once the lend
    /// returns, each read is the borrow's end, and a value its site made no
    /// reads for has no text of its own.
    #[test]
    fn a_lent_value_is_read_through_its_types_traits_while_its_lend_runs() {
        let (three, other) = (3_usize, 3_usize);
        let kept = super::lend_ref::<CountF, _>(&three, |n| {
            let lent = Lent::new(n, "usize", None, None).with_reads(&COUNT_READS);
            assert_eq!(lent.to_string(), "3");
            assert_eq!(format!("{lent:?}"), "Count(3)");
            assert_eq!(lent.equal_native(&3_usize), Ok(Some(true)));
            assert_eq!(lent.equal_native(&4_usize), Ok(Some(false)));
            assert_eq!(lent.equal_native(&"3"), Ok(None));
            super::lend_ref::<CountF, _>(&other, |m| {
                let again = Lent::new(m, "usize", None, None).with_reads(&COUNT_READS);
                assert_eq!(lent.equal(&again), Ok(Some(true)));
                assert_eq!(lent.equal(&lent), Ok(Some(true)));
            });
            lent
        });
        assert_eq!(kept.display(), Err(Error::Expired));
        assert!(format!("{kept:?}").starts_with("lent `usize`: "));
        let plain = Lent::new(super::nonce(0), "usize", None, None);
        assert_eq!(plain.display(), Ok(None));
        assert_eq!(plain.to_string(), "<usize>");
    }
}
