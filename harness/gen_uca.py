"""Generate Oracle UCA (DUCET) weight tables from Unicode `allkeys` files.

Emits `crates/db-collation/src/oracle/table_uca1210.rs` and
`table_uca0700.rs`. Each table exposes a `Ce` (packed collation element) type
and a lookup from a codepoint to its collation-element slice, plus contractions
(multi-codepoint keys) and the implicit-weight rule.

Unlike the MySQL generator, this keeps **all** of each collation element
(primary, secondary, tertiary) and the DUCET variable flag, because Oracle's
default UCA collation is quaternary/shifted, not primary-only.

Usage:  python -m harness.gen_uca
"""

import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "harness" / "data"
OUT = ROOT / "crates" / "db-collation" / "src" / "oracle"

HEADER = """\
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

// @generated from {allkeys} (UCA {uca}) by harness/gen_uca.py.
// Do not edit by hand; run `make gen-weights`.
#![allow(clippy::unreadable_literal, clippy::too_many_lines)]
"""

CE_RE = re.compile(r"\[([.*])([0-9A-Fa-f]{4})\.([0-9A-Fa-f]{4})\.([0-9A-Fa-f]{4})\]")

# Code-point ranges Oracle treats as Han unified ideographs for implicit
# weighting. "Core" uses `FB40 + (cp>>15)`, "other" uses `FB80 + (cp>>15)`;
# everything else (including unassigned holes) uses the generic `FBC0 +`.
# These are the CJK Radicals / Kangxi / CJK Symbols / Unified / Compatibility
# Ideographs blocks and their extensions, as Oracle applies them. Verified
# against live Oracle `NLSSORT` in the differential harness.
HAN_CORE_1210 = [
    (0x2E80, 0x2E99),
    (0x2E9B, 0x2EF3),
    (0x2F00, 0x2FD5),
    (0x3038, 0x303A),
    (0x3192, 0x319F),
    (0x3220, 0x3247),
    (0x3280, 0x32B0),
    (0x32FF, 0x32FF),
    (0x337B, 0x337F),
    (0x4E00, 0x9FFF),
    (0xF900, 0xFA6B),
    (0xFA6D, 0xFA6D),
    (0xFA70, 0xFACE),
    (0xFAD8, 0xFAD9),
]
# UCA 7.0 treats U+32FF (a later addition) as non-ideographic, so its core set
# omits it; everything else matches 12.1.
HAN_CORE_0700 = [r for r in HAN_CORE_1210 if r != (0x32FF, 0x32FF)]
HAN_OTHER = [
    (0x3400, 0x4DBF),
    (0xFA6C, 0xFA6C),
    (0xFACF, 0xFAD7),
    (0x20000, 0x2A6DF),
    (0x2A700, 0x2B739),
    (0x2B740, 0x2B81D),
    (0x2B820, 0x2CEA1),
    (0x2CEB0, 0x2EBE0),
    # CJK Extensions G and H: present in the live Oracle data for *both* named
    # UCA versions (verified by sweeping every scalar against Oracle 23.5). The
    # gap U+3134B..U+3134F and U+323B0 onward stay generic.
    (0x30000, 0x3134A),
    (0x31350, 0x323AF),
]

# Oracle normalizes with a **fixed** Unicode version, independent of the UCA
# weight-table version: the same decomposition and canonical-ordering data
# applies to `UCA1210_DUCET` and `UCA0700_DUCET` alike. Probing the live source
# shows it knows the Unicode 15.0 canonical combining classes (e.g. U+0898,
# U+10EFD) but not those added in 16.0 (e.g. U+10D69), and it decomposes the
# U+11938 mapping added in 13.0 under both named collations. 15.1 adds no
# decomposition or combining-class data, so 15.0 pins it exactly.
NORMALIZATION_DATA = "UnicodeData-15.0.0.txt"

# Oracle's fixed implicit-weight ranges. These are part of Oracle's collation
# data, independent of the Unicode version: `allkeys-12.1.0.txt` carries matching
# `@implicitweights` lines, but `allkeys-7.0.0.txt` (predating Tangut/Nushu) does
# not, while Oracle still applies the same ranges. They are injected for every
# version so 7.0 matches Oracle too.
ORACLE_IMPLICIT_RANGES = [
    (0x17000, 0x18AFF, 0xFB00),  # Tangut
    (0x1B170, 0x1B2FF, 0xFB01),  # Nushu
]


def parse_allkeys(path):
    """Return (singles, contractions, implicit_ranges).

    singles: {cp: [Ce, ...]}
    contractions: {tuple(cps): [Ce, ...]}
    implicit_ranges: [(start, end, base), ...]
    """
    singles = {}
    contractions = {}
    implicit_ranges = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("@implicitweights"):
            # e.g. "@implicitweights 17000..18AFF; FB00"
            spec = line[len("@implicitweights") :].strip()
            rng, _, base = spec.partition(";")
            rng = rng.strip()
            base = int(base.strip(), 16) if base.strip() else 0
            lo, _, hi = rng.partition("..")
            implicit_ranges.append((int(lo, 16), int(hi, 16), base))
            continue
        if line.startswith("@"):
            continue
        cp_part, _, ce_part = line.partition(";")
        cp_part = cp_part.strip()
        if ".." in cp_part:
            continue
        cps = [int(x, 16) for x in cp_part.split()]
        ces = []
        for m in CE_RE.finditer(ce_part):
            variable = m.group(1) == "*"
            p = int(m.group(2), 16)
            s = int(m.group(3), 16)
            t = int(m.group(4), 16)
            ces.append((p, s, t, variable))
        if not ces:
            continue
        if len(cps) == 1:
            singles[cps[0]] = ces
        else:
            contractions[tuple(cps)] = ces
    return singles, contractions, implicit_ranges


def parse_unicode_data(path):
    """Return `(decomp, ccc)` from a `UnicodeData.txt`.

    `decomp`: {cp: [cps]} for canonical (non-compatibility) decompositions,
    excluding the Hangul syllables (which are algorithmic and handled in Rust).
    `ccc`: {cp: combining_class} for code points with a non-zero class.

    Oracle normalizes to NFD using the Unicode version of its collation data.
    Vendoring the target version's `UnicodeData` lets the crate reproduce that
    NFD exactly, instead of the modern Unicode tables a normalizer crate ships.
    """
    decomp = {}
    ccc = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        if not raw:
            continue
        f = raw.split(";")
        if len(f) < 6:
            continue
        cp = int(f[0], 16)
        cls = int(f[3])
        if cls:
            ccc[cp] = cls
        field = f[5].strip()
        if not field or field.startswith("<"):
            # Compatibility decomposition (or none): NFD leaves it as-is.
            continue
        # Hangul syllables have an algorithmic decomposition; skip them here.
        if 0xAC00 <= cp <= 0xD7A3:
            continue
        decomp[cp] = [int(x, 16) for x in field.split()]
    return decomp, ccc


def pack_ce(ce):
    """Pack one collation element into a u64.

    Layout: bits 48 = variable flag, 32..47 = primary (16), 16..31 = secondary,
    0..15 = tertiary. DUCET assigned primaries fit in 16 bits; implicit weights
    are synthesised at runtime and also fit in 16 bits.
    """
    p, s, t, variable = ce
    if p > 0xFFFF or s > 0xFFFF or t > 0xFFFF:
        raise ValueError(f"collation element out of u16 range: {ce}")
    return (1 << 48 if variable else 0) | (p << 32) | (s << 16) | t


def emit(allkeys, dst, uca, module_doc, han_core):
    singles, contractions, implicit_ranges = parse_allkeys(DATA / allkeys)
    # Oracle applies its fixed implicit ranges regardless of the UCA file version.
    for rng in ORACLE_IMPLICIT_RANGES:
        if rng not in implicit_ranges:
            implicit_ranges.append(rng)

    # Flat CE array + index. Sort codepoints for a stable, binary-searchable table.
    cps = sorted(singles)
    ce_blob = []
    index = []  # (cp, offset, count)
    for cp in cps:
        ces = singles[cp]
        index.append((cp, len(ce_blob), len(ces)))
        ce_blob.extend(pack_ce(c) for c in ces)

    # Contractions: sorted by (len, cps) for deterministic output.
    contra_items = sorted(contractions.items(), key=lambda kv: (len(kv[0]), kv[0]))
    contra_index = []  # (len, [cps...], offset, count)
    for key, ces in contra_items:
        contra_index.append((len(key), list(key), len(ce_blob), len(ces)))
        ce_blob.extend(pack_ce(c) for c in ces)

    lines = [HEADER.format(allkeys=allkeys, uca=uca)]
    lines.append(f"//! {module_doc}\n")
    lines.append("use super::uca::UcaTable;\n")

    lines.append("/// DUCET collation elements, packed by `harness/gen_uca.py`.")
    lines.append("pub(super) static CES: &[u64] = &[")
    for i in range(0, len(ce_blob), 6):
        chunk = ", ".join(f"0x{v:016X}" for v in ce_blob[i : i + 6])
        lines.append(f"    {chunk},")
    lines.append("];\n")

    lines.append("/// Single-codepoint index: `(codepoint, ce_offset, ce_count)`.")
    lines.append("pub(super) static SINGLES: &[(u32, u32, u8)] = &[")
    for cp, off, cnt in index:
        lines.append(f"    (0x{cp:06X}, {off}, {cnt}),")
    lines.append("];\n")

    lines.append("/// Contractions: `(len, cps, ce_offset, ce_count)`, cps packed as u32.")
    lines.append("pub(super) static CONTRACTIONS: &[(u8, &[u32], u32, u8)] = &[")
    for ln, key, off, cnt in contra_index:
        cps_lit = ", ".join(f"0x{c:06X}" for c in key)
        lines.append(f"    ({ln}, &[{cps_lit}], {off}, {cnt}),")
    lines.append("];\n")

    lines.append("/// Implicit-weight ranges `(start, end, base_primary)`.")
    lines.append("pub(super) static IMPLICIT: &[(u32, u32, u16)] = &[")
    for lo, hi, base in sorted(implicit_ranges):
        lines.append(f"    (0x{lo:06X}, 0x{hi:06X}, 0x{base:04X}),")
    if not implicit_ranges:
        lines.append("    // (this UCA version has no explicit @implicitweights ranges)")
    lines.append("];\n")

    lines.append("/// Han unified-ideograph ranges for implicit weighting: `(start, end, core)`")
    lines.append("/// where `core` selects `FB40 +` over `FB80 +` (UTS #10 §10.1.3).")
    lines.append("pub(super) static UNIFIED_IDEOGRAPH: &[(u32, u32, bool)] = &[")
    for lo, hi in han_core:
        lines.append(f"    (0x{lo:06X}, 0x{hi:06X}, true),")
    for lo, hi in HAN_OTHER:
        lines.append(f"    (0x{lo:06X}, 0x{hi:06X}, false),")
    lines.append("];\n")

    # Version-pinned NFD: canonical decompositions and combining classes from the
    # fixed Oracle normalization version (see `NORMALIZATION_DATA`), so a code
    # point assigned after that version stays atomic exactly as Oracle treats it.
    decomp, ccc = parse_unicode_data(DATA / NORMALIZATION_DATA)
    d_cps = sorted(decomp)
    d_blob = []
    d_index = []
    for cp in d_cps:
        d_index.append((cp, len(d_blob), len(decomp[cp])))
        d_blob.extend(decomp[cp])
    lines.append("/// Canonical decomposition blob (NFD part 1), a flat code-point list.")
    lines.append("pub(super) static DECOMP_BLOB: &[u32] = &[")
    for i in range(0, len(d_blob), 8):
        chunk = ", ".join(f"0x{v:X}" for v in d_blob[i : i + 8])
        lines.append(f"    {chunk},")
    lines.append("];\n")
    lines.append("/// Canonical decomposition index `(codepoint, blob_offset, length)`.")
    lines.append("pub(super) static DECOMP: &[(u32, u32, u8)] = &[")
    for cp, off, cnt in d_index:
        lines.append(f"    (0x{cp:06X}, {off}, {cnt}),")
    lines.append("];\n")
    lines.append("/// Non-zero canonical combining classes `(codepoint, class)`.")
    lines.append("pub(super) static CCC: &[(u32, u8)] = &[")
    for cp in sorted(ccc):
        lines.append(f"    (0x{cp:06X}, {ccc[cp]}),")
    lines.append("];\n")

    # A tiny sanity constant so an empty/soonest mismatch is loud.
    lines.append("#[allow(dead_code)]")
    lines.append(f'pub(super) static UCA_VERSION: &str = "{uca}";\n')
    lines.append("pub(super) static TABLE: UcaTable = UcaTable {")
    lines.append("    ces: CES,")
    lines.append("    singles: SINGLES,")
    lines.append("    contractions: CONTRACTIONS,")
    lines.append("    implicit: IMPLICIT,")
    lines.append("    unified_ideograph: UNIFIED_IDEOGRAPH,")
    lines.append("    decomp: DECOMP,")
    lines.append("    decomp_blob: DECOMP_BLOB,")
    lines.append("    ccc: CCC,")
    lines.append("};")

    dst.write_text("\n".join(lines) + "\n")
    _rustfmt(dst)
    print(
        f"emitted {dst.name}: {len(cps)} singles, {len(contra_index)} contractions, "
        f"{len(ce_blob)} CEs, {len(implicit_ranges)} implicit ranges"
    )


def _rustfmt(path):
    """Format `path` in place with rustfmt (edition 2024).

    The generated tables are large; formatting them here keeps `cargo fmt
    --check` green without hand-tuned line wrapping.
    """
    rustfmt = shutil.which("rustfmt")
    if rustfmt is None:
        raise RuntimeError("rustfmt not found; required to format generated tables")
    proc = subprocess.run([rustfmt, "--edition", "2024", str(path)], text=True, capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(f"rustfmt failed on {path}: {proc.stderr.strip()}")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    emit(
        "allkeys-12.1.0.txt",
        OUT / "table_uca1210.rs",
        "12.1.0",
        "Unicode Collation Algorithm 12.1.0 DUCET (Oracle `UCA1210_DUCET`).",
        HAN_CORE_1210,
    )
    emit(
        "allkeys-7.0.0.txt",
        OUT / "table_uca0700.rs",
        "7.0.0",
        "Unicode Collation Algorithm 7.0.0 DUCET (Oracle `UCA0700_DUCET`).",
        HAN_CORE_0700,
    )


if __name__ == "__main__":
    main()
