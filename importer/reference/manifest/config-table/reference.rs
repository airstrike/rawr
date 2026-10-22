//! Standalone pinned std/Fx allocation and ordered-map observation harness.
use std::alloc::{GlobalAlloc, Layout, System};
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
mod scenario;

struct Allocator;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static ALIGN: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ACTIVE.load(Relaxed) {
            COUNT.fetch_add(1, Relaxed);
            BYTES.store(layout.size(), Relaxed);
            ALIGN.store(layout.align(), Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn main() {
    let keys = scenario::keys(|key| rustc_hash::FxBuildHasher.hash_one(key));
    let mut map = rustc_hash::FxHashMap::<&str, usize>::default();
    for (operation, key, value) in scenario::operations() {
        COUNT.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        ALIGN.store(0, Relaxed);
        ACTIVE.store(true, Relaxed);
        match operation {
            'i' => { map.insert(&keys[key], value); }
            'm' => { map.entry(&keys[key]).or_insert(value); }
            'r' => { map.remove(keys[key].as_str()); }
            _ => unreachable!(),
        }
        ACTIVE.store(false, Relaxed);
        assert!(COUNT.load(Relaxed) <= 1);
        print!("{} {} {}", map.capacity(), BYTES.load(Relaxed), ALIGN.load(Relaxed));
        for (key, value) in &map { print!(" {key}:{value}"); }
        println!();
    }
}
