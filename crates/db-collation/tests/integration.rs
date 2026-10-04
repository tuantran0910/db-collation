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

//! Integration tests for the public API.

use std::cmp::Ordering;

use db_collation::{Backend, Collation};
#[cfg(feature = "mysql-uca")]
use db_collation::{Error, MysqlCollation, MysqlUcaVersion, Pad};

#[test]
fn test_binary_matches_bytes() {
    let c = Collation::binary();
    assert_eq!(c.compare("A", "a").unwrap(), Ordering::Less); // 0x41 < 0x61
    assert_eq!(c.compare("abc", "abd").unwrap(), Ordering::Less);
    assert!(matches!(c.backend(), Backend::Binary));
}

#[test]
#[cfg(feature = "mysql-uca")]
fn test_mysql_0900_ai_ci_basic_semantics() {
    let c = Collation::mysql_0900_ai_ci();
    assert!(c.equal("a", "A").unwrap());
    assert!(c.equal("e", "\u{e9}").unwrap());
    assert!(!c.equal("a", "a ").unwrap()); // NO PAD
    assert!(c.equal("\u{df}", "ss").unwrap());
}

#[test]
#[cfg(feature = "mysql-uca")]
fn test_mysql_unicode_ci_pad_space() {
    let c = Collation::mysql_unicode_ci();
    assert!(c.equal("a", "a ").unwrap()); // PAD SPACE
    assert!(c.equal("a ", "a  ").unwrap());
    assert!(c.equal("e", "\u{e9}").unwrap());
}

#[test]
#[cfg(feature = "mysql-uca")]
fn test_refuses_unsupported_mysql_collations() {
    for name in [
        "utf8mb4_general_ci",
        "utf8mb4_unicode_520_ci",
        "utf8mb4_0900_as_cs",
        "utf8mb4_zh_0900_as_cs",
    ] {
        let err = Collation::from_mysql_name_unspecified_version(name).unwrap_err();
        assert!(err.is_fallback_required(), "{name}");
        assert!(matches!(err, Error::Unsupported { .. }));
    }
}

#[test]
#[cfg(feature = "mysql-uca")]
fn test_inconsistent_mysql_spec_is_refused() {
    let bad = MysqlCollation::new("utf8mb4_0900_ai_ci", MysqlUcaVersion::Uca400, Pad::Space);
    assert!(Collation::mysql(bad).is_err());
}

#[test]
fn test_postgres_libc_is_refused() {
    use db_collation::PostgresCollation;
    let spec = PostgresCollation::libc("en_US.UTF-8");
    let err = Collation::postgres(spec).unwrap_err();
    assert!(err.is_fallback_required());
}

#[test]
fn test_postgres_c_is_binary() {
    let c = Collation::postgres_c();
    assert!(matches!(c.backend(), Backend::Binary));
}

#[test]
#[cfg(feature = "mysql-uca")]
fn test_error_display_mentions_fallback() {
    let e = Collation::from_mysql_name_unspecified_version("utf8mb4_general_ci").unwrap_err();
    assert!(e.to_string().contains("compare on the source"));
}

#[test]
#[cfg(feature = "mysql-uca")]
fn test_collation_is_thread_safe() {
    use std::sync::Arc;

    static_assertions::assert_impl_all!(Collation: Send, Sync);

    let c = Arc::new(Collation::mysql_0900_ai_ci());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let c = Arc::clone(&c);
            std::thread::spawn(move || {
                for _ in 0..1000 {
                    assert!(c.equal("abc", "ABC").unwrap());
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[cfg(all(feature = "serde", feature = "mysql-uca"))]
#[test]
fn test_collation_is_serializable() {
    let c = Collation::mysql_0900_ai_ci();
    let json = serde_json::to_string(&c).unwrap();
    let back: Collation = serde_json::from_str(&json).unwrap();
    assert_eq!(c, back);
}

#[test]
fn test_postgres_custom_rules_are_refused() {
    use db_collation::{PostgresCollation, VersionId};
    let spec =
        PostgresCollation::icu("und", false, VersionId::new("153.128")).with_rules("& z < a");
    let err = Collation::postgres(spec).unwrap_err();
    assert!(err.is_fallback_required());
}

#[test]
fn test_postgres_non_utf8_is_refused() {
    use db_collation::{PostgresCollation, VersionId};
    let spec =
        PostgresCollation::icu("und", true, VersionId::new("153.128")).with_encoding("LATIN1");
    let err = Collation::postgres(spec).unwrap_err();
    assert!(err.is_fallback_required());
}

#[cfg(feature = "serde")]
#[test]
fn test_deserialize_rejects_custom_rules() {
    let json = r#"{"PostgresIcu":{"provider":"Icu","locale":"und","deterministic":false,"version":"153.128","rules":"& z < a","encoding":"UTF8"}}"#;
    assert!(serde_json::from_str::<Collation>(json).is_err());
}

#[cfg(feature = "serde")]
#[test]
fn test_deserialize_rejects_unsupported_mysql() {
    // A description naming an unsupported collation must not deserialize into a
    // usable `Collation`, even though the JSON is well-formed.
    let json = r#"{"MysqlUca":{"name":"utf8mb4_general_ci","uca_version":"Uca900","pad":"NoPad"}}"#;
    assert!(serde_json::from_str::<Collation>(json).is_err());
}

#[cfg(all(feature = "serde", feature = "postgres-icu"))]
#[test]
fn test_deserialize_rejects_postgres_empty_version() {
    // A complete, otherwise-valid payload: every required field is present, so
    // the failure must come from version validation (empty version), not from a
    // missing-field parse error.
    let json = r#"{"PostgresIcu":{"provider":"Icu","locale":"und","deterministic":true,"version":"","rules":null,"encoding":"UTF8"}}"#;
    let err = serde_json::from_str::<Collation>(json).unwrap_err();
    assert!(
        err.to_string().contains("version") || err.to_string().contains("unsupported"),
        "expected a version-validation error, got: {err}"
    );
}

#[cfg(all(feature = "serde", not(feature = "mysql-uca")))]
#[test]
fn test_deserialize_rejects_backend_when_disabled() {
    // In a serde-only build (no `mysql-uca`), a MySQL description must not
    // deserialize into a usable `Collation`.
    let json = r#"{"MysqlUca":{"name":"utf8mb4_0900_ai_ci","uca_version":"Uca900","pad":"NoPad"}}"#;
    assert!(serde_json::from_str::<Collation>(json).is_err());
}

#[cfg(feature = "postgres-icu")]
mod postgres {
    use std::cmp::Ordering;
    use std::sync::Arc;

    use db_collation::{Collation, Error, VersionId};

    fn local_version(locale: &str) -> VersionId {
        VersionId::new(
            db_collation::collation_version(locale)
                .unwrap_or_else(|| panic!("no local collation version for {locale}")),
        )
    }

    #[test]
    fn test_deterministic_ties_are_broken_bytewise() {
        // At primary strength `und-u-ks-level1`, ICU considers "A" and "a" equal;
        // a deterministic collation then breaks the tie bytewise.
        let c = Collation::postgres_icu("und-u-ks-level1", true, local_version("und-u-ks-level1"))
            .unwrap();
        // IU-equal, byte-distinct: "A" (0x41) < "a" (0x61).
        assert_eq!(c.compare("A", "a").unwrap(), Ordering::Less);
        assert_eq!(c.compare("a", "A").unwrap(), Ordering::Greater);
        // Bytewise: lowercase ASCII (0x61..) sorts after uppercase (0x41..).
        assert_eq!(c.compare("abc", "ABC").unwrap(), Ordering::Greater);
        // Prefix tie-break: "a" is a prefix of "aa", shorter first.
        assert_eq!(c.compare("a", "aa").unwrap(), Ordering::Less);
    }

    #[test]
    fn test_nondeterministic_equality_is_icu_equality() {
        let c = Collation::postgres_icu("und-u-ks-level1", false, local_version("und-u-ks-level1"))
            .unwrap();
        assert_eq!(c.compare("A", "a").unwrap(), Ordering::Equal);
        assert_eq!(c.compare("abc", "ABC").unwrap(), Ordering::Equal);
    }

    #[test]
    fn test_source_version_mismatch_is_refused() {
        let err = Collation::postgres_icu("und", true, VersionId::new("1.2")).unwrap_err();
        assert!(matches!(err, Error::VersionMismatch { .. }));
        assert!(err.is_fallback_required());
    }

    #[test]
    fn test_source_version_unknown_is_refused() {
        for v in ["", "unknown"] {
            let err = Collation::postgres_icu("und", true, VersionId::new(v)).unwrap_err();
            assert!(err.is_fallback_required(), "version {v:?}");
        }
    }

    #[test]
    fn test_postgres_is_thread_safe_in_use() {
        // The per-thread ICU collator cache must let many threads compare at once.
        let c = Arc::new(Collation::postgres_icu("und", true, local_version("und")).unwrap());
        let handles: Vec<_> = (0..8)
            .map(|k| {
                let c = Arc::clone(&c);
                std::thread::spawn(move || {
                    for i in 0..500u32 {
                        let a = format!("{k}-{i:04}");
                        let b = format!("{k}-{:04}", i + 1);
                        assert_eq!(c.compare(&a, &b).unwrap(), Ordering::Less);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }
}
