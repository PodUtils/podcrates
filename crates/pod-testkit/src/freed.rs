//! A check that no freed block of memory holds a secret (T-1023). A test
//! binary makes [`Scanning`] its allocator; [`scan`] runs a closure, and
//! each block that any thread frees in the scope is searched for a marker.
//! A code path that wipes each copy of a secret before its drop frees no
//! block that holds the marker. The binary must have one test: the other
//! tests of a binary free blocks in the same scope.
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: pod_testkit::freed::Scanning = pod_testkit::freed::Scanning;
//! ```
//!
//! Each block is zeroed at its allocation, so that the search reads only
//! bytes that were written. A growth in place (`realloc`) is a new block, a
//! copy and the free of the old block: the old block is searched too.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};

/// The system allocator, with a search of each freed block in a scope.
pub struct Scanning;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static ON: AtomicBool = AtomicBool::new(false);
static MARKER: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static MARKER_LEN: AtomicUsize = AtomicUsize::new(0);
static HITS: AtomicU64 = AtomicU64::new(0);
static LARGEST: AtomicUsize = AtomicUsize::new(0);

/// Count a hit when `block` holds the marker. No allocation: this runs in
/// the allocator.
fn search(block: &[u8]) {
    if !ON.load(Ordering::Acquire) {
        return;
    }
    let len = MARKER_LEN.load(Ordering::Acquire);
    let ptr = MARKER.load(Ordering::Acquire);
    if ptr.is_null() || len == 0 || block.len() < len {
        return;
    }
    // SAFETY: `scan` sets the pointer and the length of a `&'static [u8]`.
    let marker = unsafe { std::slice::from_raw_parts(ptr, len) };
    if block.windows(len).any(|w| w == marker) {
        HITS.fetch_add(1, Ordering::Relaxed);
        LARGEST.fetch_max(block.len(), Ordering::Relaxed);
    }
}

// SAFETY: each block comes from the system allocator with the layout of the
// caller, zeroed, so that each of its bytes is written before `search`
// reads it; a block is searched before it goes back to the system.
unsafe impl GlobalAlloc for Scanning {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        INSTALLED.store(true, Ordering::Relaxed);
        // SAFETY: the caller keeps the contract of `GlobalAlloc::alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        INSTALLED.store(true, Ordering::Relaxed);
        // SAFETY: the caller keeps the contract of `GlobalAlloc::alloc_zeroed`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` is a live block of `layout.size()` bytes, each one
        // written (zeroed at the allocation).
        search(unsafe { std::slice::from_raw_parts(ptr, layout.size()) });
        // SAFETY: `ptr` came from this allocator with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller gives a valid size for the alignment of `layout`.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        // SAFETY: the caller keeps the contract of `GlobalAlloc::realloc`.
        let new = unsafe { self.alloc(new_layout) };
        if !new.is_null() {
            // SAFETY: both blocks are live and do not overlap; the copy is
            // the smaller of the two sizes.
            unsafe { std::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size)) };
            // SAFETY: `ptr` came from this allocator with `layout`.
            unsafe { self.dealloc(ptr, layout) };
        }
        new
    }
}

/// What [`scan`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    /// The freed blocks that held the marker.
    pub blocks: u64,
    /// The size of the largest of them.
    pub largest: usize,
}

/// The value of `f`, and the freed blocks of each thread in its scope that
/// held `marker`; `None` where [`Scanning`] is not the allocator.
pub fn scan<R>(marker: &'static [u8], f: impl FnOnce() -> R) -> (R, Option<Found>) {
    // An allocation that proves the allocator, out of the scope.
    drop(std::hint::black_box(Box::new(0u8)));
    MARKER_LEN.store(marker.len(), Ordering::Release);
    MARKER.store(marker.as_ptr().cast_mut(), Ordering::Release);
    HITS.store(0, Ordering::Relaxed);
    LARGEST.store(0, Ordering::Relaxed);
    ON.store(true, Ordering::Release);
    let value = f();
    ON.store(false, Ordering::Release);
    let found = Found { blocks: HITS.load(Ordering::Relaxed), largest: LARGEST.load(Ordering::Relaxed) };
    (value, INSTALLED.load(Ordering::Relaxed).then_some(found))
}
