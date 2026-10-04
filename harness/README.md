# db-collation differential harness

Test-time only. This harness talks to live PostgreSQL/MySQL **as an oracle** and
compares them against the **real `db-collation` crate** — never a Python
reimplementation. `db-collation` itself remains offline: it never opens a
connection and never issues a query.

## What it does

1. **Scenario-class generators** (`corpus.py`) produce strings tagged by the
   algorithmic dimension they exercise: equality/padding, controls, case, accents,
   canonical reordering, ignorables, ligatures/expansions, contractions, numeric,
   punctuation, scripts, RTL diacritics, emoji/ZWJ/variation selectors,
   supplementary/new code points, long prefixes, and seeded random mixes.
2. **Oracles** (`oracles.py`) load the corpus and return the source's total order
   and equivalence classes via `dense_rank() OVER (ORDER BY s COLLATE ...)`,
   along with the source's collation data version.
3. **Candidate** (`candidate.py`): the real crate, built as the binary in
   `crates/harness-candidate`.
   - PostgreSQL: the binary is built and run **inside a candidate image derived
     `FROM` the same base as the oracle** (see `candidate.Dockerfile`), so it
     links the identical ICU data version. The source's `collversion` is passed
     in, exercising the crate's version gate; the harness asserts the candidate
     reproduces the server's version string.
   - MySQL: the binary runs on the host (MySQL ordering depends on the pinned
     UCA weight tables, not on the host ICU).
   The candidate reports a three-valued **outcome**: `ok`, `refused` (clean
   construction refusal — the *only* acceptable non-support), or `error` (a
   runtime comparison failure, which the runner **always fails**). A runtime
   error is never accepted as an expected refusal.
4. **Differ** (`differ.py`) compares sequence, equivalence partition, and every
   pairwise relation, attributing mismatches to scenario-class pairs.
5. **Negative controls** (`tests/test_pure.py`) seed known-bad orders/ranks and
   assert the differ rejects them, so a green run is *measurably* meaningful.
   This includes control edits to the drift check (lookup code, decimal
   literals) and runtime-error-vs-refusal acceptance.
6. **Weight-table drift** (`check_weights.py`): regenerates the MySQL tables and
   compares the **complete rustfmt-formatted files**, so any change to the
   generated content — weights, lookup code, or literals — is detected.

## Run

```sh
# pure unit tests (no Docker, no database)
python -m unittest discover -s harness/tests -v

# full matrix (builds the candidate image per PG image; Docker must be running)
python -m harness.run --engine all --out report.json

# subsets
python -m harness.run --engine mysql          # host candidate, no image build
python -m harness.run --images postgres:16    # one PG image
python -m harness.run --engine oracle         # host candidate vs live Oracle

# full-BMP direct-operator sweep: every BMP scalar plus implicit-range
# boundary witnesses, checked against live MySQL (`make harness-bmp`)
python -m harness.bmp_sweep --image mysql:8.4
```

The full-BMP sweep (`bmp_sweep.py`) is a stronger check than the scenario corpus:
it compares the candidate's total order over **all 63,498 BMP scalar values**
(surrogates and NUL excluded) against MySQL for each modelled collation, and
reports the equivalence-class count so a collapsed partition cannot pass by
accident. It is intentionally not part of `make ci` (it needs Docker and MySQL);
run `make deep` for the full matrix plus the sweep.

The MySQL candidate binary must be built first (`make candidate`). Run inside a
Python environment that has `psycopg` and `pymysql` (the harness dependencies);
the library never depends on them.

## Matrix (as of 2026-10-04)

| Engine | Images | Configs | Result |
|---|---|---|---|
| PostgreSQL ICU | 14, 15, 15-bookworm (ICU 72), 16, 17, 18 | 13 | **all exact (0 pair disagreements)** |
| MySQL | 8.0, 8.4, 9.4 | 3 modelled | **all exact** |
| MySQL | 8.0, 8.4, 9.4 | full BMP (63,498 scalars) | **all exact** |
| MySQL | same | 7 tailored / other-UCA / `utf8mb4_bin` | **refused (fallback)** |
| Oracle | 23.5 Free (`gvenzl/oracle-free`) | `BINARY`, `UCA1210_DUCET`, `UCA0700_DUCET` | **all exact** |
| Oracle | same | 10 monolingual / `_M` / `BINARY_CI` / `*_ROOT` / tailored | **refused (fallback)** |

The PostgreSQL candidate is built `FROM` each image, so it links that image's
exact ICU data version; the harness asserts the candidate's reported version
equals the server's `pg_collation_actual_version(oid)`. The MySQL and Oracle
candidates run on the host, since their ordering is fixed by pinned weight tables
rather than the host ICU.

The Oracle oracle connects with `python-oracledb` in thin mode and orders the
corpus with `NLSSORT(s, 'NLS_SORT=<name>')`, asserting the candidate reproduces
the UCA version token. Empty strings are excluded (Oracle stores `''` as NULL)
and the `long_prefix` class is excluded: Oracle truncates UCA sort keys past
2000 bytes, which the crate refuses with `Error::InputLimit`. Because the corpus
therefore contains no over-limit input, every *supported* Oracle spec must yield
real comparisons — a candidate that only reports per-input fallbacks is a
failure, not a pass (see `docs/CONFIGURATION.md`).

## Adding a configuration / candidate

- Add a `Spec` in `matrix.py` and the matching entry in `docs/CONFIGURATION.md`.
- Add the collation to the crate (with the differential harness proving
  exactness) or mark it `supported=False` in the matrix; the runner asserts that
  a spec expected to be unsupported is in fact refused by the crate.
- For a new engine/provider, add an oracle adapter (`oracles.py`) and, if the
  engine needs a native library that must match the oracle, a candidate image
  derived from the oracle base (see `candidate.Dockerfile`).

## Data

`data/` holds the Unicode UCA `allkeys` files used to generate the weight
tables; see `data/PROVENANCE.md`.
