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

//! Throughput of the Oracle UCA (DUCET) comparison path.

#![allow(missing_docs)]
#![allow(clippy::pedantic)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use db_collation::{Collation, OracleCollation, OracleUcaVersion};

fn bench(c: &mut Criterion) {
    let col = Collation::oracle(OracleCollation::uca_ducet(OracleUcaVersion::Uca1210))
        .expect("UCA1210_DUCET is supported");

    let ascii: Vec<String> = (0..2_000)
        .map(|i| format!("order_key_{:05}_{}", (i * 7919) % 100_000, i % 97))
        .collect();
    let accented: Vec<String> = (0..2_000)
        .map(|i| format!("café_{i}_naïve_{}", i % 89))
        .collect();

    let mut group = c.benchmark_group("oracle_uca1210_ducet");
    group.bench_function("ascii", |b| {
        b.iter(|| {
            let mut n = 0i64;
            for w in ascii.windows(2) {
                n += black_box(col.compare(&w[0], &w[1]).unwrap()) as i64;
            }
            n
        });
    });
    group.bench_function("accented", |b| {
        b.iter(|| {
            let mut n = 0i64;
            for w in accented.windows(2) {
                n += black_box(col.compare(&w[0], &w[1]).unwrap()) as i64;
            }
            n
        });
    });
    // Over-length refusal: the element array is built before the key-length
    // check, so this exercises the scaling of the refuse path itself.
    let long = "a".repeat(8_000);
    group.bench_function("over_limit_refusal", |b| {
        b.iter(|| black_box(col.compare(black_box(&long), black_box("b")).is_err()));
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
