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

//! Throughput of the `PostgreSQL` ICU comparison path.
//!
//! Measured in three regimes, because the per-thread ICU collator cache behaves
//! very differently across them:
//!
//! * **warm**: one thread, one locale, repeated comparisons (the steady state;
//!   every lookup is a cache hit).
//! * **cold**: a fresh thread per sample, so every comparison forces the ICU
//!   collator to be opened (exercises the cache-miss path).
//! * **eviction**: a single thread cycling through more distinct locales than
//!   the per-thread cache holds, so each pass past the cap clears the cache and
//!   forces cold opens. This is the only regime that exercises eviction.
//!
//! It also measures the comparison *shapes* so a regression in short-circuiting
//! shows up: equal, early and late differences, a shared long prefix, and an
//! expansion (one character whose weight is several collation elements).

#![allow(missing_docs)]
// Criterion's `benchmark_group`/`thread::scope` guards are flagged as
// early-droppable temporaries by `clippy::pedantic`.
#![allow(clippy::pedantic)]
// Criterion's benchmark-group guard is intentionally held for the whole group;
// tightening its drop would end the measurement early.
#![allow(clippy::significant_drop_tightening)]

use std::hint::black_box;
use std::thread;

use criterion::{Criterion, criterion_group, criterion_main};
use db_collation::{Collation, VersionId};

const LOCALE: &str = "und";

/// More distinct, valid locales than the per-thread cache holds
/// (`MAX_CACHED_LOCALES` is 64). Cycling these on one thread forces the cache
/// to clear and reopen collators, exercising the eviction path.
const MANY_LOCALES: &[&str] = &[
    "af", "am", "ar", "as", "az", "be", "bg", "bn", "bo", "bs", "ca", "cs", "cy", "da", "de", "dz",
    "el", "en", "eo", "es", "et", "eu", "fa", "fi", "fo", "fr", "ga", "gl", "gu", "ha", "he", "hi",
    "hr", "hu", "hy", "id", "is", "it", "ja", "ka", "kk", "km", "kn", "ko", "ky", "lb", "lo", "lt",
    "lv", "mk", "ml", "mn", "mr", "ms", "mt", "my", "nb", "ne", "nl", "nn", "or", "pa", "pl", "ps",
    "pt", "ro", "ru", "si", "sk", "sl", "sq", "sr", "sv", "sw", "ta", "te", "th", "ti", "tr", "uk",
    "ur", "uz", "vi", "zh", "zu",
];

fn collation() -> Collation {
    // The local ICU's version is discovered by the library, so any valid locale
    // exercises the real comparison path without a live database.
    let version = db_collation::collation_version(LOCALE).unwrap_or_else(|| "153.128".to_owned());
    Collation::postgres_icu(LOCALE, true, VersionId::new(version)).expect("locale supported")
}

fn collation_for(locale: &str) -> Option<Collation> {
    let version = db_collation::collation_version(locale)?;
    Collation::postgres_icu(locale, true, VersionId::new(version)).ok()
}

fn samples() -> Vec<String> {
    (0..2_000)
        .map(|i| format!("order_key_{:05}_{}", (i * 7919) % 100_000, i % 97))
        .collect()
}

fn bench(c: &mut Criterion) {
    let col = collation();
    let data = samples();

    let mut group = c.benchmark_group("postgres_icu");
    group.bench_function("warm", |b| {
        // Prime the cache on this thread before measuring.
        let _ = col.compare(&data[0], &data[1]).unwrap();
        b.iter(|| {
            let mut n = 0i64;
            for w in data.windows(2) {
                n += black_box(col.compare(&w[0], &w[1]).unwrap()) as i64;
            }
            n
        });
    });
    group.bench_function("cold_thread_per_sample", |b| {
        b.iter(|| {
            thread::scope(|s| {
                for w in data.windows(2) {
                    s.spawn(|| {
                        black_box(col.compare(&w[0], &w[1]).unwrap());
                    });
                }
            });
        });
    });
    group.bench_function("eviction_cycling_locales", |b| {
        // Build one collation per locale up front (construction is cheap); the
        // cost measured is the cache miss/eviction on each comparison.
        let locales: Vec<Collation> = MANY_LOCALES
            .iter()
            .filter_map(|l| collation_for(l))
            .collect();
        let a = "abcdefghij";
        let b2 = "abcdefghiZ";
        b.iter(|| {
            let mut n = 0i64;
            for lc in &locales {
                // Each distinct locale past the cap evicts and reopens.
                n += black_box(lc.compare(a, b2).unwrap()) as i64;
            }
            n
        });
    });
    group.finish();

    let equal = "a".repeat(64);
    let prefix = "a".repeat(63);
    let early_a = format!("b{}", "a".repeat(63));
    let late_a = format!("{}x", "a".repeat(63));
    // Two DIFFERENT strings sharing a long common prefix (the earlier version
    // compared a string with itself, which only measured the fast equal path).
    let long_a = format!("{}x", "a".repeat(4095));
    let long_b = format!("{}y", "a".repeat(4095));
    // One character (U+0066 'f' has a two-element expansion in `und`-tailored
    // maps only for some locales; use a ligature with a multi-element weight).
    let expansion_a = "\u{FB01}".to_owned(); // ﬁ LATIN SMALL LIGATURE FI
    let expansion_b = "fi".to_owned();

    let mut shapes = c.benchmark_group("postgres_icu/shapes");
    shapes.bench_function("equal_64", |b| {
        b.iter(|| black_box(col.compare(&equal, &equal).unwrap()));
    });
    shapes.bench_function("early_difference_64", |b| {
        b.iter(|| black_box(col.compare(&prefix, &early_a).unwrap()));
    });
    shapes.bench_function("late_difference_64", |b| {
        b.iter(|| black_box(col.compare(&prefix, &late_a).unwrap()));
    });
    shapes.bench_function("long_prefix_4096", |b| {
        b.iter(|| black_box(col.compare(&long_a, &long_b).unwrap()));
    });
    shapes.bench_function("expansion_ligature", |b| {
        b.iter(|| black_box(col.compare(&expansion_a, &expansion_b).unwrap()));
    });
    shapes.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
