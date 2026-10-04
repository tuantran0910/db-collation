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

//! The Unicode Collation Algorithm, reproducing Oracle's `UCA*_DUCET`
//! collations.
//!
//! Oracle's default UCA parameters are `_S4_VS_BN_NY_EN_FN_HN_DN_MN`:
//! quaternary strength, **shifted** variable weighting, NFD normalization,
//! forward levels. This module implements precisely that:
//!
//! 1. **NFD** the input.
//! 2. Build a collation-element array by longest-match lookup (including
//!    discontiguous contractions across unblocked non-starters), synthesising
//!    implicit weights for code points absent from the DUCET.
//! 3. Apply **shifted** variable weighting: variable elements move to
//!    a quaternary weight, non-variables get `FFFF`, and ignorables following a
//!    variable become fully ignorable.
//! 4. Compare level by level, L1 → L2 → L3 → L4, then stop. Oracle has **no
//!    `identical` level**, so strings equal at L4 compare equal.

// The implicit-weight arithmetic masks code points into 16-bit lanes
// (`>> 15`, `& 0x7FFF`, `cp - lo` within a table range), so every narrowing
// cast here is bounded by construction.
#![allow(clippy::cast_possible_truncation)]

use core::cmp::Ordering;

use crate::spec::OracleUcaVersion;

use super::table_uca0700;
use super::table_uca1210;

/// A packed collation element: variable flag, primary, secondary, tertiary.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ce {
    primary: u16,
    secondary: u16,
    tertiary: u16,
    variable: bool,
}

impl Ce {
    const fn unpack(packed: u64) -> Self {
        Self {
            primary: ((packed >> 32) & 0xFFFF) as u16,
            secondary: ((packed >> 16) & 0xFFFF) as u16,
            tertiary: (packed & 0xFFFF) as u16,
            variable: (packed >> 48) & 1 != 0,
        }
    }
}

/// The longest contraction key length in the generated DUCET tables.
const MAX_CONTRACTION_LEN: usize = 3;

/// A versioned DUCET table.
pub(super) struct UcaTable {
    pub(super) ces: &'static [u64],
    pub(super) singles: &'static [(u32, u32, u8)],
    pub(super) contractions: &'static [(u8, &'static [u32], u32, u8)],
    pub(super) implicit: &'static [(u32, u32, u16)],
    pub(super) unified_ideograph: &'static [(u32, u32, bool)],
    /// Canonical decompositions `(cp, blob_offset, len)` for Oracle's fixed NFD.
    pub(super) decomp: &'static [(u32, u32, u8)],
    pub(super) decomp_blob: &'static [u32],
    /// Non-zero canonical combining classes `(cp, class)`.
    pub(super) ccc: &'static [(u32, u8)],
}

impl UcaTable {
    /// The canonical decomposition of `cp`, if any (excluding Hangul).
    fn decompose(&self, cp: u32) -> Option<&[u32]> {
        self.decomp
            .binary_search_by_key(&cp, |&(c, _, _)| c)
            .ok()
            .map(|i| {
                let (_, off, len) = self.decomp[i];
                &self.decomp_blob[off as usize..off as usize + len as usize]
            })
    }

    /// The canonical combining class of `cp` (0 if not listed).
    fn combining_class(&self, cp: u32) -> u8 {
        self.ccc
            .binary_search_by_key(&cp, |&(c, _)| c)
            .map_or(0, |i| self.ccc[i].1)
    }

    /// NFD as Oracle normalizes it: recursively decompose (with algorithmic
    /// Hangul decomposition), then apply canonical ordering by combining class.
    ///
    /// This deliberately does **not** use `unicode_normalization`'s NFD, whose
    /// tables track the *latest* Unicode. Oracle normalizes with a single fixed
    /// Unicode version regardless of the UCA weight-table version, so a code
    /// point assigned after that version (with a canonical decomposition or
    /// combining class unknown to Oracle) must stay atomic / unreordered. The
    /// generated `DECOMP` and `CCC` tables are pinned to that version.
    fn nfd(&self, s: &str) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::with_capacity(s.len());
        for ch in s.chars() {
            self.decompose_into(ch as u32, &mut out);
        }
        // Canonical ordering: stable-sort each maximal run of non-starters by
        // combining class (UTS #15 canonical ordering algorithm).
        self.canonical_order(&mut out);
        out
    }

    fn decompose_into(&self, cp: u32, out: &mut Vec<u32>) {
        // Hangul syllable decomposition (UTS #15, §3.12), algorithmic.
        if (0xAC00..=0xD7A3).contains(&cp) {
            const S_BASE: u32 = 0xAC00;
            const L_BASE: u32 = 0x1100;
            const V_BASE: u32 = 0x1161;
            const T_BASE: u32 = 0x11A7;
            const T_COUNT: u32 = 28;
            const N_COUNT: u32 = 21 * T_COUNT;
            let s_index = cp - S_BASE;
            let l = L_BASE + s_index / N_COUNT;
            let v = V_BASE + (s_index % N_COUNT) / T_COUNT;
            let t = T_BASE + s_index % T_COUNT;
            out.push(l);
            out.push(v);
            if s_index % T_COUNT != 0 {
                out.push(t);
            }
            return;
        }
        if let Some(parts) = self.decompose(cp) {
            for &p in parts {
                self.decompose_into(p, out);
            }
        } else {
            out.push(cp);
        }
    }

    fn canonical_order(&self, cps: &mut [u32]) {
        let n = cps.len();
        let mut i = 0;
        while i < n {
            // Skip starters up to the next non-starter run.
            if self.combining_class(cps[i]) == 0 {
                i += 1;
                continue;
            }
            let start = i;
            let mut end = i;
            while end < n && self.combining_class(cps[end]) != 0 {
                end += 1;
            }
            // Stable insertion sort by combining class within `[start, end)`.
            for a in (start + 1)..end {
                let mut b = a;
                while b > start && self.combining_class(cps[b - 1]) > self.combining_class(cps[b]) {
                    cps.swap(b - 1, b);
                    b -= 1;
                }
            }
            i = end;
        }
    }

    fn ces_at(&self, offset: u32, count: u8) -> &[u64] {
        &self.ces[offset as usize..offset as usize + count as usize]
    }

    /// Explicit elements for a single code point, if present.
    fn single(&self, cp: u32) -> Option<&[u64]> {
        self.singles
            .binary_search_by_key(&cp, |&(c, _, _)| c)
            .ok()
            .map(|i| {
                let (_, off, cnt) = self.singles[i];
                self.ces_at(off, cnt)
            })
    }

    /// The collation elements for the contraction whose key is exactly `key`, if
    /// the table defines one. Keys are sorted by `(len, code points)`.
    fn contraction_for(&self, key: &[u32]) -> Option<&[u64]> {
        self.contractions
            .binary_search_by(|(cl, k, _, _)| (*cl as usize, *k).cmp(&(key.len(), key)))
            .ok()
            .map(|j| {
                let (_, _, off, cnt) = self.contractions[j];
                self.ces_at(off, cnt)
            })
    }

    /// Implicit elements for a code point absent from the table, as
    /// `[.AAAA.0020.0002][.BBBB.0000.0000]`.
    fn implicit(&self, cp: u32) -> [u64; 2] {
        for &(lo, hi, base) in self.implicit {
            if (lo..=hi).contains(&cp) {
                return implicit_pair(base, ((cp - lo) as u16) | 0x8000);
            }
        }
        // Han unified ideographs get their own implicit lead unit; every other
        // code point (including assigned non-ideographs and unassigned code
        // points) uses the generic `FB C0 +` range. Core Han uses `FB40`,
        // other Han uses `FB80`.
        let mut han = None;
        for &(lo, hi, core) in self.unified_ideograph {
            if (lo..=hi).contains(&cp) {
                han = Some(core);
                break;
            }
        }
        let (aaaa, bbbb) = match han {
            None => (0xFBC0 + (cp >> 15) as u16, ((cp & 0x7FFF) | 0x8000) as u16),
            Some(true) => (0xFB40 + (cp >> 15) as u16, ((cp & 0x7FFF) | 0x8000) as u16),
            Some(false) => (0xFB80 + (cp >> 15) as u16, ((cp & 0x7FFF) | 0x8000) as u16),
        };
        implicit_pair(aaaa, bbbb)
    }

    /// Build the collation-element array for an NFD code-point sequence: at each
    /// position take the longest contiguous table match, then extend it across
    /// following unblocked non-starters whenever the extended sequence is also a
    /// contraction.
    ///
    /// A non-starter `C` following the match is eligible when no intervening
    /// character `B` has `ccc(B) = 0` (a starter) or `ccc(B) ≥ ccc(C)`. An
    /// eligible mark that extends the match is consumed, i.e. physically removed
    /// from the stream; one that does not stays and is processed on its own.
    ///
    /// A `cursor` advances past the already-processed prefix so that plain input
    /// is handled without repeatedly shifting the remaining code points. Only a
    /// consumed lookahead mark (which lies after the cursor) is removed from the
    /// body; this keeps the remaining marks logically adjacent and prevents a
    /// consumed mark from taking part in a later match.
    fn element_array(&self, cps: &[u32]) -> Vec<Ce> {
        let mut out = Vec::with_capacity(cps.len() + 2);
        let mut work: Vec<u32> = cps.to_vec();
        let mut cursor = 0usize;
        while cursor < work.len() {
            let rest = &work[cursor..];
            // Longest contiguous initial substring with a table match. Prefer a
            // multi-code-point contraction, else a single-codepoint entry; a code
            // point with no entry has no contraction and is implicitly weighted.
            let mut key: Vec<u32>;
            let mut base: Option<&[u64]>;
            let contig_len;
            if let Some((len, els)) = self.longest_contraction(rest) {
                key = rest[..len].to_vec();
                base = Some(els);
                contig_len = len;
            } else if let Some(els) = self.single(rest[0]) {
                key = vec![rest[0]];
                base = Some(els);
                contig_len = 1;
            } else {
                // No table mapping: emit implicit elements and move on.
                out.extend(self.implicit(rest[0]).iter().map(|&p| Ce::unpack(p)));
                cursor += 1;
                continue;
            }

            // Extend across unblocked non-starters, looking ahead only over the
            // unprocessed remainder.
            let mut scan = cursor + contig_len;
            let mut max_skipped_ccc = 0u8;
            while scan < work.len() {
                let c = work[scan];
                let ccc_c = self.combining_class(c);
                if ccc_c == 0 {
                    // A starter always terminates the discontiguous lookahead.
                    break;
                }
                if ccc_c <= max_skipped_ccc {
                    // Blocked by an earlier skipped mark.
                    scan += 1;
                    continue;
                }
                key.push(c);
                if let Some(els) = self.contraction_for(&key) {
                    // Extend the match and physically remove `C` from the stream,
                    // so it can never match again. `contig_len` (the contiguous
                    // prefix) is unchanged; the following character shifts into
                    // `C`'s place, so `scan` is not advanced.
                    base = Some(els);
                    work.remove(scan);
                } else {
                    key.pop();
                    max_skipped_ccc = max_skipped_ccc.max(ccc_c);
                    scan += 1;
                }
            }

            out.extend(base.unwrap().iter().map(|&p| Ce::unpack(p)));
            cursor += contig_len;
        }
        out
    }

    /// The longest contiguous contraction match at the start of `cps`, if any.
    fn longest_contraction(&self, cps: &[u32]) -> Option<(usize, &[u64])> {
        for len in (2..=MAX_CONTRACTION_LEN).rev() {
            if len > cps.len() {
                continue;
            }
            if let Some(els) = self.contraction_for(&cps[..len]) {
                return Some((len, els));
            }
        }
        None
    }
}

fn pack(primary: u16, secondary: u16, tertiary: u16, variable: bool) -> u64 {
    (u64::from(variable) << 48)
        | (u64::from(primary) << 32)
        | (u64::from(secondary) << 16)
        | u64::from(tertiary)
}

fn implicit_pair(aaaa: u16, bbbb: u16) -> [u64; 2] {
    [pack(aaaa, 0x0020, 0x0002, false), pack(bbbb, 0, 0, false)]
}

fn table_for(version: OracleUcaVersion) -> &'static UcaTable {
    match version {
        OracleUcaVersion::Uca700 => &table_uca0700::TABLE,
        OracleUcaVersion::Uca1210 => &table_uca1210::TABLE,
    }
}

/// Compare two strings under a DUCET collation (Oracle `UCA*_DUCET`).
///
/// Returns `None` when either input might exceed Oracle's 2000-byte sort-key
/// limit, beyond which Oracle truncates the key and the result is no longer the
/// pure-DUCET order modelled here. The caller maps this to `Error::InputLimit`.
///
/// Oracle's key layout is undocumented, so instead of reproducing its exact
/// byte length this uses a **provable upper bound** on that length. A string
/// with `n` collation elements produces a key of at most `8·n + 8` bytes (each
/// of the four levels contributes at most two bytes per element, plus level
/// terminators), so a string whose bound reaches 2000 bytes is refused. This
/// refuses slightly before Oracle's true limit (a safe over-approximation:
/// over-refusal is allowed by "exact, or refuse", under-refusal is not).
pub(super) fn compare(version: OracleUcaVersion, a: &str, b: &str) -> Option<Ordering> {
    let table = table_for(version);
    // Normalize each input once and build its collation-element array once; the
    // same arrays serve both the length guard and every level comparison.
    let left = table.element_array(&table.nfd(a));
    let right = table.element_array(&table.nfd(b));
    if exceeds_key_limit(left.len()) || exceeds_key_limit(right.len()) {
        return None;
    }
    // Oracle has no `identical` level: compare L1..L4 and stop.
    for level in 1u8..=4 {
        match compare_level(&left, &right, level) {
            Ordering::Equal => {}
            other => return Some(other),
        }
    }
    Some(Ordering::Equal)
}

/// Oracle's maximum `NLSSORT` key size in bytes. Keys longer than this are
/// truncated by Oracle, so the comparison is no longer pure DUCET.
const ORACLE_MAX_KEY: usize = 2000;

/// An upper bound, in bytes, on the per-collation-element key contribution: at
/// most two bytes at each of the four levels.
const MAX_BYTES_PER_CE: usize = 8;

/// A fixed upper bound on Oracle's per-level terminators and key header.
const KEY_OVERHEAD: usize = 8;

/// Whether a collation-element array of `count` elements might produce an Oracle
/// sort key longer than [`ORACLE_MAX_KEY`], applying the conservative bound.
fn exceeds_key_limit(count: usize) -> bool {
    let threshold = (ORACLE_MAX_KEY - KEY_OVERHEAD) / MAX_BYTES_PER_CE;
    count > threshold
}

fn compare_level(a_ces: &[Ce], b_ces: &[Ce], level: u8) -> Ordering {
    let mut wa = WeightIter::new(a_ces, level);
    let mut wb = WeightIter::new(b_ces, level);
    loop {
        match (wa.next_weight(), wb.next_weight()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match x.cmp(&y) {
                Ordering::Equal => {}
                other => return other,
            },
        }
    }
}

/// Streams the non-zero weights of one collation-element array at one level,
/// applying shifted variable weighting on the fly.
struct WeightIter<'a> {
    ces: &'a [Ce],
    idx: usize,
    level: u8,
    /// Whether the last consumed collation element was a variable one.
    after_variable: bool,
}

impl<'a> WeightIter<'a> {
    fn new(ces: &'a [Ce], level: u8) -> Self {
        Self {
            ces,
            idx: 0,
            level,
            after_variable: false,
        }
    }

    /// Compute the shifted weight for `ce` at this level, if any.
    ///
    /// `after_variable` is true when the immediately preceding collation
    /// element was a variable one. Under `shifted`, any *ignorable* element
    /// (zero primary) that follows a variable is reset to zero at **all** levels,
    /// so it contributes no weight at any level.
    fn weight_of(&self, ce: Ce, after_variable: bool) -> Option<u16> {
        if after_variable && ce.primary == 0 {
            // Ignorable following a variable: fully suppressed.
            return None;
        }
        match self.level {
            1 => (!ce.variable && ce.primary != 0).then_some(ce.primary),
            2 => (!ce.variable && ce.secondary != 0).then_some(ce.secondary),
            3 => (!ce.variable && ce.tertiary != 0).then_some(ce.tertiary),
            _ => {
                if ce.variable {
                    (ce.primary != 0).then_some(ce.primary)
                } else if ce.primary != 0 || ce.secondary != 0 || ce.tertiary != 0 {
                    // Non-variable, non-ignorable → FFFF.
                    Some(0xFFFF)
                } else {
                    None
                }
            }
        }
    }

    fn next_weight(&mut self) -> Option<u16> {
        loop {
            let ce = *self.ces.get(self.idx)?;
            self.idx += 1;
            let w = self.weight_of(ce, self.after_variable);
            // Track the "inside a variable run" state: a variable element starts
            // a run, a following ignorable continues it, and any non-variable
            // primary element ends it.
            self.after_variable = ce.variable || (self.after_variable && ce.primary == 0);
            if let Some(w) = w {
                return Some(w);
            }
        }
    }
}
