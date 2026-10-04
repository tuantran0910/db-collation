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

//! Differential-harness candidate.
//!
//! This is the *real* `db-collation` crate used as the candidate under test: it
//! reads the harness corpus and specs, orders the corpus with the shipped
//! library, and writes the same order/rank JSON shape the differ consumes. It
//! performs no database I/O — the oracle lives in the Python harness.
//!
//! ```text
//! harness-candidate <corpus.json> <specs.json> <out.json>
//! ```
//!
//! For `PostgreSQL` specs the candidate is built against (and runs inside) the
//! same image as the oracle, so it links the identical ICU; construction with
//! the source's `collversion` therefore exercises the version gate. A spec the
//! library refuses is reported with `supported: false` and the reason.

use core::cell::RefCell;
use core::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::process::ExitCode;

use serde::{Deserialize, Serialize};

use db_collation::{Collation, VersionId};

#[derive(Debug, Deserialize)]
struct Scenario {
    id: i64,
    /// Scenario class; unused here but part of the corpus format.
    #[serde(rename = "cat")]
    #[allow(dead_code)]
    category: String,
    s: String,
}

#[derive(Debug, Deserialize)]
struct Spec {
    id: String,
    engine: String,
    collation: String,
    #[serde(default = "default_true")]
    deterministic: bool,
    /// The source's collation data version (`collversion`); `PostgreSQL` only.
    #[serde(default)]
    version: Option<String>,
    /// Custom ICU rules (`collicurules`); `PostgreSQL` only.
    #[serde(default)]
    rules: Option<String>,
    /// Source encoding; `PostgreSQL` only.
    #[serde(default = "default_utf8")]
    encoding: String,
}

const fn default_true() -> bool {
    true
}

fn default_utf8() -> String {
    "UTF8".to_owned()
}

/// How a spec resolved, kept distinct so the harness can tell a clean
/// construction refusal from a runtime comparison failure.
///
/// Construction refusal (`Refused`) is the *expected* outcome for an
/// unsupported configuration. A runtime failure (`Error`) — a comparison that
/// errored after successful construction — is always a defect and must never be
/// accepted as a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Ok,
    Refused,
    Error,
}

#[derive(Debug, Serialize)]
struct Out {
    spec_id: String,
    order: Vec<i64>,
    rank: BTreeMap<i64, i64>,
    version: Option<String>,
    supported: bool,
    outcome: Outcome,
    note: String,
}

impl Out {
    fn refused(spec: &Spec, note: String) -> Self {
        Self {
            spec_id: spec.id.clone(),
            order: Vec::new(),
            rank: BTreeMap::new(),
            version: report_version(spec),
            supported: false,
            outcome: Outcome::Refused,
            note,
        }
    }

    fn error(spec: &Spec, note: String) -> Self {
        Self {
            spec_id: spec.id.clone(),
            order: Vec::new(),
            rank: BTreeMap::new(),
            version: report_version(spec),
            supported: false,
            outcome: Outcome::Error,
            note,
        }
    }
}

/// Why a spec could not be turned into a collation.
enum BuildError {
    /// The library refused the configuration (expected for unsupported specs).
    Refused(String),
    /// The spec itself is malformed — a harness defect, not a refusal.
    Invalid(String),
}

/// Construct the collation for a spec using the real library, or classify why it
/// could not be built.
fn build(spec: &Spec) -> Result<Collation, BuildError> {
    match spec.engine.as_str() {
        "postgres" => {
            let version = spec.version.clone().unwrap_or_else(|| "unknown".to_owned());
            let mut description = db_collation::PostgresCollation::icu(
                spec.collation.clone(),
                spec.deterministic,
                VersionId::new(version),
            )
            .with_encoding(spec.encoding.clone());
            if let Some(rules) = &spec.rules {
                description = description.with_rules(rules.clone());
            }
            Collation::postgres(description).map_err(|e| BuildError::Refused(e.to_string()))
        }
        "mysql" => Collation::from_mysql_name_unspecified_version(&spec.collation)
            .map_err(|e| BuildError::Refused(e.to_string())),
        other => Err(BuildError::Invalid(format!("unknown engine: {other}"))),
    }
}

/// The collation data version the library would use for a spec, for the harness
/// to assert against the source's reported version.
fn report_version(spec: &Spec) -> Option<String> {
    if spec.engine == "postgres" {
        db_collation::collation_version(&spec.collation)
    } else {
        None
    }
}

/// Order the corpus and assign dense ranks by equivalence class.
///
/// Returns `Err` if any comparison fails, so a backend error is surfaced rather
/// than silently collapsed to equality.
fn order_and_rank(
    collation: &Collation,
    corpus: &[Scenario],
) -> Result<(Vec<i64>, BTreeMap<i64, i64>), String> {
    let strings: HashMap<i64, &str> = corpus.iter().map(|s| (s.id, s.s.as_str())).collect();
    let failure: RefCell<Option<String>> = RefCell::new(None);

    let compare = |x: i64, y: i64| -> Ordering {
        match collation.compare(strings[&x], strings[&y]) {
            Ok(Ordering::Equal) => x.cmp(&y),
            Ok(ord) => ord,
            Err(e) => {
                *failure.borrow_mut() = Some(e.to_string());
                Ordering::Equal
            }
        }
    };

    let mut ids: Vec<i64> = corpus.iter().map(|s| s.id).collect();
    ids.sort_by(|&x, &y| compare(x, y));
    if let Some(e) = failure.borrow().as_ref() {
        return Err(e.clone());
    }

    let equal: RefCell<Option<String>> = RefCell::new(None);
    let is_equal = |x: i64, y: i64| -> bool {
        match collation.compare(strings[&x], strings[&y]) {
            Ok(Ordering::Equal) => true,
            Ok(_) => false,
            Err(e) => {
                *equal.borrow_mut() = Some(e.to_string());
                false
            }
        }
    };

    let mut rank = BTreeMap::new();
    let mut current = 0_i64;
    for (pos, &id) in ids.iter().enumerate() {
        if pos == 0 || !is_equal(ids[pos - 1], id) {
            current += 1;
        }
        rank.insert(id, current);
    }
    if let Some(e) = equal.borrow().as_ref() {
        return Err(e.clone());
    }
    Ok((ids, rank))
}

fn run(corpus_path: &str, specs_path: &str, out_path: &str) -> Result<(), String> {
    let corpus: Vec<Scenario> = serde_json::from_str(
        &fs::read_to_string(corpus_path).map_err(|e| format!("{corpus_path}: {e}"))?,
    )
    .map_err(|e| format!("{corpus_path}: {e}"))?;
    let specs: Vec<Spec> = serde_json::from_str(
        &fs::read_to_string(specs_path).map_err(|e| format!("{specs_path}: {e}"))?,
    )
    .map_err(|e| format!("{specs_path}: {e}"))?;

    let mut out = Vec::with_capacity(specs.len());
    for spec in &specs {
        match build(spec) {
            Ok(collation) => match order_and_rank(&collation, &corpus) {
                Ok((order, rank)) => out.push(Out {
                    spec_id: spec.id.clone(),
                    order,
                    rank,
                    version: report_version(spec),
                    supported: true,
                    outcome: Outcome::Ok,
                    note: String::new(),
                }),
                // A comparison error after successful construction is a
                // backend failure, not a refusal: mark it `Error`.
                Err(e) => out.push(Out::error(spec, format!("compare failed: {e}"))),
            },
            // A malformed spec is a harness defect, not a collation refusal.
            Err(BuildError::Invalid(e)) => out.push(Out::error(spec, e)),
            Err(BuildError::Refused(e)) => out.push(Out::refused(spec, format!("refused: {e}"))),
        }
    }

    let json = serde_json::to_string(&out).map_err(|e| e.to_string())?;
    fs::write(out_path, json).map_err(|e| format!("{out_path}: {e}"))?;
    println!("wrote {} candidate results", out.len());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [program, corpus, specs, out] = args.as_slice() else {
        eprintln!("usage: {} <corpus.json> <specs.json> <out.json>", args[0]);
        return ExitCode::from(2);
    };
    match run(corpus, specs, out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{program}: error: {e}");
            ExitCode::FAILURE
        }
    }
}
