// Copyright 2026 db-collation contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Verifies that comparing two already-constructed strings does not allocate.
//!
//! The `MySQL` streaming comparison emits each character's weights into a fixed
//! stack buffer; the binary backend is a direct byte slice comparison. The whole
//! point is that `compare` is allocation-free for the string contents. This test
//! installs a counting global allocator and asserts the allocation count does
//! not grow with input length, for equal keys, late differences, and expansions
//! alike.

// Test-only: this file must define a `GlobalAlloc`, which is inherently `unsafe`
// FFI to the system allocator. The crate forbids `unsafe` in the library, not in
// this isolated measurement harness.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cmp::Ordering;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use db_collation::Collation;

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, AtomicOrdering::Relaxed);
        // SAFETY: delegating to the system allocator with the same layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` came from `System.alloc` via `alloc` above.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, AtomicOrdering::Relaxed);
        // SAFETY: `ptr`/`layout` came from this allocator; `new_size` is valid.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Run `f` and return how many allocations it performed.
///
/// Only ever called from the single `test_compare_does_not_allocate` test, so no
/// other test thread can allocate inside the measurement window.
fn allocs_of<F: FnOnce()>(f: F) -> usize {
    let before = ALLOCS.load(AtomicOrdering::Relaxed);
    f();
    ALLOCS.load(AtomicOrdering::Relaxed) - before
}

fn assert_no_alloc(c: &Collation, len: usize) {
    // Pre-build all inputs so construction/formatting never counts.
    let a = "a".repeat(len);
    let b = "a".repeat(len);
    let late_a = "a".repeat(len);
    let late_b = format!("{}b", "a".repeat(len - 1));
    let expansion = "\u{bd}".repeat(len); // ½ expands to three elements
    let expansion_b = "\u{bd}".repeat(len);

    // Warm any lazily-initialised state (none for MySQL, but be explicit).
    let _ = c.compare(&a, &b).unwrap();

    assert_eq!(
        allocs_of(|| assert_eq!(c.compare(&a, &b).unwrap(), Ordering::Equal)),
        0,
        "equal keys, len={len}"
    );
    assert_eq!(
        allocs_of(|| assert_eq!(c.compare(&late_a, &late_b).unwrap(), Ordering::Less)),
        0,
        "late diff, len={len}"
    );
    assert_eq!(
        allocs_of(|| assert_eq!(
            c.compare(&expansion, &expansion_b).unwrap(),
            Ordering::Equal
        )),
        0,
        "expansion, len={len}"
    );
}

// A single test so the binary and MySQL measurements never run concurrently:
// the allocation counter is process-global, and even with a lock another
// test thread's *untracked* setup allocations would be counted while the lock
// holder is measuring.
#[test]
fn test_compare_does_not_allocate() {
    let binary = Collation::binary();
    for len in [1, 16, 64, 1000] {
        assert_no_alloc(&binary, len);
    }

    #[cfg(feature = "mysql-uca")]
    {
        let mysql = Collation::mysql_0900_ai_ci();
        for len in [1, 16, 64, 1000] {
            assert_no_alloc(&mysql, len);
        }
    }
}
