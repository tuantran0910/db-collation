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

//! `Oracle` backend.
//!
//! Oracle selects a collation by name through `NLS_SORT`. This crate reproduces
//! exactly two families:
//!
//! * `NLS_SORT=BINARY` on an `AL32UTF8` database with `VARCHAR2` (nonpadded)
//!   semantics — bytewise comparison, i.e. the [`Backend::Binary`] backend.
//! * The `UCA0700_DUCET` / `UCA1210_DUCET` collations — the Unicode Collation
//!   Algorithm with the open Unicode DUCET tables (Unicode 7.0.0 / 12.1.0),
//!   using Oracle's default parameters `_S4_VS_BN_NY_EN_FN_HN_DN_MN`
//!   (quaternary strength, shifted variable weighting, NFD normalization).
//!
//! Everything else is refused: Oracle's monolingual and multilingual (`_M`)
//! collations are defined by Oracle-proprietary locale data that is neither
//! documented nor redistributable, the `BINARY_CI`/`BINARY_AI` collations are
//! nondeterministic (no total order), the `*_ROOT`/`*_ORADUCET` collations
//! deviate from the open tables, and the tailored UCA collations are not
//! modelled.
//!
//! [`Backend::Binary`]: crate::spec::Backend::Binary

use crate::error::{Error, Result};
use crate::spec::{Backend, OracleCollation, OracleProvider};

/// Validate a resolved `Oracle` description, returning the concrete backend.
pub(crate) fn validate(spec: &OracleCollation) -> Result<Backend> {
    if !spec.charset().eq_ignore_ascii_case("AL32UTF8") {
        return Err(Error::unsupported(
            "oracle",
            spec.nls_sort(),
            "only the AL32UTF8 character set is reproducible",
        ));
    }
    if spec.blank_padded() {
        return Err(Error::unsupported(
            "oracle",
            spec.nls_sort(),
            "blank-padded CHAR/NCHAR comparison differs from VARCHAR2 and is not modelled",
        ));
    }
    match spec.provider() {
        OracleProvider::Binary => {
            // `NLS_SORT=BINARY` is bytewise on the storage bytes; for AL32UTF8
            // (valid UTF-8) that is exactly the binary backend.
            if spec.nls_sort().eq_ignore_ascii_case("BINARY") {
                Ok(Backend::Binary)
            } else {
                Err(Error::unsupported(
                    "oracle",
                    spec.nls_sort(),
                    "only NLS_SORT=BINARY is modelled for the binary provider",
                ))
            }
        }
        OracleProvider::Uca => {
            #[cfg(feature = "oracle-uca")]
            {
                let version = spec.uca_version().ok_or_else(|| {
                    Error::unsupported("oracle", spec.nls_sort(), "UCA collation has no version")
                })?;
                // Name/version consistency: the resolved name must be one of the
                // exactly-modelled `_DUCET` collations.
                if spec.nls_sort() != version.collation_name() {
                    return Err(Error::unsupported(
                        "oracle",
                        spec.nls_sort(),
                        "only UCA0700_DUCET and UCA1210_DUCET are reproducible",
                    ));
                }
                Ok(Backend::Oracle(spec.clone()))
            }
            #[cfg(not(feature = "oracle-uca"))]
            {
                let _ = spec;
                Err(Error::FeatureDisabled {
                    feature: "oracle-uca",
                })
            }
        }
    }
}

/// Compare two strings under a resolved `Oracle` UCA collation.
///
/// Only reached for `UCA*_DUCET` collations (`BINARY` is dispatched to the
/// binary backend).
#[cfg(feature = "oracle-uca")]
pub(crate) fn compare(spec: &OracleCollation, a: &str, b: &str) -> Result<core::cmp::Ordering> {
    match spec.provider() {
        OracleProvider::Uca => {
            let version = spec
                .uca_version()
                .ok_or_else(|| Error::unsupported("oracle", spec.nls_sort(), "no UCA version"))?;
            uca::compare(version, a, b).ok_or_else(|| {
                Error::input_limit(
                    "oracle",
                    "input exceeds Oracle's 2000-byte UCA sort-key limit; \
                     Oracle truncates the key there and is no longer pure DUCET",
                )
            })
        }
        OracleProvider::Binary => Err(Error::unsupported(
            "oracle",
            spec.nls_sort(),
            "binary provider is dispatched to the binary backend",
        )),
    }
}

#[cfg(feature = "oracle-uca")]
mod uca;

#[cfg(feature = "oracle-uca")]
mod table_uca0700;

#[cfg(feature = "oracle-uca")]
mod table_uca1210;
