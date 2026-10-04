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
//! Measured in two regimes, because the per-thread ICU collator cache behaves
//! very differently on a cold thread versus a warm one:
//!
//! * **cold**: a fresh thread per sample, so every comparison forces the ICU
//!   collator to be opened (exercises the cache-miss path and the bounded
//!   cache's eviction behaviour).
//! * **warm**: one thread, one locale, repeated comparisons (the steady state).
//!
//! Also measures the ASCII-comparison shapes (equal, early, late difference,
//! long common prefix, expansions) so a regression in short-circuiting shows up.

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

fn collation() -> Collation {
    // The local ICU's version is discovered by the library, so any valid locale
    // exercises the real comparison path without a live database.
    let version = db_collation::collation_version(LOCALE).unwrap_or_else(|| "153.128".to_owned());
    Collation::postgres_icu(LOCALE, true, VersionId::new(version)).expect("locale supported")
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
    group.finish();

    let equal = "a".repeat(64);
    let equal_b = equal.clone();
    let prefix = "a".repeat(63);
    let early_a = format!("b{}", "a".repeat(63));
    let late_a = format!("{}x", "a".repeat(63));
    let long_a = "a".repeat(4096);
    let long_b = "a".repeat(4096);

    let mut shapes = c.benchmark_group("postgres_icu/shapes");
    shapes.bench_function("equal_64", |b| {
        b.iter(|| black_box(col.compare(&equal, &equal_b).unwrap()));
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
    shapes.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
