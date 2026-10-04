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

//! `PostgreSQL` ICU backend, implemented over ICU4C (`rust_icu_ucol`).
//!
//! Mirrors `PostgreSQL`'s `varstr_cmp` (`src/backend/utils/adt/varlena.c`):
//! `ucol_strcollUTF8` on the UTF-8 bytes, then, for deterministic collations,
//! a bytewise tie-break with the shorter string first.

use core::cmp::Ordering;
use std::ffi::CString;

use rust_icu_sys as sys;
use rust_icu_sys::versioned_function;
use rust_icu_ucol::UCollator;

use crate::error::{Error, Result};
use crate::spec::PostgresCollation;
use crate::thread_local;

/// The local ICU's collation data version for `locale`, formatted exactly as
/// `PostgreSQL` writes `collversion` (e.g. `153.128` or, for a tailored
/// locale, `153.128.46`). Returns `None` if ICU cannot open the locale.
///
/// This mirrors `get_collation_actual_version()` in `PostgreSQL`'s
/// `pg_locale.c` exactly: open the collator, fill the four version bytes with
/// `ucol_getVersion`, then format them with ICU's own `u_versionToString`
/// (which drops trailing zero components). Hand-formatting the bytes would
/// diverge from `PostgreSQL` — e.g. a naive `major.minor.uca` prints
/// `153.128.0` where `PostgreSQL` prints `153.128`.
///
/// `rust_icu_ucol` does not expose `ucol_getVersion`/`u_versionToString`, so
/// this is the one place the crate calls ICU4C directly. The FFI is small,
/// self-contained, and every pointer is derived from a live ICU handle closed
/// before return.
#[allow(unsafe_code)]
fn local_collversion(locale: &str) -> Option<String> {
    let c_locale = CString::new(locale).ok()?;
    let mut status: sys::UErrorCode = sys::UErrorCode::U_ZERO_ERROR;
    // Safety: `ucol_open` is given a valid NUL-terminated locale and a status
    // pointer, per `ucol.h`. The returned handle owns ICU resources and is
    // closed before this function returns.
    let coll = unsafe { versioned_function!(ucol_open)(c_locale.as_ptr(), &raw mut status) };
    if status > sys::UErrorCode::U_ZERO_ERROR || coll.is_null() {
        return None;
    }

    let mut info = [0u8; 4];
    // `U_MAX_VERSION_STRING_LENGTH` is 20; `u_versionToString` NUL-terminates.
    let mut buf: [core::ffi::c_char; 20] = [0; 20];
    // Safety: `coll` is a live, open collator; `info` is exactly the 4-byte
    // buffer `ucol_getVersion` writes, and `buf` is the documented size for
    // `u_versionToString`.
    unsafe {
        versioned_function!(ucol_getVersion)(coll, info.as_mut_ptr());
        versioned_function!(ucol_close)(coll);
        versioned_function!(u_versionToString)(info.as_mut_ptr(), buf.as_mut_ptr());
    }

    let len = buf.iter().position(|&c| c == 0)?;
    // The string is ASCII digits and dots, so each `c_char` is a positive byte.
    let bytes = buf[..len]
        .iter()
        .map(|&c| u8::try_from(c).ok())
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

/// The local ICU's own library version (e.g. `76.1`), for diagnostics.
#[must_use]
#[allow(unsafe_code)]
pub fn icu_library_version() -> String {
    let mut bytes = [0u8; 4];
    // Safety: `u_getVersion` writes exactly 4 bytes into the provided buffer.
    unsafe { versioned_function!(u_getVersion)(bytes.as_mut_ptr()) };
    let [major, minor, _, _] = bytes;
    format!("{major}.{minor}")
}

/// The collation data version this build would use for `locale`, formatted
/// exactly as `PostgreSQL` reports it in `pg_collation_actual_version(oid)`.
///
/// Returns `None` if ICU cannot open `locale`. This is the value a caller
/// compares against the source's `collversion`; it is exposed so the
/// differential harness can assert the library reproduces the server's version
/// string without reimplementing the formatting.
#[must_use]
pub fn collation_version(locale: &str) -> Option<String> {
    local_collversion(locale)
}

/// Validate a `PostgreSQL` ICU description before accepting it.
///
/// This enforces the version gate: the caller-supplied `collversion` must match
/// the collation data version that this build of ICU would use. On mismatch the
/// caller must fall back to comparing on the source database.
pub(crate) fn validate(spec: &PostgresCollation) -> Result<()> {
    let locale = spec.locale();
    if locale.is_empty() {
        return Err(Error::unsupported(
            "postgres",
            locale,
            "ICU collation requires a non-empty locale id",
        ));
    }
    // Opening the collator both validates the locale and proves ICU is usable.
    UCollator::try_from(locale).map_err(|_| {
        Error::unsupported(
            "postgres",
            locale,
            "ICU could not open this locale on this system",
        )
    })?;

    let Some(local) = local_collversion(locale) else {
        return Err(Error::unsupported(
            "postgres",
            locale,
            "ICU could not report a collation version for this locale",
        ));
    };

    // The version is part of the collation's identity. A missing or sentinel
    // value cannot be checked, so it is refused rather than silently accepted.
    let source = spec.version().as_str();
    if source.is_empty() || source == "unknown" {
        return Err(Error::unsupported(
            "postgres",
            locale,
            "an exact ICU collation version is required; empty or unknown is not accepted",
        ));
    }
    if !version_matches(source, &local) {
        return Err(Error::VersionMismatch {
            source: spec.version().clone(),
            local: crate::spec::VersionId::new(local),
        });
    }
    Ok(())
}

/// Compare the caller's `collversion` with the local one.
///
/// ICU reports the version as up to four bytes; `PostgreSQL` formats the full
/// value with `u_versionToString`. The two are the same identity if they agree
/// after normalizing only trailing *zero* components: `153.128` and
/// `153.128.0` are the same data version, but a nonzero component
/// (`…​.46` vs `…​.999`) is a different identity and must not be discarded.
fn version_matches(source: &str, local: &str) -> bool {
    fn components(v: &str) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for part in v.split('.') {
            out.push(part.parse::<u8>().ok()?);
        }
        if out.is_empty() || out.len() > 4 {
            return None;
        }
        while out.len() > 1 && out.last() == Some(&0) {
            out.pop();
        }
        Some(out)
    }
    match (components(source), components(local)) {
        (Some(a), Some(b)) => a == b,
        _ => source == local,
    }
}

/// Compare two strings exactly as `PostgreSQL` would for this ICU collation.
///
/// Returns [`Error`] if the underlying ICU comparison fails, so a failed
/// comparison is never silently reported as equality.
pub(crate) fn compare(spec: &PostgresCollation, a: &str, b: &str) -> Result<Ordering> {
    // ICU collators are `!Send + !Sync`; cache one per thread. `None` means ICU
    // could not open the locale on this thread, which we surface as an error.
    let unsupported = || {
        Error::unsupported(
            "postgres",
            spec.locale(),
            "ICU string comparison failed at runtime",
        )
    };
    let ord = thread_local::with(spec.locale(), |collator| collator.strcoll_utf8(a, b))
        .ok_or_else(unsupported)?
        .map_err(|_| unsupported())?;

    if ord == Ordering::Equal && spec.deterministic() {
        Ok(a.as_bytes().cmp(b.as_bytes()))
    } else {
        Ok(ord)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::VersionId;

    #[test]
    fn test_empty_locale_is_refused() {
        let spec = PostgresCollation::icu(String::new(), true, VersionId::new("x"));
        assert!(validate(&spec).is_err());
    }

    #[test]
    fn test_version_matches_normalizes_only_trailing_zeros() {
        // Trailing-zero spellings are the same identity.
        assert!(version_matches("153.128", "153.128.0"));
        assert!(version_matches("153.128", "153.128"));
        assert!(version_matches("153.14", "153.14.0.0"));
        // A nonzero component is a different identity and must not be dropped.
        assert!(!version_matches("153.128", "153.14"));
        assert!(!version_matches("153.128", "152.1"));
        assert!(!version_matches("153.136.999", "153.136.48"));
        assert!(!version_matches("153.136.48", "153.136.999"));
    }

    #[test]
    fn test_version_gate_refuses_nonzero_tail_mismatch() {
        // Local `und` is 153.<UCA>; 153.136.999 must not be accepted by matching
        // only the leading major.minor.
        let spec = PostgresCollation::icu("und", true, VersionId::new("153.136.999"));
        assert!(validate(&spec).is_err());
    }

    #[test]
    fn test_version_gate_refuses_empty_and_unknown() {
        for v in ["", "unknown"] {
            let spec = PostgresCollation::icu("und", true, VersionId::new(v));
            let err = validate(&spec).unwrap_err();
            assert!(err.is_fallback_required(), "version {v:?} must be refused");
        }
    }

    #[test]
    fn test_local_collversion_reproduces_postgres_format() {
        // PostgreSQL's `collversion` for the default locale is exactly the two
        // components `153.<UCA>`: `u_versionToString` trims the trailing zero
        // components. A hand-rolled `major.minor.uca` would emit `153.128.0`.
        let v = local_collversion("und").expect("und collation version");
        let parts: Vec<&str> = v.split('.').collect();
        assert_eq!(parts.len(), 2, "unexpected version shape: {v}");
        assert_eq!(parts[0], "153", "unexpected UCOL major: {v}");
        assert!(
            parts[1].parse::<u32>().is_ok(),
            "non-numeric UCA version: {v}"
        );
    }

    #[test]
    fn test_tailored_locale_adds_component() {
        // A tailored locale reports at least one component beyond the base
        // `major.uca` pair — PostgreSQL's `153.128.46`-style values. The exact
        // count depends on the ICU data version (newer UCA revisions add a
        // fourth), so assert "strictly more than the base", not an exact 3.
        if let Some(base) = local_collversion("und") {
            let base_parts = base.split('.').count();
            if let Some(v) = local_collversion("de-u-co-phonebk") {
                assert!(
                    v.split('.').count() > base_parts,
                    "tailored version {v} should have more components than base {base}"
                );
            }
        }
    }

    #[test]
    fn test_version_gate_refuses_mismatch() {
        let spec = PostgresCollation::icu("und", true, VersionId::new("1.2"));
        match validate(&spec) {
            Err(Error::VersionMismatch { source, local }) => {
                assert_eq!(source.as_str(), "1.2");
                assert_ne!(local.as_str(), "");
            }
            other => panic!("expected VersionMismatch, got {other:?}"),
        }
    }
}
