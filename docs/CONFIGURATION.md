# Configuration and supported matrix

`db-collation` supports a small, explicitly enumerated set of configurations. A
configuration is supported only if the differential harness has shown an exact
match against the source database. Everything else is refused, and the caller
must fall back to comparing on the source.

## PostgreSQL

A PostgreSQL text comparison depends on the collation's **provider**,
**encoding**, **locale/options**, **determinism**, and **collation-data version**.
The caller reads these from the source catalog (`pg_collation`, `pg_database`)
and supplies them via `PostgresCollation`.

| Field | Source | Notes |
|---|---|---|
| provider | `collprovider` | `i` ICU, `c` libc, `d` default, `b` builtin (PG17+) |
| locale | `colliculocale` / `colllocale` / `collcollate` | column name varies by PG version |
| deterministic | `collisdeterministic` | |
| version | `pg_collation_actual_version(oid)` | compared against the linked ICU |
| encoding | `pg_database.encoding` | only UTF-8 is supported |

### Supported

| Case | Behaviour |
|---|---|
| `provider = 'i'`, UTF-8, deterministic | `ucol_strcollUTF8` + bytewise tie-break |
| `provider = 'i'`, UTF-8, nondeterministic | `ucol_strcollUTF8`, no tie-break |
| `C` / `POSIX` | bytewise |

### Refused

- `provider = 'c'` (libc) with any locale other than `C`/`POSIX`.
- `provider = 'b'` (PG 17 builtin) and `provider = 'd'` (database default) whose
  effective provider is not ICU.
- Custom ICU rules (`collicurules` non-empty) — requires `ucol_openRules`.
- Non-UTF-8 encodings.
- A `collversion` that does not match the linked ICU data.

### Version gate

PostgreSQL records the collation data version in `collversion`, and
`pg_collation_actual_version(oid)` returns the version the server is actually
using. The library exposes this as `VersionId` and uses it as the compatibility
boundary. A change detected at recovery time must invalidate persisted ordering
state rather than silently reuse it.

The library compares the supplied version against the linked ICU's
`ucol_getVersion` at construction time. The comparison is on the full canonical
token (up to four byte components), normalizing only trailing *zero* components:
`153.128` and `153.128.0` are the same data version, but a nonzero component
(`…​.46` vs `…​.999`) is a different identity and is never discarded. On a
mismatch, construction fails with [`Error::VersionMismatch`] and the caller must
compare on the source.

The gate is mandatory: an empty `collversion` or the sentinel `unknown` is
refused (`Error::Unsupported`), because neither can be checked. The gate's
precision is bounded by the ICU the library is linked against; a caller that
cannot pin that ICU build must compare on the source, not rely on this crate.

[`Error::VersionMismatch`]: ../crates/db-collation/src/error.rs

## MySQL

MySQL collations are identified by name, which pins the UCA version, accent and
case sensitivity, tailoring, and padding. There is no runtime version token like
PostgreSQL's `collversion`; compatibility is bound to the pinned weight tables
and the supported server range.

| Collation | UCA | Case | Accent | Padding | Supported |
|---|---|---|---|---|---|
| `utf8mb4_0900_ai_ci` | 9.0.0 | insens. | insens. | NO PAD | yes |
| `utf8mb4_unicode_ci` | 4.0.0 | insens. | insens. | PAD SPACE | yes |
| `utf8mb4_0900_bin` | 9.0.0 | sens. | sens. | NO PAD | yes (binary backend) |
| `utf8mb4_bin` | legacy | sens. | sens. | PAD SPACE | no |
| `utf8mb4_0900_as_ci` | 9.0.0 | insens. | sens. | NO PAD | no |
| `utf8mb4_0900_as_cs` | 9.0.0 | sens. | sens. | NO PAD | no |
| `utf8mb4_unicode_520_ci` | 5.2.0 | insens. | insens. | PAD SPACE | no |
| `utf8mb4_general_ci` | non-UCA | insens. | insens. | PAD SPACE | no |
| `utf8mb4_cs_0900_ai_ci` | 9.0.0 | insens. | insens. | NO PAD | no (tailored) |
| `utf8mb4_zh_0900_as_cs` | 9.0.0 | sens. | sens. | NO PAD | no (tailored) |

Validated on MySQL 8.0, 8.4 and 9.4. MySQL 5.7 is not in the matrix: the
`utf8mb4_0900_*` collations do not exist before 8.0, and 5.7's only modelled
collation (`utf8mb4_unicode_ci`, UCA 4.0.0) is already covered by the 8.0/8.4/9.4
runs. 5.7's other common collation, `utf8mb4_general_ci`, is non-UCA and is
refused.

## Binary

Bytewise comparison is always available and is exact for PostgreSQL `C`/`POSIX`
and for `utf8mb4_0900_bin` (NO PAD, code-point order — identical to bytewise
for valid UTF-8). It is **not** exact for the legacy `utf8mb4_bin`, which is
PAD SPACE; that collation is refused.

## Validated environment

| Engine | Images | ICU |
|---|---|---|
| PostgreSQL | 14, 15, 16, 17, 18 | 76 |
| PostgreSQL | 15-bookworm | 72 |
| MySQL | 8.0, 8.4, 9.4 | — |

Result: 0 mismatches. `postgres:15-bullseye` (ICU 67) is excluded because
Debian 11 is EOL and no longer buildable from live mirrors; see
`harness/matrix.py`. See `harness/README.md`.

## Caller checklist

1. Read the source collation metadata once and persist it with the subscription.
2. Construct the matching `Collation`; treat any `Error` as "use the source".
3. On restart or schema change, re-read the metadata. If it changed, discard
   ordering state rather than reusing it.
