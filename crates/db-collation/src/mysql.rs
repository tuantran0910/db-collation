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

//! `MySQL` UCA weight-table backend.
//!
//! The weight tables in `src/mysql/table_*.rs` are generated from the Unicode
//! Collation Algorithm `allkeys` data by `harness/gen_weights.py`. The
//! comparison semantics reproduce `MySQL`:
//!
//! * NO PAD (`utf8mb4_0900_*`): compare the concatenated per-character weights.
//! * PAD SPACE (`utf8mb4_unicode_ci`): drop zero weights, then pad the weight
//!   sequence with the space weight (`0x0209`) to the longer length.
//! * Default-ignorable code points get zero weight rather than an implicit
//!   weight (`MySQL` excludes them from implicit-weight construction).
//!
//! Each rule was established against live `MySQL` with the differential harness.

use core::cmp::Ordering;

use crate::error::{Error, Result};
use crate::spec::{MysqlCollation, MysqlUcaVersion, Pad};

mod table_0400;
mod table_0900;

/// Space weight, used to pad PAD SPACE comparisons.
const SPACE: u16 = 0x0209;
/// The replacement weight, as a 16-bit value.
const LONG_RUNE_U16: u16 = 0xFFFD;

/// The Unicode 9.0.0 `Default_Ignorable_Code_Point` set.
///
/// `MySQL` treats these as entirely ignorable (zero weight); the implicit-weight
/// formula must not be applied to them. Note `0x180F`, `0x2065`, and
/// `0xFFF0..0xFFF8` are NOT ignorable here and receive implicit weights (later
/// Unicode versions added them). The generated weight tables already encode
/// every code point they cover — including all default-ignorables, which
/// `allkeys` lists with zero primaries — so this set is only consulted for code
/// points *beyond* the table extent (supplementary ranges, e.g. the tag
/// characters `0xE0020..0xE007F`). Verified against `MySQL`'s weight output.
const DICP_RANGES: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x034F, 0x034F),
    (0x061C, 0x061C),
    (0x115F, 0x1160),
    (0x17B4, 0x17B5),
    (0x180B, 0x180E),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0x3164, 0x3164),
    (0xFE00, 0xFE0F),
    (0xFEFF, 0xFEFF),
    (0xFFA0, 0xFFA0),
    (0xFFF9, 0xFFFB),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
    (0xE0100, 0xE01EF),
];

fn is_default_ignorable(cp: u32) -> bool {
    DICP_RANGES.iter().any(|&(lo, hi)| (lo..=hi).contains(&cp))
}

/// The known-good collation names, each mapped to its exact semantics.
pub(crate) fn validate(spec: &MysqlCollation) -> Result<()> {
    let expected = match spec.name.as_str() {
        "utf8mb4_0900_ai_ci" => Some((MysqlUcaVersion::Uca900, Pad::NoPad)),
        "utf8mb4_unicode_ci" => Some((MysqlUcaVersion::Uca400, Pad::Space)),
        _ => None,
    };
    match expected {
        Some((uca, pad)) if uca == spec.uca_version && pad == spec.pad => Ok(()),
        Some(_) => Err(Error::unsupported(
            "mysql",
            &spec.name,
            "collation name/version/pad combination is inconsistent",
        )),
        None => Err(Error::unsupported(
            "mysql",
            &spec.name,
            "not one of the exactly reproducible utf8mb4 UCA collations",
        )),
    }
}

/// The maximum number of 16-bit elements a single character can expand to.
const MAX_WEIGHTS: usize = 8;

/// A fixed-capacity sink for a single character's weight elements.
///
/// This is the allocation-free core of the streaming comparator: every
/// character's weights are emitted here directly, so comparing strings never
/// allocates. The buffer holds `MAX_WEIGHTS` elements (the largest expansion any
/// supported table produces), and any write beyond that is dropped, mirroring
/// the truncation the packed tables themselves apply.
struct WeightBuf {
    buf: [u16; MAX_WEIGHTS],
    len: usize,
}

impl WeightBuf {
    const fn new() -> Self {
        Self {
            buf: [0; MAX_WEIGHTS],
            len: 0,
        }
    }

    /// Append a single element, ignoring it if the buffer is full.
    const fn push(&mut self, w: u16) {
        if self.len < MAX_WEIGHTS {
            self.buf[self.len] = w;
            self.len += 1;
        }
    }

    #[cfg(test)]
    fn as_slice(&self) -> &[u16] {
        &self.buf[..self.len]
    }
}

/// Emit the weight elements of `ch` into `out`, in `MySQL`'s element order.
fn push_weight(collation: &MysqlCollation, ch: char, out: &mut WeightBuf) {
    let cp = ch as u32;
    match collation.uca_version {
        MysqlUcaVersion::Uca900 => {
            if let Some(u) = table_0900::weight(cp) {
                push_u64(u, out);
            } else if is_default_ignorable(cp) {
                // zero weight
            } else {
                push_implicit(cp, out);
            }
        }
        MysqlUcaVersion::Uca400 => {
            if cp > 0xFFFF {
                // All supplementary characters compare equal to U+FFFD.
                out.push(LONG_RUNE_U16);
            } else if let Some(u) = table_0400::weight(cp) {
                // The BMP table is dense, so this always matches: every code
                // point up to U+FFFF carries a real, implicit, or zero weight.
                push_u64(u, out);
            } else {
                // Unreachable for a well-formed table; fall back to the
                // supplementary value rather than silently ignoring the char.
                out.push(LONG_RUNE_U16);
            }
        }
    }
}

/// Emit a table entry (one or more 16-bit weights) in `MySQL`'s order.
///
/// The generator packs a multi-element weight with the *first* collation element
/// in the least significant 16-bit chunk and the last element highest, so we
/// emit the chunks low-to-high. `MySQL`'s `WEIGHT_STRING` concatenates elements
/// first-to-last (verified: `WEIGHT_STRING('½') = 1C3E06261C3F` ⇒
/// `[1C3E, 0626, 1C3F]`). `u128` accommodates multi-element expansions.
const fn push_u64(mut u: u128, out: &mut WeightBuf) {
    while u != 0 {
        out.push((u & 0xFFFF) as u16);
        u >>= 16;
    }
}

/// The leading primary page for an implicit code point, mirroring `MySQL`'s
/// `uca_scanner_900::next_implicit` (root collation; no zh reordering).
///
/// The weight table already encodes these pages for every code point it covers
/// (the generator inlines them); this is only reached *outside* the table
/// (beyond U+2CEA0), so it must reproduce the same ranges.
fn implicit_page(cp: u32) -> u32 {
    if (0x17000..=0x18AFF).contains(&cp) {
        // Tangut.
        0xFB00
    } else if (0x3400..=0x4DB5).contains(&cp)
        || (0x20000..=0x2A6D6).contains(&cp)
        || (0x2A700..=0x2B734).contains(&cp)
        || (0x2B740..=0x2B81D).contains(&cp)
        || (0x2B820..=0x2CEA1).contains(&cp)
    {
        // CJK Unified Ideographs Extension ranges.
        (cp >> 15) + 0xFB80
    } else if (0x4E00..=0x9FD5).contains(&cp) || (0xFA0E..=0xFA29).contains(&cp) {
        // CJK Unified Ideographs (and the compatibility block subset).
        (cp >> 15) + 0xFB40
    } else {
        // Generic implicit.
        (cp >> 15) + 0xFBC0
    }
}

/// Hangul syllable decomposition into conjoining jamo, per `MySQL`'s
/// `my_decompose_hangul_syllable`. Returns `None` if `cp` is not a Hangul
/// syllable.
fn decompose_hangul(cp: u32) -> Option<[u32; 3]> {
    const SYLLABLE_BASE: u32 = 0xAC00;
    const LEADING_BASE: u32 = 0x1100;
    const VOWEL_BASE: u32 = 0x1161;
    const TRAILING_BASE: u32 = 0x11A7;
    const VOWEL_CNT: u32 = 21;
    const TRAILING_CNT: u32 = 28;

    if !(SYLLABLE_BASE..=0xD7AF).contains(&cp) {
        return None;
    }
    let index = cp - SYLLABLE_BASE;
    let v_t = VOWEL_CNT * TRAILING_CNT;
    let leading = LEADING_BASE + index / v_t;
    let vowel = VOWEL_BASE + (index % v_t) / TRAILING_CNT;
    let trailing_index = index % TRAILING_CNT;
    let trailing = if trailing_index == 0 {
        0
    } else {
        TRAILING_BASE + trailing_index
    };
    Some([leading, vowel, trailing])
}

/// Push the implicit weights for a code point not covered by the table.
///
/// Mirrors `MySQL`'s `uca_scanner_900::next_implicit`: Hangul syllables decompose
/// to jamo (whose own table weights are used); everything else gets the primary
/// pair `[page, low]`.
///
/// `MySQL`'s full implicit collation element carries fixed secondary/tertiary
/// components `0x0020 0x0002`, but the copied weight table stores every CJK
/// character's *flattened primary* pair (verified:
/// `WEIGHT_STRING(U+2CEA0)=FB85CEA0`, `U+2CEA1=FB85CEA1`). Emitting the
/// secondary/tertiary here would insert extra elements and desynchronize the
/// primary sequence, so only the primary pair is emitted, matching the table.
fn push_implicit(cp: u32, out: &mut WeightBuf) {
    if let Some([l, v, t]) = decompose_hangul(cp) {
        for jamo in [l, v, t] {
            if jamo == 0 {
                continue;
            }
            if let Some(u) = table_0900::weight(jamo) {
                push_u64(u, out);
            }
        }
        return;
    }

    let tangut = (0x17000..=0x18AFF).contains(&cp);
    let page = if tangut { 0xFB00 } else { implicit_page(cp) };
    let low = if tangut {
        (cp - 0x17000) | 0x8000
    } else {
        (cp & 0x7FFF) | 0x8000
    };
    out.push(u16::try_from(page).unwrap_or(u16::MAX));
    out.push(u16::try_from(low).unwrap_or(u16::MAX));
}

#[cfg(test)]
fn weights(collation: &MysqlCollation, s: &str) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    for ch in s.chars() {
        let mut buf = WeightBuf::new();
        push_weight(collation, ch, &mut buf);
        out.extend_from_slice(buf.as_slice());
    }
    out
}

/// Streams the 16-bit weight elements of a string without allocating.
///
/// Each `char` yields up to eight elements (the maximum expansion); the buffer
/// is reused per character. PAD SPACE is reproduced by padding the shorter
/// stream with `SPACE`, which the comparator does on exhaustion.
struct WeightIter<'a> {
    collation: &'a MysqlCollation,
    chars: std::str::Chars<'a>,
    buf: WeightBuf,
    pos: usize,
}

impl<'a> WeightIter<'a> {
    fn new(collation: &'a MysqlCollation, s: &'a str) -> Self {
        Self {
            collation,
            chars: s.chars(),
            buf: WeightBuf::new(),
            pos: 0,
        }
    }

    /// The next weight element, or `None` when the string is exhausted.
    fn next_weight(&mut self) -> Option<u16> {
        loop {
            if self.pos < self.buf.len {
                let w = self.buf.buf[self.pos];
                self.pos += 1;
                return Some(w);
            }
            let ch = self.chars.next()?;
            self.buf = WeightBuf::new();
            push_weight(self.collation, ch, &mut self.buf);
            self.pos = 0;
        }
    }
}

/// Compare two strings exactly as `MySQL` would for this collation.
pub(crate) fn compare(collation: &MysqlCollation, a: &str, b: &str) -> Ordering {
    let pad = collation.pad == Pad::Space;
    let mut ia = WeightIter::new(collation, a);
    let mut ib = WeightIter::new(collation, b);
    loop {
        let (xw, yw) = match (ia.next_weight(), ib.next_weight()) {
            (None, None) => return Ordering::Equal,
            // Under NO PAD the shorter (prefix) string is smaller.
            (Some(_), None) if !pad => return Ordering::Greater,
            (None, Some(_)) if !pad => return Ordering::Less,
            // Under PAD SPACE, treat exhaustion as an endless run of SPACE.
            (Some(xw), None) => (xw, SPACE),
            (None, Some(yw)) => (SPACE, yw),
            (Some(xw), Some(yw)) => (xw, yw),
        };
        match xw.cmp(&yw) {
            Ordering::Equal => {}
            ord => return ord,
        }
    }
}

#[cfg(test)]
mod tests {
    use core::cmp::Ordering;

    use super::*;

    fn ai_ci() -> MysqlCollation {
        MysqlCollation::utf8mb4_0900_ai_ci()
    }

    fn unicode_ci() -> MysqlCollation {
        MysqlCollation::utf8mb4_unicode_ci()
    }

    #[test]
    fn test_case_and_accent_insensitive() {
        let c = ai_ci();
        assert_eq!(compare(&c, "a", "A"), Ordering::Equal);
        assert_eq!(compare(&c, "e", "\u{e9}"), Ordering::Equal);
        assert_eq!(compare(&c, "e", "e\u{301}"), Ordering::Equal);
    }

    #[test]
    fn test_no_pad_treats_trailing_space_as_significant() {
        let c = ai_ci();
        assert_ne!(compare(&c, "a", "a "), Ordering::Equal);
    }

    #[test]
    fn test_pad_space_ignores_trailing_space() {
        let c = unicode_ci();
        assert_eq!(compare(&c, "a", "a "), Ordering::Equal);
        assert_eq!(compare(&c, "a ", "a  "), Ordering::Equal);
    }

    #[test]
    fn test_pad_space_shorter_can_be_greater() {
        // PAD SPACE pads the shorter side with the space weight (0x0209). A
        // longer string whose first extra weight is below SPACE stays smaller.
        let c = unicode_ci();
        // TAB (U+0009, weight 0x0201) < SPACE (0x0209): "a" > "a\t" (MySQL agrees).
        assert_eq!(compare(&c, "a", "a\t"), Ordering::Greater);
        // A longer string with extra letters sorts after the prefix.
        assert_eq!(compare(&c, "a", "ab"), Ordering::Less);
        // Trailing space is padding on both sides.
        assert_eq!(compare(&c, "a", "a "), Ordering::Equal);
    }

    #[test]
    fn test_supplementary_default_ignorable_has_zero_weight() {
        let c = ai_ci();
        assert_eq!(compare(&c, "", "\u{e0001}"), Ordering::Equal);
        assert_eq!(compare(&c, "", "\u{e0100}"), Ordering::Equal);
        // Unassigned neighbour is not ignorable and keeps an implicit weight.
        assert_ne!(compare(&c, "", "\u{e0002}"), Ordering::Equal);
    }

    #[test]
    fn test_ligature_expands() {
        let c = ai_ci();
        // ß folds to "ss" under UCA 9.0.0.
        assert_eq!(compare(&c, "\u{df}", "ss"), Ordering::Equal);
    }

    #[test]
    fn test_multi_element_weights_match_mysql_order() {
        // Pinned against MySQL 8.0 `WEIGHT_STRING` (utf8mb4_0900_ai_ci):
        // elements are emitted first-to-last.
        let c = ai_ci();
        assert_eq!(weights(&c, "\u{bd}"), vec![0x1C3E, 0x0626, 0x1C3F]); // ½
        assert_eq!(weights(&c, "\u{e6}"), vec![0x1C47, 0x1CAA]); // æ
        assert_eq!(
            weights(&c, "\u{33ae}"),
            vec![0x1E33, 0x1C47, 0x1C8F, 0x0625, 0x1E71]
        ); // ㎮

        // And the ordering consequence: under 0900, "½" sorts before "2"
        // (MySQL: ½|2|A|a|æ|B) because ½'s first element 0x1C3E < 2's 0x1C3F.
        assert_eq!(compare(&c, "\u{bd}", "2"), Ordering::Less);
    }

    #[test]
    fn test_implicit_weight_ranges_beyond_table() {
        let c = ai_ci();
        // Witness from the review: U+2CEA1 is the CJK-Ext-E endpoint and falls
        // just past the weight table, so it must use page FB80 (not FBC0). The
        // implicit element is [page, 0x0020, 0x0002, low].
        // MySQL: WEIGHT_STRING(U+2CEA1)=FB85CEA1 < WEIGHT_STRING(U+0378)=FBC08378,
        // so U+2CEA1 sorts before U+0378.
        assert_eq!(weights(&c, "\u{2CEA1}"), vec![0xFB85, 0xCEA1]);
        assert_eq!(compare(&c, "\u{2CEA1}", "\u{0378}"), Ordering::Less);
        assert_eq!(compare(&c, "\u{2CEA1}", "\u{2CEA0}"), Ordering::Greater);
    }

    #[test]
    fn test_uca400_unassigned_uses_generic_implicit() {
        // Witnesses pinned against MySQL 8.4 `WEIGHT_STRING` for
        // utf8mb4_unicode_ci: unassigned BMP code points get the generic
        // `0xFBC0 + (cp >> 15)` page, so below U+8000 it is FBC0 and at or above
        // it is FBC1. The CJK compatibility block gaps (e.g. U+FA2E) are the
        // cases that a naive "0xF900..0xFAFF => FB40" rule gets wrong.
        let c = unicode_ci();
        assert_eq!(weights(&c, "\u{4DB6}"), vec![0xFBC0, 0xCDB6]);
        assert_eq!(weights(&c, "\u{FA2E}"), vec![0xFBC1, 0xFA2E]);
        assert_eq!(weights(&c, "\u{9FA6}"), vec![0xFBC1, 0x9FA6]);
        // Assigned CJK keep their page rules.
        assert_eq!(weights(&c, "\u{3400}"), vec![0xFB80, 0xB400]);
        assert_eq!(weights(&c, "\u{FA29}"), vec![0xFB41, 0xFA29]);
        // Ordering consequence: the unassigned compat point sorts after Ext-A.
        assert_eq!(compare(&c, "\u{3400}", "\u{FA2E}"), Ordering::Less);
    }

    #[test]
    fn test_uca400_overlong_expansion_packs_overflow_page_first() {
        // U+FDFA expands past the 8-element inline limit, so MySQL uses the
        // whole-character fallback [0xFBC1, U+FDFA] (verified:
        // WEIGHT_STRING(U+FDFA)=FBC1FDFA). The page must be emitted first;
        // packing it into the high chunk would reverse the pair and sort the
        // character after the rest of the supplementary range.
        let c = unicode_ci();
        assert_eq!(weights(&c, "\u{FDFA}"), vec![0xFBC1, 0xFDFA]);
        assert_eq!(compare(&c, "\u{FDFA}", "\u{FDFE}"), Ordering::Less);
    }

    #[test]
    fn test_default_ignorable_set_is_unicode_9_0_0() {
        // 0900: default-ignorable per Unicode 9.0.0 (zero weight) ...
        let c = ai_ci();
        for cp in ['\u{061C}', '\u{2064}', '\u{2066}', '\u{2069}', '\u{180E}'] {
            assert_eq!(
                compare(&c, "", &cp.to_string()),
                Ordering::Equal,
                "U+{:04X}",
                cp as u32
            );
        }
        // ... but points added to the set in later Unicode versions are not
        // ignorable under the pinned 9.0.0 data and keep implicit weights.
        for cp in ['\u{180F}', '\u{2065}', '\u{FFFD}'] {
            assert_ne!(
                compare(&c, "", &cp.to_string()),
                Ordering::Equal,
                "U+{:04X}",
                cp as u32
            );
        }
        // 0400: the same BMP points are unassigned and get implicit weights.
        let u = unicode_ci();
        assert_ne!(compare(&u, "", "\u{061C}"), Ordering::Equal);
        // A genuinely ignorable point is zero in both.
        assert_eq!(compare(&c, "", "\u{034F}"), Ordering::Equal);
        assert_eq!(compare(&u, "", "\u{034F}"), Ordering::Equal);
    }

    #[test]
    fn test_hangul_syllable_decomposes() {
        let c = ai_ci();
        // U+AC00 decomposes to jamo U+1100 U+1161; MySQL: 3BF53C73.
        assert_eq!(weights(&c, "\u{AC00}"), vec![0x3BF5, 0x3C73]);
    }

    #[test]
    fn test_validate_rejects_inconsistent_spec() {
        let bad = MysqlCollation {
            name: "utf8mb4_0900_ai_ci".to_owned(),
            uca_version: MysqlUcaVersion::Uca400,
            pad: Pad::NoPad,
        };
        assert!(validate(&bad).is_err());
    }
}
