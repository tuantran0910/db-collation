"""Regenerate the Rust weight-table modules from the Unicode UCA data files.

Source of truth: the Unicode Collation Algorithm `allkeys` files, vendored in
`harness/data/`:

* UCA 4.0.0  -> `allkeys-4.0.0.txt`  (used by `utf8mb4_unicode_ci`)
* UCA 9.0.0  -> `allkeys-9.0.0.txt`  (used by `utf8mb4_0900_ai_ci`)

The generated tables reproduce MySQL's comparison weights, which are the
*primary* weights of the DUCET collation elements with zero primaries dropped:

* pack the first (up to) eight nonzero primaries, first element in the least
  significant 16-bit chunk — matching MySQL's `WEIGHT_STRING` element order;
* UCA 4.0.0 additionally treats an expansion longer than eight primaries as a
  whole-character fallback `[0xFBC1, cp]` (only U+FDFA qualifies).

These rules were verified against MySQL 8.0/8.4/9.4 `WEIGHT_STRING` and the
differential harness. No third-party intermediate table is used.

Run from the repository root: `python harness/gen_weights.py` (or `make gen-weights`).
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "harness" / "data"
OUT = ROOT / "crates" / "db-collation" / "src" / "mysql"

MAX_WEIGHTS = 8
UCA400_OVERFLOW_PAGE = 0xFBC1

# MySQL inlines the *implicit* weight for code points within the table range
# that have no DUCET entry, so the runtime only computes implicit weights beyond
# the table. These ranges mirror `uca_scanner_900::next_implicit` (root
# collation). See crates/db-collation/src/mysql.rs for the same rules.
_TANGUT = (0x17000, 0x18AFF)
_CJK_EXT = [
    (0x3400, 0x4DB5),
    (0x20000, 0x2A6D6),
    (0x2A700, 0x2B734),
    (0x2B740, 0x2B81D),
    (0x2B820, 0x2CEA1),
]
_CJK_MAIN = [(0x4E00, 0x9FD5), (0xFA0E, 0xFA29)]


def _implicit_400(cp):
    """Implicit weight for UCA 4.0.0 (MySQL `utf8mb4_unicode_ci`).

    Derived from MySQL 8.0 `WEIGHT_STRING`. `allkeys-4.0.0` lists every assigned
    code point (including CJK), so this is only reached for *unassigned* BMP code
    points. MySQL applies:

    * CJK Ext-A 0x3400..0x4DB5  -> page FB80
    * CJK       0x4E00..0x9FA5  -> page FB40
    * everything else           -> generic page ``0xFBC0 + (cp >> 15)``, whose
      low half is ``(cp & 0x7FFF) | 0x8000``. For cp >= 0x8000 this page is
      FBC1 (matching MySQL's Hangul-syllable and PUA/implicit weights).
    """
    if 0x3400 <= cp <= 0x4DB5:
        return [(cp >> 15) + 0xFB80, (cp & 0x7FFF) | 0x8000]
    if 0x4E00 <= cp <= 0x9FA5:
        return [(cp >> 15) + 0xFB40, (cp & 0x7FFF) | 0x8000]
    return [(cp >> 15) + 0xFBC0, (cp & 0x7FFF) | 0x8000]


def _implicit_900(cp):
    """Implicit primary pair [page, low] for a UCA-9 out-of-DUCET code point."""
    if _TANGUT[0] <= cp <= _TANGUT[1]:
        return [0xFB00, (cp - _TANGUT[0]) | 0x8000]
    if any(lo <= cp <= hi for lo, hi in _CJK_EXT):
        return [(cp >> 15) + 0xFB80, (cp & 0x7FFF) | 0x8000]
    if any(lo <= cp <= hi for lo, hi in _CJK_MAIN):
        return [(cp >> 15) + 0xFB40, (cp & 0x7FFF) | 0x8000]
    return [(cp >> 15) + 0xFBC0, (cp & 0x7FFF) | 0x8000]


def _hangul_jamo(cp):
    """Decompose a Hangul syllable to jamo code points, or None."""
    if not (0xAC00 <= cp <= 0xD7AF):
        return None
    index = cp - 0xAC00
    v_t = 21 * 28
    lead = 0x1100 + index // v_t
    vowel = 0x1161 + (index % v_t) // 28
    trailing_index = index % 28
    return [lead, vowel, 0x11A7 + trailing_index] if trailing_index else [lead, vowel]


HDR = """// This file is generated from the Unicode Collation Algorithm data file
// {allkeys} (Unicode License v3). Do not edit by hand.
//
// Copyright 2026 db-collation contributors
// Unicode data: Copyright © Unicode, Inc. Licensed under Unicode-3.0.
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
//
//! GENERATED from the Unicode UCA {uca} weight table
//! (`harness/data/{allkeys}`). Do not edit by hand.
//! Regenerate with `harness/gen_weights.py`.
//! See THIRD-PARTY-NOTICES for the Unicode License v3 notice.
#![allow(clippy::unreadable_literal, clippy::too_many_lines, clippy::cast_possible_truncation)]

/// The expansion table for characters whose weight is multi-element.
static LONG_RUNE_MAP: &[(u32, u128)] = &[
"""

# Matches one collation element: [flag.primary.secondary.tertiary[.quaternary]]
_CE = re.compile(
    r"\[([.*])([0-9A-Fa-f]{4})\.([0-9A-Fa-f]{4})\.([0-9A-Fa-f]{4})(?:\.[0-9A-Fa-f]{4})?\]"
)


def parse_allkeys(path):
    """Return {single_codepoint: [nonzero primaries]} for length-1 sequences."""
    table = {}
    for raw in Path(path).read_text().splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line or line.startswith("@") or ";" not in line:
            continue
        seq, rhs = line.split(";", 1)
        codepoints = [int(x, 16) for x in seq.split()]
        if len(codepoints) != 1:
            # Only single-code-point entries go into the per-code-point table;
            # contractions/expansions are handled by the character-weight map.
            continue
        primaries = [int(p, 16) for _flag, p, _s, _t in _CE.findall(rhs)]
        table[codepoints[0]] = [p for p in primaries if p != 0]
    return table


def pack(primaries):
    value = 0
    for index, weight in enumerate(primaries):
        value |= weight << (16 * index)
    return value


def build_table(weights, highest, overflow_page):
    """Build the per-code-point u64 array up to `highest`.

    A character's weight is stored inline when its packed value fits in 64 bits
    (up to four 16-bit elements). Longer expansions use the `0xFFFD` sentinel and
    live in `LONG_RUNE_MAP` (the crate's `u128` map).
    """
    table = [0] * (highest + 1)
    long_runes = {}
    for cp in range(highest + 1):
        primaries = weights.get(cp)
        if primaries is None:
            # No entry in `allkeys`: MySQL assigns an implicit weight. Genuine
            # default-ignorable code points are listed explicitly with zero
            # primaries in `allkeys` (handled below), so this only covers
            # unassigned/blank code points.
            if overflow_page is not None:
                # UCA 4.0.0 / MySQL BMP implicit rules (Hangul syllables, PUA, and
                # over-long ranges fall back to [0xFBC1, cp]).
                packed = pack(_implicit_400(cp))
                table[cp] = packed if packed <= 0xFFFF_FFFF_FFFF_FFFF else 0xFFFD
            else:
                jamo = _hangul_jamo(cp)
                if jamo is not None:
                    prims = []
                    for j in jamo:
                        prims.extend(weights.get(j, [])[:MAX_WEIGHTS])
                    packed = pack(prims[:MAX_WEIGHTS])
                    table[cp] = packed if packed <= 0xFFFF_FFFF_FFFF_FFFF else 0xFFFD
                    if packed > 0xFFFF_FFFF_FFFF_FFFF:
                        long_runes[cp] = packed
                else:
                    table[cp] = pack(_implicit_900(cp))
            continue
        if not primaries:
            table[cp] = 0
        elif len(primaries) > MAX_WEIGHTS and overflow_page is not None:
            # Whole-character fallback (UCA 4.0.0 / MySQL): the primary pair
            # [overflow_page, cp], packed first-element-first like every other
            # entry so the crate's low-chunk-first emission preserves the order.
            table[cp] = pack([overflow_page, cp])
            long_runes[cp] = table[cp]
        else:
            packed = pack(primaries[:MAX_WEIGHTS])
            if packed <= 0xFFFF_FFFF_FFFF_FFFF:
                table[cp] = packed
            else:
                # Too wide for the u64 table: sentinel + u128 map.
                table[cp] = 0xFFFD
                long_runes[cp] = packed
    return table, long_runes


def emit(src, dst, ver, uca, allkeys, highest, overflow_page):
    weights = parse_allkeys(src)
    table, long_runes = build_table(weights, highest, overflow_page)

    lines = [HDR.format(uca=uca, allkeys=allkeys)]
    for cp, value in sorted(long_runes.items()):
        lines.append(f"    (0x{cp:X}, 0x{value:X}),\n")
    lines.append("];\n\n")
    lines.append("/// Per-code-point weight. Returns `None` if `cp` is out of range.\n")
    lines.append("pub(super) fn weight(cp: u32) -> Option<u128> {\n")
    lines.append("    static TABLE: &[u64] = &[\n")
    for i in range(0, len(table), 12):
        lines.append("        " + ", ".join(f"0x{v:X}" for v in table[i : i + 12]) + ",\n")
    lines.append("    ];\n")
    lines.append("    let u = *TABLE.get(cp as usize)?;\n")
    lines.append("    if u == 0xFFFD {\n")
    lines.append("        for &(c, v) in LONG_RUNE_MAP {\n")
    lines.append("            if c == cp { return Some(v); }\n")
    lines.append("        }\n")
    lines.append("        return Some(0xFFFDu128);\n")
    lines.append("    }\n")
    lines.append("    Some(u.into())\n")
    lines.append("}\n")
    Path(dst).write_text("".join(lines))
    print(f"{dst}: {len(table)} entries, {len(long_runes)} expansions (UCA {uca})")


if __name__ == "__main__":
    # 0900 covers up to the last assigned CJK-Ext-E code point; beyond it the
    # crate uses the implicit-weight path (matching MySQL's table extent).
    emit(
        DATA / "allkeys-9.0.0.txt",
        OUT / "table_0900.rs",
        "0900",
        "9.0.0",
        "allkeys-9.0.0.txt",
        0x2CEA0,
        None,
    )
    # 0400 is BMP-only; supplementary characters compare as U+FFFD.
    emit(
        DATA / "allkeys-4.0.0.txt",
        OUT / "table_0400.rs",
        "0400",
        "4.0.0",
        "allkeys-4.0.0.txt",
        0xFFFF,
        UCA400_OVERFLOW_PAGE,
    )
