# Data provenance

`crates/db-collation/src/mysql/table_0400.rs` and `table_0900.rs` are generated
from the **Unicode Collation Algorithm** data files, vendored here:

- UCA 4.0.0: `allkeys-4.0.0.txt` — https://www.unicode.org/Public/UCA/4.0.0/allkeys-4.0.0.txt
- UCA 9.0.0: `allkeys-9.0.0.txt` — https://www.unicode.org/Public/UCA/9.0.0/allkeys.txt
- License: Unicode License v3 (`LICENSE-UNICODE.txt`), Copyright © Unicode, Inc.

`harness/gen_weights.py` parses these files and emits MySQL's comparison
weights:

- each collation element's **primary** weight, zero primaries dropped;
- packed with the first element in the least significant 16-bit chunk;
- expansions longer than four elements move to the `u128` `LONG_RUNE_MAP`;
- code points **absent** from `allkeys` (unassigned) get MySQL's implicit weight:
  - UCA 9.0.0: Tangut page `FB00`, CJK Ext pages `FB80`/`FB40`, else
    `0xFBC0 + (cp >> 15)`;
  - UCA 4.0.0 (BMP only): CJK Ext-A `0x3400..0x4DB5` → page `FB80`, CJK
    `0x4E00..0x9FA5` → page `FB40`, else `0xFBC0 + (cp >> 15)`; over-long
    expansions (e.g. U+FDFA) collapse to the whole-character fallback
    `[0xFBC1, cp]`.
- default-ignorable code points are **not** special-cased in the generator: the
  `allkeys` files already list them with zero primaries (unlike later Unicode
  versions, `0x180F`, `0x2065`, and `0xFFF0..0xFFF8` are not ignorable under the
  pinned data and receive implicit weights).

The rules were **verified against MySQL 8.0, 8.4 and 9.4 `WEIGHT_STRING` and the
differential harness**, not taken from any third-party transcription. They are
UCA/MySQL semantics, not DUCET defaults — so the harness (not a table copy) is
what establishes exactness. A full-BMP sweep (`harness/bmp_sweep.py`,
`make harness-bmp`) checks the candidate's total order over every BMP scalar
against live MySQL; it is what surfaced the U+FDFA packing and unassigned
CJK-compat implicit-page bugs.

The crate applies further corrected comparison *semantics* on top (see
`crates/db-collation/src/mysql.rs` and `docs/ARCHITECTURE.md`): default-ignorable
zero weights, PAD SPACE as drop-zero-then-space-pad, multi-element weights in
MySQL's first-element-first order, and the out-of-table implicit-weight ranges.
