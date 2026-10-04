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

//! Throughput of the local comparison paths.

#![allow(missing_docs)]
// Criterion's `benchmark_group` guard is flagged as an early-droppable temporary
// by `clippy::pedantic`; the macro-generated code is what controls its drop.
#![allow(clippy::pedantic)]
// Criterion's benchmark-group guard is intentionally held for the whole group;
// tightening its drop would end the measurement early.
#![allow(clippy::significant_drop_tightening)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use db_collation::Collation;

fn samples() -> Vec<String> {
    (0..10_000)
        .map(|i| format!("order_key_{:05}_{}", (i * 7919) % 100_000, i % 97))
        .collect()
}

fn bench(c: &mut Criterion) {
    let data = samples();
    let mysql = Collation::mysql_0900_ai_ci();
    let unicode = Collation::mysql_unicode_ci();
    let binary = Collation::binary();

    let mut group = c.benchmark_group("compare");
    group.bench_function("binary", |b| {
        b.iter(|| {
            let mut n = 0i64;
            for w in data.windows(2) {
                n += black_box(binary.compare(&w[0], &w[1]).unwrap()) as i64;
            }
            n
        });
    });
    group.bench_function("mysql_0900_ai_ci", |b| {
        b.iter(|| {
            let mut n = 0i64;
            for w in data.windows(2) {
                n += black_box(mysql.compare(&w[0], &w[1]).unwrap()) as i64;
            }
            n
        });
    });
    group.bench_function("mysql_unicode_ci", |b| {
        b.iter(|| {
            let mut n = 0i64;
            for w in data.windows(2) {
                n += black_box(unicode.compare(&w[0], &w[1]).unwrap()) as i64;
            }
            n
        });
    });
    group.finish();

    // Shape-specific paths: the streaming comparator short-circuits on the
    // first differing element, so equal keys and long common prefixes (worst
    // cases) must be measured apart from early differences (best case).
    let equal = "a".repeat(64);
    let late_a = format!("{}x", "a".repeat(63));
    let early_a = format!("b{}", "a".repeat(63));
    let prefix = "a".repeat(63);
    let long = "a".repeat(4096);
    // ½ expands to three elements under UCA; measure the expansion path.
    let expansion = "\u{bd}".repeat(64);

    let mut shapes = c.benchmark_group("mysql_0900_ai_ci/shapes");
    shapes.bench_function("equal_prefix_64", |b| {
        b.iter(|| black_box(mysql.compare(&equal, &equal).unwrap()));
    });
    shapes.bench_function("late_difference_64", |b| {
        b.iter(|| black_box(mysql.compare(&prefix, &late_a).unwrap()));
    });
    shapes.bench_function("early_difference_64", |b| {
        b.iter(|| black_box(mysql.compare(&prefix, &early_a).unwrap()));
    });
    shapes.bench_function("equal_4096", |b| {
        b.iter(|| black_box(mysql.compare(&long, &long).unwrap()));
    });
    shapes.bench_function("expansion_64", |b| {
        b.iter(|| black_box(mysql.compare(&expansion, &expansion).unwrap()));
    });
    shapes.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
