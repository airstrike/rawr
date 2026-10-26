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
        }
    }

    /// The error a number stands for; an unknown one is a mismatch.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Expired,
            3 => Self::Busy,
            4 => Self::ReadOnly,
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

fn push<R>(key: TypeId, place: *mut (), writable: bool, body: impl FnOnce(Nonce) -> R) -> R {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let nonce = Nonce(NEXT.fetch_add(1, Ordering::Relaxed));
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
