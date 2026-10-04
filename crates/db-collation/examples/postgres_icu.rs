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

//! Reproduce a `PostgreSQL` ICU collation locally.
//!
//! Run with:
//! `cargo run --example postgres_icu --features postgres-icu`
//!
//! The `version` argument is the source's `pg_collation_actual_version(oid)`
//! (the `collversion` column). The caller reads it from the source catalog and
//! passes it in; this crate never queries the database. If it does not match the
//! linked ICU's data version, construction fails with
//! [`db_collation::Error::VersionMismatch`] and the caller must compare on the
//! source.

use db_collation::{Collation, Error, VersionId};

fn main() {
    // Replace with the value from `pg_collation_actual_version(oid)`. The
    // version is mandatory: an empty value or the sentinel `"unknown"` is
    // refused, because an unchecked identity is not an exact comparison.
    let source_version = VersionId::new("153.128");

    let collation = match Collation::postgres_icu("und", true, source_version) {
        Ok(c) => c,
        Err(Error::VersionMismatch { source, local }) => {
            eprintln!("version mismatch (source {source}, local {local}); compare on the source");
            return;
        }
        Err(e) => {
            eprintln!("unsupported: {e}");
            return;
        }
    };

    println!("deterministic ICU und:");
    println!(
        "  a == A  -> {}",
        collation.equal("a", "A").unwrap_or(false)
    );

    let mut words = vec!["resume", "résumé", "resumes", "Resume"];
    words.sort_by(|a, b| collation.compare(a, b).expect("ICU comparison failed"));
    println!("  sorted: {words:?}");
}
