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

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::marker::PhantomData;
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
/// name of its type, and the accessors a trampoline made for it. Copying
/// it copies the claim, not the value, and every copy expires with the
/// lend.
#[derive(Clone, Copy)]
pub struct Lent {
    nonce: Nonce,
    name: &'static str,
    mutable: Option<Accessor>,
    shared: Option<Accessor>,
}

impl std::fmt::Debug for Lent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lent `{}`", self.name)
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
        }
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
    FRAMES.with(|frames| {
        if let Some(frame) = frames
            .borrow_mut()
            .iter_mut()
            .rev()
            .find(|frame| frame.nonce == nonce)
        {
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
/// script code (`island::__rawr_island_call_v2`). The value is reached by
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
    FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        let frame = frames
            .iter_mut()
            .rev()
            .find(|frame| frame.nonce == nonce)
            .ok_or(Error::Expired)?;
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
    slots: Vec<Option<Box<dyn Erased<'r> + 'r>>>,
    _r: PhantomData<fn(&'r ()) -> &'r ()>,
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
            slots: Vec::new(),
            _r: PhantomData,
        }
    }

    /// The nonce the region is lent by while it is open.
    #[must_use]
    pub const fn nonce(&self) -> Nonce {
        self.nonce
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
    /// is then [`Error::Moved`].
    pub fn take<F: Regional>(&mut self, handle: Scoped) -> Result<F::Of<'r>, Error> {
        self.slot_of::<F>(handle)?;
        let slot = self.slots[handle.index as usize]
            .take()
            .ok_or(Error::Moved)?;
        // SAFETY: as in `get`: the box holds a `Slot<'r, F>`, so it is
        // turned back into the box it was made as.
        let slot = unsafe { Box::from_raw(Box::into_raw(slot).cast::<Slot<'r, F>>()) };
        Ok(slot.0)
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

/// A [`Region`] as a family, so it is lent whole.
pub struct RegionF;

impl Family for RegionF {
    type Of<'x> = Region<'x>;
}

/// Run `body` with `region` lent, by its own nonce, to [`with`] and
/// [`with_ref`] of [`RegionF`] on this thread: the rest of a scope whose
/// values it holds. Every [`Scoped`] handle into it expires when `body`
/// returns or unwinds.
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
}
