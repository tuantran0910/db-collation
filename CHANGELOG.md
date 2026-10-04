# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Breaking:** `Collation::compare` and `Collation::equal` now return
  `Result<Ordering>` / `Result<bool>`. A backend failure is surfaced as a typed
  error instead of being silently reported as equality.
- The PostgreSQL ICU version gate is enforced at construction time: the
  caller-supplied `collversion` is checked against the linked ICU's
  `ucol_getVersion`, returning `Error::VersionMismatch` unless the full
  canonical token matches (normalizing only trailing zero components). Empty or
  `unknown` versions are refused with `Error::Unsupported` rather than skipping
  the gate.

### Fixed

- **MySQL UCA multi-element weights were emitted in reverse order.** The source
  tables stored the first collation element in the least significant 16-bit
  chunk; `MySQL`'s `WEIGHT_STRING` uses first-element-first order. This affected
  every multi-element weight (e.g. `½`, `æ`, `㎮`). Found by running the real
  crate — not a Python model — as the differential candidate.
- **MySQL implicit weights beyond the table were wrong.** Code points such as the
  CJK-Ext-E endpoint U+2CEA1 received the generic implicit page instead of the
  CJK page. The generator now inlines the correct MySQL implicit ranges.

### Performance

- MySQL streaming comparison is now **allocation-free**: each character's weight
  elements are emitted directly into a fixed stack buffer instead of a temporary
  `Vec`. A counting-allocator regression test asserts zero allocations for equal
  keys, late differences, and expansions up to 1000 characters.

### Changed

- The MySQL weight tables are now **generated directly from the Unicode UCA
  `allkeys` data** (`harness/gen_weights.py`) rather than transcribed from TiKV,
  removing the TiKV dependency. Exactness is unchanged (the differential harness
  is green on MySQL 8.0/8.4/9.4) and attribution is now Unicode-3.0 only.
- The harness candidate now reports a distinct `outcome` (`ok` / `refused` /
  `error`). A runtime comparison failure is never accepted as an expected
  refusal, and the runner fails it unconditionally.
- `make check-weights-drift` compares the **complete rustfmt-formatted generated
  files**, not just hex weight literals, so edits to lookup code are detected.
- The differential harness candidate container is uniquely named per invocation
  and force-removed in a `finally` block, so a client timeout can no longer leak
  a running container. Oracle container names are likewise unique per run.
- `make ci-full` is documented as the Docker-free superset (fast suite plus MSRV,
  `cargo deny`, and drift), and no longer implies the differential harness is
  included.
- Centrally validated `Collation` deserialization: a serialized description is
  re-run through the same constructors, so unsupported or stale payloads are
  refused instead of bypassing validation.

### Added

- `Collation::from_mysql_name_unspecified_version` accepts `utf8mb4_0900_bin`
  (NO PAD, code-point order == bytewise for UTF-8). The legacy `utf8mb4_bin`
  (PAD SPACE) remains refused.
- `db_collation::collation_version(locale)` exposes the local ICU collation data
  version, formatted as `PostgreSQL`'s `pg_collation_actual_version`.
- `PostgresCollation` carries `rules` and `encoding`; custom-rule and non-UTF-8
  sources are refused at construction.

## [0.1.0] - 2026-10-03

Initial release.

### Added

- `Collation` with a closed set of backends: binary, PostgreSQL ICU, and MySQL
  UCA.
- Bytewise comparison (PostgreSQL `C`/`POSIX`, MySQL `*_bin`).
- MySQL `utf8mb4_0900_ai_ci` (UCA 9.0.0, NO PAD) and `utf8mb4_unicode_ci`
  (UCA 4.0.0, PAD SPACE), with corrected PAD SPACE and default-ignorable
  handling relative to TiKV.
- PostgreSQL ICU backend over ICU4C (`postgres-icu` feature), reproducing
  `varstr_cmp` including the deterministic bytewise tie-break.
- Fail-closed construction: `Error::Unsupported`, `Error::VersionMismatch`,
  `Error::FeatureDisabled`.
- `Send + Sync` collation via per-thread ICU collator cache.
- Differential harness: 110 configurations across PostgreSQL 14–18 (ICU
  67/72/76) and MySQL 8.0/8.4/9.4, 0 mismatches.
- Optional `serde` support for collation descriptions.

[Unreleased]: https://github.com/tuantran0910/db-collation/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tuantran0910/db-collation/releases/tag/v0.1.0
