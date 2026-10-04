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

//! Per-thread cache of ICU collators.
//!
//! `rust_icu_ucol::UCollator` is neither `Send` nor `Sync`, so it cannot live in
//! a shared structure. We keep one per thread and key by locale. This keeps the
//! public [`crate::Collation`] `Send + Sync` while avoiding repeated open cost.

use std::cell::RefCell;
use std::collections::HashMap;

use rust_icu_ucol::UCollator;

/// Upper bound on distinct locales cached per thread. A `Collation` typically
/// uses one locale; the bound only guards against pathological callers that
/// build unbounded distinct descriptions on one worker. When exceeded, the
/// cache is cleared rather than grown without limit.
const MAX_CACHED_LOCALES: usize = 64;

thread_local! {
    static COLLATORS: RefCell<HashMap<String, UCollator>> = RefCell::new(HashMap::new());
}

/// Access (or lazily open) the thread's collator for `locale`.
///
/// Returns `None` if ICU cannot open the locale on this thread even though it
/// validated earlier (e.g. a transient resource failure), so the caller can
/// surface a typed error instead of panicking.
pub(crate) fn with<R>(locale: &str, f: impl FnOnce(&UCollator) -> R) -> Option<R> {
    COLLATORS.with(|cell| {
        let mut map = cell.borrow_mut();

        // Warm path: look up by the borrowed key without allocating.
        if let Some(collator) = map.get(locale) {
            return Some(f(collator));
        }

        // Cold path: open the collator, then insert. On failure return None.
        let collator = UCollator::try_from(locale).ok()?;
        if map.len() >= MAX_CACHED_LOCALES {
            map.clear();
        }
        let collator = map.entry(locale.to_owned()).or_insert(collator);
        Some(f(collator))
    })
}
