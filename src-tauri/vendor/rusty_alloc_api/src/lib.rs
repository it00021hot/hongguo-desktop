//! Safe Rust-native surface of rusty_alloc (plan §5.14).
//!
//! From M2, [`RustyAlloc`] is a real [`core::alloc::GlobalAlloc`]:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: rusty_alloc_api::RustyAlloc = rusty_alloc_api::RustyAlloc;
//! ```
//!
//! `Heap` and the `Allocator` trait impl land at M6. This crate stays a thin
//! veneer over the same internals as the C ABI — no separate code path, so
//! corpus numbers speak for Rust users too.

#![cfg_attr(not(test), no_std)]
#![deny(missing_docs)]

use core::alloc::{GlobalAlloc, Layout};

pub use rusty_alloc::{MI_COMPAT_VERSION, VERSION, version};

/// The global allocator handle (zero-sized).
pub struct RustyAlloc;

/// One machine word: every block the allocator hands out is aligned this far.
const WORD: usize = core::mem::size_of::<usize>();

/// The alignment the size classes already give every block of MORE than one
/// word: `bins::bin` rounds word counts up to even ones (upstream's
/// `MI_ALIGN2W`), and every page area starts on a slice boundary, so a block
/// of two words or more sits on a two-word boundary — 16 bytes on a 64-bit
/// target, `MAX_ALIGN_SIZE`. Only the one-word class is not, which is why a
/// request aligned between one and two words is raised to two words.
///
/// This is the alignment hashbrown asks for on every table (its SSE2 control
/// group is 16 bytes), so a Rust `HashMap` used to go through the aligned
/// path on every allocation and through allocate-copy-free on every realloc.
const NATURAL_ALIGN: usize = 2 * WORD;

/// A first-class heap (plan §5.14): `Drop` runs `mi_heap_delete` semantics
/// (blocks migrate to the thread's backing heap and stay valid) unless built
/// with [`Heap::new_destroyable`], where `Drop` releases every block at once.
/// The destroyable form inherits C's contract: callers must not touch its
/// blocks after drop (a lifetime-carrying `Allocator` impl that makes this
/// unrepresentable is the planned follow-up once allocator_api stabilizes).
pub struct Heap {
    hb: *mut rusty_alloc::init::HeapBox,
    destroy_on_drop: bool,
}

impl Heap {
    /// New heap; dropped ⇒ blocks migrate to the backing heap.
    ///
    /// # Panics
    /// When the OS refuses the heap's backing mapping (memory exhaustion) —
    /// a defined panic, matching std's convention for infallible
    /// constructors, rather than a null pointer carried into later use.
    pub fn new() -> Heap {
        let hb = rusty_alloc::init::create_heap(0, false, -1);
        assert!(!hb.is_null(), "rusty_alloc: heap creation failed (OOM)");
        Heap {
            hb,
            destroy_on_drop: false,
        }
    }

    /// New heap; dropped ⇒ every allocation is released wholesale
    /// (arena-style teardown).
    ///
    /// # Panics
    /// As [`Heap::new`], on memory exhaustion.
    pub fn new_destroyable() -> Heap {
        let hb = rusty_alloc::init::create_heap(0, true, -1);
        assert!(!hb.is_null(), "rusty_alloc: heap creation failed (OOM)");
        Heap {
            hb,
            destroy_on_drop: true,
        }
    }

    /// Allocate `layout`, borrowing the heap (so the block cannot outlive it).
    pub fn alloc(&self, layout: core::alloc::Layout) -> Option<core::ptr::NonNull<u8>> {
        // SAFETY: hb live (we own it), called on the owning thread by the
        // !Send/!Sync nature of raw-pointer fields.
        let p = unsafe {
            if layout.align() <= 8 {
                rusty_alloc::alloc::heap_malloc(self.hb, layout.size())
            } else {
                rusty_alloc::alloc::heap_malloc_aligned_at(
                    self.hb,
                    layout.size(),
                    layout.align(),
                    0,
                )
            }
        };
        core::ptr::NonNull::new(p)
    }

    /// Zeroed variant of [`alloc`](Self::alloc).
    pub fn alloc_zeroed(&self, layout: core::alloc::Layout) -> Option<core::ptr::NonNull<u8>> {
        // SAFETY: as alloc.
        let p = unsafe {
            if layout.align() <= 8 {
                rusty_alloc::alloc::heap_zalloc(self.hb, layout.size())
            } else {
                rusty_alloc::alloc::heap_zalloc_aligned_at(
                    self.hb,
                    layout.size(),
                    layout.align(),
                    0,
                )
            }
        };
        core::ptr::NonNull::new(p)
    }

    /// Free a block previously allocated from this heap.
    ///
    /// # Safety
    /// `p` came from this heap's alloc methods and is freed exactly once.
    pub unsafe fn dealloc(&self, p: core::ptr::NonNull<u8>) {
        // SAFETY: forwarded contract.
        unsafe { rusty_alloc::alloc::free(p.as_ptr()) }
    }

    /// Drain cross-thread frees and retire empty pages.
    pub fn collect(&self) {
        // SAFETY: owner thread (see alloc).
        unsafe { rusty_alloc::alloc::heap_collect(self.hb, true) }
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        // SAFETY: we own hb; exactly one of delete/destroy runs, once.
        unsafe {
            if self.destroy_on_drop {
                rusty_alloc::init::heap_destroy(self.hb);
            } else {
                rusty_alloc::init::heap_delete(self.hb);
            }
        }
    }
}

// SAFETY: GlobalAlloc contract — Layout-described allocation/free delegated to
// the rusty_alloc core, which returns blocks satisfying the layout's size and
// alignment (natural bins up to `NATURAL_ALIGN`; the aligned path above it)
// and accepts any such block back in `free` regardless of which thread frees
// it (M4: per-thread heaps, no lock — `free` routes by the segment's owner and
// hands cross-thread blocks to the loom-modeled remote protocol).
//
// Every method is `#[inline]`, as the `mimalloc` crate's are. rustc generates
// `__rust_alloc` and friends in the crate that declares `#[global_allocator]`,
// and without the hint each one was a shim that loaded `&self`, shuffled the
// arguments and jumped through the GOT into an out-of-line method — on every
// Rust allocation and every drop. With it the fast paths land in the shims and
// in their callers. Measured whole-program (callgrind, same output): a boxed-
// tree/buffer/`Rc` workload −17.9 %, a HashMap/BTreeMap/String one −4.8 %, for
// +1,888 and +2,784 bytes of text.
unsafe impl GlobalAlloc for RustyAlloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= WORD {
            rusty_alloc::alloc::malloc(layout.size())
        } else if layout.align() <= NATURAL_ALIGN {
            // See `NATURAL_ALIGN`: a class of two words or more is already
            // aligned this far, so only a one-word request needs raising.
            rusty_alloc::alloc::malloc(layout.size().max(NATURAL_ALIGN))
        } else {
            // SAFETY: `Layout` guarantees a power-of-two alignment, the one
            // precondition `malloc_aligned_pow2` adds over `malloc_aligned`.
            unsafe { rusty_alloc::alloc::malloc_aligned_pow2(layout.size(), layout.align()) }
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // `free_inline`, not `free`: `dealloc` IS a free and does nothing
        // else, the case `free_inline` exists for (the LD_PRELOAD export is
        // the other). Through `free` every Rust deallocation paid a `jmp`
        // into it and its null test; `GlobalAlloc` never passes null, and the
        // hint lets that test fold away.
        // SAFETY: GlobalAlloc contract — ptr came from `alloc`, is non-null
        // and is freed once.
        unsafe {
            core::hint::assert_unchecked(!ptr.is_null());
            rusty_alloc::alloc::free_inline(ptr)
        }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= WORD {
            rusty_alloc::alloc::zalloc(layout.size())
        } else if layout.align() <= NATURAL_ALIGN {
            rusty_alloc::alloc::zalloc(layout.size().max(NATURAL_ALIGN))
        } else {
            rusty_alloc::alloc::zalloc_aligned(layout.size(), layout.align())
        }
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() <= NATURAL_ALIGN {
            // Up to `NATURAL_ALIGN` the size classes carry the alignment, so
            // the in-place arm is open to these layouts too: a block that
            // stays keeps its address, and a move lands in a class of at
            // least two words. Before this, every realloc aligned above one
            // word (a growing `Vec<u128>`, say) was an allocate-copy-free.
            let new_size = if layout.align() <= WORD {
                new_size
            } else {
                new_size.max(NATURAL_ALIGN)
            };
            // SAFETY: GlobalAlloc contract — ptr live and non-null,
            // invalidated on move; our realloc preserves min(old, new) bytes.
            // The hint lets `realloc`'s null arm fold away.
            unsafe {
                core::hint::assert_unchecked(!ptr.is_null());
                rusty_alloc::alloc::realloc(ptr, new_size)
            }
        } else {
            // Above `NATURAL_ALIGN`. The block already satisfies
            // `layout.align()` and keeps it if it stays, so when the new size
            // fits and at least half the block stays in use — `realloc`'s own
            // in-place rule, which `mi_realloc_aligned` applies too — it
            // stays. This arm used to allocate, copy and free every time,
            // shrinks and fits included (bench/rust-globalalloc
            // `overaligned`).
            // SAFETY: GlobalAlloc contract — ptr is a live, non-null block
            // of ours.
            let usable = unsafe { rusty_alloc::alloc::usable_size(ptr) };
            if new_size <= usable && new_size >= usable / 2 {
                return ptr;
            }
            // Otherwise allocate through the aligned path, copy the bytes the
            // layout says are live, free.
            // SAFETY: forwarded GlobalAlloc contract.
            unsafe {
                let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
                let np = GlobalAlloc::alloc(self, new_layout);
                if !np.is_null() {
                    core::ptr::copy_nonoverlapping(ptr, np, layout.size().min(new_size));
                    GlobalAlloc::dealloc(self, ptr, layout);
                }
                np
            }
        }
    }
}
