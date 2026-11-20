//! The bench's global allocator: the system's, counting each allocation
//! and its bytes, so a workload's allocations are read as deltas around
//! one run. It is the bench binary's alone; rawr's own builds never see it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

/// The system allocator, counted.
struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// SAFETY: every call goes to `System` unchanged; the counts are relaxed
// atomic adds, which allocate nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: the caller's contract for `alloc` is `System::alloc`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: the caller's contract is `System::alloc_zeroed`'s.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, which is `System`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        // SAFETY: `ptr` came from this allocator, which is `System`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// How many allocations (a reallocation counts as one) and how many bytes
/// they asked for, since the process started.
#[derive(Clone, Copy)]
pub struct Allocated {
    pub count: u64,
    pub bytes: u64,
}

impl Allocated {
    /// The totals now.
    pub fn now() -> Self {
        Self {
            count: ALLOCATIONS.load(Ordering::Relaxed),
            bytes: BYTES.load(Ordering::Relaxed),
        }
    }

    /// What was allocated from `earlier` to `self`.
    pub fn since(self, earlier: Self) -> Self {
        Self {
            count: self.count - earlier.count,
            bytes: self.bytes - earlier.bytes,
        }
    }
}
