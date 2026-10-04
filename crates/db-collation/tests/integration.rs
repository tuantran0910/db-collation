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

#[cfg(all(feature = "serde", feature = "oracle"))]
#[test]
fn test_oracle_collation_is_serializable() {
    use db_collation::OracleCollation;
    let c = Collation::oracle(OracleCollation::binary()).unwrap();
    let json = serde_json::to_string(&c).unwrap();
    let back: Collation = serde_json::from_str(&json).unwrap();
    assert_eq!(c, back);
}

#[cfg(feature = "serde")]
#[test]
fn test_deserialize_rejects_unsupported_oracle() {
    // A description naming an unsupported Oracle collation must not deserialize
    // into a usable `Collation`, even though the JSON is well-formed.
    let json = r#"{"Oracle":{"provider":"Uca","nls_sort":"UCA1210_ROOT","uca_version":"Uca1210","charset":"AL32UTF8","blank_padded":false}}"#;
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

#[cfg(feature = "oracle")]
mod oracle {
    use super::*;
    #[cfg(feature = "oracle-uca")]
    use db_collation::OracleUcaVersion;
    use db_collation::{Error, OracleCollation};

    #[test]
    fn test_oracle_binary_is_bytewise() {
        let c = Collation::oracle(OracleCollation::binary()).unwrap();
        assert!(matches!(c.backend(), Backend::Binary));
        assert_eq!(c.compare("A", "a").unwrap(), Ordering::Less);
        assert_eq!(c.compare("Z", "a").unwrap(), Ordering::Less);
        assert_eq!(c.compare("abc", "abd").unwrap(), Ordering::Less);
    }

    #[test]
    fn test_oracle_linguistic_names_are_refused() {
        // Monolingual, multilingual (`_M`), case/accent-insensitive binary, and
        // the deviating `*_ROOT`/`*_ORADUCET` UCA collations are all refused.
        for name in [
            "GERMAN",
            "XDANISH",
            "GENERIC_M",
            "FRENCH_M",
            "BINARY_CI",
            "BINARY_AI",
            "UCA1210_ROOT",
            "UCA1210_ORADUCET",
            "UCA1210_SPANISH",
            "UCA0700_ROOT",
        ] {
            let err =
                Collation::oracle(OracleCollation::from_nls_sort(name, "AL32UTF8")).unwrap_err();
            assert!(err.is_fallback_required(), "{name}");
            assert!(matches!(err, Error::Unsupported { .. }), "{name}");
        }
    }

    #[test]
    fn test_oracle_non_utf8_and_blank_padded_are_refused() {
        let bad = OracleCollation::from_nls_sort("BINARY", "WE8ISO8859P1");
        assert!(Collation::oracle(bad).is_err());
        let padded = OracleCollation::binary().with_blank_padded(true);
        assert!(Collation::oracle(padded).is_err());
    }

    #[test]
    #[cfg(feature = "oracle-uca")]
    fn test_oracle_uca_ducet_semantics() {
        let c = Collation::oracle(OracleCollation::uca_ducet(OracleUcaVersion::Uca1210)).unwrap();
        // Lowercase before uppercase, accent as a lower level.
        assert_eq!(c.compare("a", "A").unwrap(), Ordering::Less);
        assert_eq!(c.compare("a", "á").unwrap(), Ordering::Less);
        // The eszett expands to two "s" primaries and differs from "ss" only at
        // the secondary level.
        assert_eq!(c.compare("ss", "ß").unwrap(), Ordering::Less);
        assert_eq!(c.compare("strasse", "straße").unwrap(), Ordering::Less);
    }

    #[test]
    #[cfg(feature = "oracle-uca")]
    fn test_oracle_uca_long_input_is_refused() {
        // Oracle caps its UCA sort key at 2000 bytes; beyond that it truncates
        // the key and is no longer pure DUCET, so the crate must refuse the
        // input rather than return a wrong order. This is a per-input limit on a
        // *supported* configuration, so it is `InputLimit` (fall back for this
        // input), not `Unsupported` (the whole configuration is refused).
        let c = Collation::oracle(OracleCollation::uca_ducet(OracleUcaVersion::Uca1210)).unwrap();
        let long_a = format!("{}a", "p".repeat(400));
        let long_b = format!("{}b", "p".repeat(400));
        let err = c.compare(&long_a, &long_b).unwrap_err();
        assert!(err.is_fallback_required());
        assert!(
            matches!(err, Error::InputLimit { .. }),
            "over-length must be a typed input limit, not a construction refusal: {err:?}"
        );
        // Short inputs of the same shape are exact.
        assert_eq!(
            c.compare("pa", "pb").unwrap(),
            Ordering::Less,
            "short prefix must still compare"
        );
    }

    #[test]
    #[cfg(feature = "oracle-uca")]
    fn test_oracle_uca_nfd_matches_target_unicode_version() {
        // U+00E5 (å) canonical-decomposes to U+0061 U+030A, and the DUCET is
        // canonically closed, so the precomposed and decomposed forms compare
        // equal. This exercises the pinned NFD.
        let c = Collation::oracle(OracleCollation::uca_ducet(OracleUcaVersion::Uca1210)).unwrap();
        assert!(c.equal("\u{e5}", "a\u{030a}").unwrap());
        // A combining sequence is canonically reordered: U+0301 has class 230,
        // U+0316 has class 220, so U+0316 must be ordered first.
        assert!(c.equal("a\u{0316}\u{0301}", "a\u{0301}\u{0316}").unwrap());
        // Normalization is pinned to Oracle's fixed Unicode version (15.0) for
        // **both** UCA weight versions, not to the UCA version. U+11938 gained a
        // decomposition in Unicode 13.0, so it decomposes here; a code point
        // added later than 15.0 (U+105C9, Unicode 16) with a canonical
        // decomposition must stay atomic and sort by its own implicit weight.
        for version in [OracleUcaVersion::Uca1210, OracleUcaVersion::Uca700] {
            let c = Collation::oracle(OracleCollation::uca_ducet(version)).unwrap();
            assert!(
                c.equal("\u{11938}", "\u{11935}\u{11930}").unwrap(),
                "{version:?}: Unicode 13 decomposition must apply"
            );
            assert_eq!(
                c.compare("\u{105c9}", "\u{105d0}").unwrap(),
                Ordering::Less,
                "{version:?}: post-15.0 code point must not be decomposed"
            );
            // U+07FD (combining class 220, Unicode 11.0) must be canonically
            // reordered even under the 7.0 weight table: normalization does not
            // follow the UCA version.
            assert!(
                c.equal("\u{0334}\u{07fd}", "\u{07fd}\u{0334}").unwrap(),
                "{version:?}: normalization must not be pinned to the UCA version"
            );
        }
    }

    #[test]
    #[cfg(feature = "oracle-uca")]
    fn test_oracle_uca_han_implicit_range_boundaries() {
        // Oracle assigns Han implicit weights to the Extension G/H intervals
        // U+30000..U+3134A and U+31350..U+323AF (verified against live Oracle),
        // for both named UCA versions. The gap U+3134B..U+3134F and U+323B0
        // onward are generic. Han primaries use `FB80 +`, which places them
        // *before* the generic U+0378 (`FBC0 +`) even though their code points
        // are larger.
        for version in [OracleUcaVersion::Uca1210, OracleUcaVersion::Uca700] {
            let c = Collation::oracle(OracleCollation::uca_ducet(version)).unwrap();
            for cp in [0x30000, 0x3134A, 0x31350, 0x323AF] {
                assert_eq!(
                    c.compare(&char::from_u32(cp).unwrap().to_string(), "\u{378}")
                        .unwrap(),
                    Ordering::Less,
                    "{version:?}: U+{cp:05X} is Han and must sort before U+0378"
                );
            }
            for cp in [0x3134B, 0x3134F, 0x323B0] {
                assert_eq!(
                    c.compare(&char::from_u32(cp).unwrap().to_string(), "\u{378}")
                        .unwrap(),
                    Ordering::Greater,
                    "{version:?}: U+{cp:05X} is in the generic gap and must sort after U+0378"
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "oracle-uca")]
    fn test_oracle_uca_consumed_mark_does_not_match_again() {
        // A mark consumed by a discontiguous contraction must not participate in
        // a later match: it is removed from the stream, so the remaining marks
        // become logically adjacent. Here the first Tibetan U+0F71 skips the
        // second U+0F71 and takes the U+0F72 (ccc 130 > 129) as a discontiguous
        // contraction; the second U+0F71 must then stand alone. Verified against
        // live Oracle (source says Less for the pair below).
        let left = "\u{0f71}\u{0f71}\u{0f72}";
        let right = "\u{0f71}\u{0f72}\u{0f72}";
        for version in [OracleUcaVersion::Uca1210, OracleUcaVersion::Uca700] {
            let c = Collation::oracle(OracleCollation::uca_ducet(version)).unwrap();
            assert_eq!(
                c.compare(left, right).unwrap(),
                Ordering::Less,
                "{version:?}: a consumed mark must not match again"
            );
            // A longer witness from the overlap corpus.
            assert_eq!(
                c.compare(
                    "\u{0fb2}\u{0fb2}\u{0f72}\u{0f71}\u{0f71}",
                    "\u{0fb2}\u{0fb2}\u{0f71}\u{0f72}\u{0f72}"
                )
                .unwrap(),
                Ordering::Less,
                "{version:?}: repeated non-starter-leading contractions"
            );
        }
    }

    #[test]
    fn test_oracle_is_send_sync() {
        static_assertions::assert_impl_all!(Collation: Send, Sync);
    }

    #[test]
    #[cfg(feature = "oracle-uca")]
    fn test_oracle_uca_discontiguous_contraction_matches_across_lower_class_mark() {
        // UTS #10: a contraction may match across intervening non-starters whose
        // combining class is lower than the mark that completes it. Here the
        // Cyrillic `и` + breve contraction (`0438 0306`) must be found even with
        // U+0591 (class 220) between them, because U+0306 has class 230.
        for version in [OracleUcaVersion::Uca1210, OracleUcaVersion::Uca700] {
            let c = Collation::oracle(OracleCollation::uca_ducet(version)).unwrap();
            assert!(
                c.equal("\u{0439}a", "\u{0438}\u{0591}\u{0306}a").unwrap(),
                "{version:?}: discontiguous Cyrillic contraction"
            );
            assert!(
                c.equal("\u{0419}a", "\u{0418}\u{0591}\u{0306}a").unwrap(),
                "{version:?}: discontiguous Cyrillic contraction (uppercase)"
            );
            // Arabic madda: `0627 0653` contraction across a skipped mark.
            assert!(
                c.equal("\u{0622}a", "\u{0627}\u{0591}\u{0653}a").unwrap(),
                "{version:?}: discontiguous Arabic contraction"
            );
        }
    }

    #[test]
    #[cfg(feature = "oracle-uca")]
    fn test_oracle_uca_discontiguous_contraction_respects_blocking() {
        // A mark whose combining class is >= the target's blocks the match.
        let c = Collation::oracle(OracleCollation::uca_ducet(OracleUcaVersion::Uca1210)).unwrap();
        // A starter in between always blocks: U+0061 is a starter.
        assert!(
            !c.equal("\u{0439}", "\u{0438}\u{0061}\u{0306}").unwrap(),
            "a starter must block discontiguous matching"
        );
        // A higher-class mark blocks the lower-class target (U+0308 has class 230).
        assert!(
            !c.equal("\u{0439}", "\u{0438}\u{0308}\u{0306}").unwrap(),
            "a higher-class intervening mark must block"
        );
        // COMBINING GRAPHEME JOINER (U+034F) is a starter-like blocker even
        // though it is otherwise completely ignorable.
        assert!(
            !c.equal("\u{0439}", "\u{0438}\u{034f}\u{0306}").unwrap(),
            "CGJ must block discontiguous matching"
        );
    }
}
