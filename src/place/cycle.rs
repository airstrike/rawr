//! Statics of a body that refer to each other (D865), as tracing's `META`
//! and `__CALLSITE` do. A static a sibling's initializer names before it is
//! written gets its memory first, so the initializer can keep its address.
//! Each value is then moved into its memory or a leaked allocation of its
//! own. Once the last of them is written, every word of each one that
//! holds the address of one of them is written again, with that address
//! taken from its allocation's own pointer. Only then is any of them read
//! through those words.
//!
//! Writing a value ends the borrows taken of its memory before, under
//! Rust's aliasing rules (Stacked and Tree Borrows), so the address a
//! sibling kept is right but reading through it would be undefined
//! behaviour. The second write changes no byte; it gives each word the
//! allocation's own permission. The words are found from rustc's layout of
//! the value's type (the import driver's), never by reading the value's
//! bytes, whose padding is uninitialized. The tests below check the whole
//! construction under Miri, in both models (see `tests`).

use std::alloc::Layout;
use std::any::{Any, TypeId};
use std::mem::MaybeUninit;
use std::ptr::NonNull;

/// Leaked memory for one static's value, allocated before the value is
/// made, the first time a sibling's initializer takes its address.
pub(super) struct Memory {
    base: NonNull<u8>,
    layout: Layout,
    ty: TypeId,
    /// The memory as the value it holds, once written.
    view: fn(NonNull<u8>) -> NonNull<dyn Any>,
}

// SAFETY: `Memory` is the address of leaked memory and holds no value. Only
// `Memory::write` writes it, and the type it is for is a static's, which is
// `Sync` (rustc's E0277).
unsafe impl Send for Memory {}

impl Memory {
    /// Memory for a `T`, leaked.
    pub(super) fn of<T: 'static>() -> Self {
        let memory: &'static mut MaybeUninit<T> = Box::leak(Box::new(MaybeUninit::uninit()));
        Self {
            base: NonNull::from(memory).cast(),
            layout: Layout::new::<T>(),
            ty: TypeId::of::<T>(),
            view: |base| base.cast::<T>(),
        }
    }

    /// The memory as the `&'static T` a sibling's initializer keeps, before
    /// the value is in it; `None` if it is for another type.
    pub(super) fn address<T: 'static>(&self) -> Option<&'static T> {
        if self.ty != TypeId::of::<T>() {
            return None;
        }
        // SAFETY: the memory was allocated for a `T`, so the reference is
        // aligned and in bounds, and it is leaked, so it lives as long as
        // the program. The value isn't written yet: the reference is only
        // kept, by the const fn an initializer calls, and never read. Every
        // copy of it is in a value `Cycle::close` writes again from `base`
        // before anything reads it.
        Some(unsafe { self.base.cast::<T>().as_ref() })
    }

    /// Whether `value` is of the type this memory is for.
    pub(super) fn fits(&self, value: &dyn Any) -> bool {
        value.type_id() == self.ty
    }

    /// Move `value`, which [`Memory::fits`], into the memory.
    ///
    /// # Panics
    ///
    /// If `value` is of another type, which the caller checks first.
    pub(super) fn write(self, value: Box<dyn Any>) -> Written {
        assert!(self.fits(&*value), "a static's value is of its type");
        let source: *mut dyn Any = Box::into_raw(value);
        // SAFETY: `source` is a live value of the memory's type, and the
        // memory, allocated for that type and not written yet, is apart
        // from it. The value's bytes move there and its box is freed
        // without dropping it, as a move does; a box of a zero-sized value
        // holds no allocation.
        unsafe {
            std::ptr::copy_nonoverlapping(
                source.cast::<u8>(),
                self.base.as_ptr(),
                self.layout.size(),
            );
            if self.layout.size() != 0 {
                std::alloc::dealloc(source.cast(), self.layout);
            }
        }
        Written {
            value: (self.view)(self.base),
        }
    }
}

/// A static's value, written, where it stays for the program's life: its
/// allocation's own pointer, from which each reference to it is taken.
pub(super) struct Written {
    value: NonNull<dyn Any>,
}

// SAFETY: a static's type is `Sync` (rustc's E0277), and `Written` gives
// only shared access to its value.
unsafe impl Send for Written {}
// SAFETY: as for `Send`.
unsafe impl Sync for Written {}

impl Written {
    /// `value`, moved into a leaked allocation of its own.
    pub(super) fn leak(value: Box<dyn Any>) -> Self {
        Self {
            value: NonNull::from(Box::leak(value)),
        }
    }

    /// The value, which lives as long as the program.
    pub(super) fn get(&self) -> &'static dyn Any {
        // SAFETY: the value is written, and its allocation is leaked and
        // never written through a reference. Only `Cycle::close` writes it
        // again, from this pointer, before the cycle it is in is read; the
        // references taken while the cycle is built are kept only in its
        // members, where `close` replaces them.
        unsafe { self.value.as_ref() }
    }

    /// The allocation's own pointer, which a `&'static` result gives.
    pub(super) fn pointer(&self) -> *const dyn Any {
        self.value.as_ptr()
    }
}

/// Where a type keeps the address words of its pointers that may hold a
/// static's address, as rustc lays it out.
pub(super) struct Pointers {
    size: usize,
    offsets: Box<[usize]>,
}

impl Pointers {
    /// The pointer words of a type of `size` bytes at `offsets`; `None` if
    /// one doesn't fit in it.
    ///
    /// # Safety
    ///
    /// `size` and `offsets` are rustc's layout of the type of each value
    /// they are given for, by [`Cycle::join`]: each offset is where a value
    /// of the type has the address word of a reference or raw pointer (the
    /// data word of a wide one), outside any enum variant or union, so the
    /// word is initialized in any value of the type.
    pub(super) unsafe fn new(size: usize, offsets: Box<[usize]>) -> Option<Self> {
        let word = std::mem::size_of::<*const u8>();
        offsets
            .iter()
            .all(|offset| offset.checked_add(word).is_some_and(|end| end <= size))
            .then_some(Self { size, offsets })
    }
}

/// The statics written since a sibling's initializer kept the address of
/// one not written yet, with their pointer words.
pub(super) struct Cycle {
    members: Vec<Member>,
}

/// A static in a cycle: its allocation's own pointer, its size and its
/// pointer words.
struct Member {
    base: NonNull<u8>,
    size: usize,
    offsets: Box<[usize]>,
}

// SAFETY: a member is a static's value, whose type is `Sync`, and `Cycle`
// only writes its pointer words, before anything reads them.
unsafe impl Send for Cycle {}

impl Cycle {
    /// A cycle no static is in yet.
    pub(super) const fn new() -> Self {
        Self {
            members: Vec::new(),
        }
    }

    /// Add the written static `written`, whose type has its pointer words
    /// at `pointers`; `false`, adding nothing, when `pointers` is for a
    /// type of another size.
    pub(super) fn join(&mut self, written: &Written, pointers: Pointers) -> bool {
        if std::mem::size_of_val(written.get()) != pointers.size {
            return false;
        }
        self.members.push(Member {
            base: written.value.cast(),
            size: pointers.size,
            offsets: pointers.offsets,
        });
        true
    }

    /// Write again each pointer word of each member that holds the address
    /// of a member, or of a place in one, with that address taken from the
    /// member's own pointer. The members may be read from then on.
    pub(super) fn close(self) {
        for member in &self.members {
            for &offset in &member.offsets {
                // SAFETY: the offsets are the member's layout (`Pointers::new`
                // and the size `join` checked), so the word at `offset` is
                // inside the value and is a pointer's initialized address
                // word. The member's own pointer may write the value, which
                // no reference reads before the cycle closes.
                let word = unsafe { member.base.as_ptr().add(offset) }.cast::<*const u8>();
                // SAFETY: as above.
                let address = unsafe { word.read_unaligned() }.addr();
                let Some(target) = self.members.iter().find(|target| target.holds(address)) else {
                    continue;
                };
                let base = target.base.as_ptr();
                let repointed = base.wrapping_add(address - base.addr());
                // SAFETY: as above; the new word is the same address.
                unsafe { word.write_unaligned(repointed.cast_const()) };
            }
        }
    }
}

impl Default for Cycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Member {
    fn holds(&self, address: usize) -> bool {
        let start = self.base.as_ptr().addr();
        (start..start + self.size).contains(&address)
    }
}

#[cfg(test)]
mod tests {
    //! The construction on its own, under Miri. Statics are leaked, so
    //! Miri's leak check is off:
    //!
    //! ```text
    //! MIRIFLAGS=-Zmiri-ignore-leaks cargo +nightly miri test -p rawr --lib place::cycle
    //! MIRIFLAGS="-Zmiri-ignore-leaks -Zmiri-tree-borrows" cargo +nightly miri test -p rawr --lib place::cycle
    //! ```
    //!
    //! Each passes with `-Zmiri-strict-provenance` too. Without
    //! `Cycle::close`, both report the read through a sibling's address as
    //! undefined behaviour.

    use super::{Cycle, Memory, Pointers, Written};
    use std::sync::atomic::{AtomicU8, Ordering};

    trait Named {
        fn name(&self) -> &'static str;
    }

    /// Two statics that refer to each other, as tracing's metadata and
    /// callsite do: one through a wide `&dyn` pointer, beside a `&str`.
    struct Meta {
        name: &'static str,
        site: &'static dyn Named,
    }

    struct Site {
        hits: AtomicU8,
        meta: &'static Meta,
    }

    impl Meta {
        const fn new(name: &'static str, site: &'static Site) -> Self {
            Self { name, site }
        }
    }

    impl Site {
        const fn new(meta: &'static Meta) -> Self {
            Self {
                hits: AtomicU8::new(0),
                meta,
            }
        }

        fn hit(&'static self) -> u8 {
            self.hits.fetch_add(1, Ordering::AcqRel) + 1
        }
    }

    impl Named for Site {
        fn name(&self) -> &'static str {
            self.meta.name
        }
    }

    fn pointers<T>(offsets: &[usize]) -> Pointers {
        // SAFETY: each offset is `offset_of!` a reference field of `T`, the
        // driver's layout in miniature.
        unsafe { Pointers::new(std::mem::size_of::<T>(), offsets.into()) }
            .expect("each offset is a field's")
    }

    /// tracing's order: the callsite's initializer declares the metadata,
    /// which keeps the callsite's address before the callsite is written.
    #[test]
    fn statics_that_refer_to_each_other_are_read_through_their_own_allocations() {
        let site = Memory::of::<Site>();
        let meta = Meta::new("event", site.address::<Site>().expect("memory for a Site"));
        let meta = Written::leak(Box::new(meta));
        let meta_ref: &'static Meta = meta.get().downcast_ref().expect("a Meta");
        let value: Box<dyn std::any::Any> = Box::new(Site::new(meta_ref));
        assert!(site.fits(&*value));
        let site = site.write(value);

        let mut cycle = Cycle::default();
        assert!(cycle.join(&meta, pointers::<Meta>(&[std::mem::offset_of!(Meta, site)])));
        assert!(cycle.join(&site, pointers::<Site>(&[std::mem::offset_of!(Site, meta)])));
        cycle.close();

        let site: &'static Site = site.get().downcast_ref().expect("a Site");
        let meta: &'static Meta = meta.get().downcast_ref().expect("a Meta");
        assert!(std::ptr::eq(site.meta, meta));
        assert!(std::ptr::addr_eq(meta.site, site));
        assert_eq!(meta.site.name(), "event");
        assert_eq!(site.meta.site.name(), "event");
        assert_eq!(site.hit(), 1);
        assert_eq!(site.meta.site.name(), "event");
        assert_eq!(site.hit(), 2);
    }

    /// The other order, both written into memory taken before: the first
    /// keeps the second's address, and the second the first's, written.
    #[test]
    fn a_static_written_into_memory_a_sibling_kept_is_read_through_it() {
        let meta = Memory::of::<Meta>();
        let site = Site::new(meta.address::<Meta>().expect("memory for a Meta"));
        let site = Written::leak(Box::new(site));
        let site_ref: &'static Site = site.get().downcast_ref().expect("a Site");
        let value: Box<dyn std::any::Any> = Box::new(Meta::new("written", site_ref));
        let meta = meta.write(value);

        let mut cycle = Cycle::default();
        assert!(cycle.join(&site, pointers::<Site>(&[std::mem::offset_of!(Site, meta)])));
        assert!(cycle.join(&meta, pointers::<Meta>(&[std::mem::offset_of!(Meta, site)])));
        cycle.close();

        let site: &'static Site = site.get().downcast_ref().expect("a Site");
        assert_eq!(site.hit(), 1);
        assert_eq!(site.meta.name, "written");
        assert_eq!(site.meta.site.name(), "written");
    }

    #[test]
    fn pointers_of_another_size_join_nothing() {
        let meta = Written::leak(Box::new(7_u64));
        let mut cycle = Cycle::default();
        assert!(!cycle.join(&meta, pointers::<Site>(&[])));
    }
}
