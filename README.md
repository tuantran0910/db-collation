# db-collation

[![CI](https://github.com/tuantran0910/db-collation/actions/workflows/ci.yml/badge.svg)](https://github.com/tuantran0910/db-collation/actions/workflows/ci.yml)
[![docs.rs](https://docs.rs/db-collation/badge.svg)](https://docs.rs/db-collation)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](#license)

**Offline, exact comparison of strings using an explicitly identified database
collation and version.**

`db-collation` reproduces the *exact* ordering semantics of a source database's
collation **locally**. It is a pure function of a collation description and the
input bytes. It opens no sockets, holds no database drivers, and never issues a
query.

## Why this exists

Many applications need to order strings the same way a database does, but
locally — for sorting cached values, comparing keys, or checking ranges —
without a round trip to that database. The obvious approaches are not safe. A
column may use a collated text type whose ordering differs from Rust's default
bytewise comparison *and* from a generic Unicode collator, so results computed
locally can silently disagree with the source.

Approximations are not enough:

| Same locale, different ICU | Result |
|---|---|
| PostgreSQL ICU 76 vs local ICU 78 | 4,704 mismatches / 64,516 ordered pairs, in each of 8 configurations |
| MySQL `utf8mb4_0900_ai_ci` vs generic ICU | 928 mismatches / 8,257 sampled pairs |
| MySQL `utf8mb4_unicode_ci` vs generic ICU | 1,664 mismatches / 8,257 sampled pairs |

Even the same locale name with a different ICU data version is unsafe: PostgreSQL
placed `あ` before `U+11DB0`, while a newer ICU reversed it. Selecting the same
locale name does **not** establish compatibility.

## Design: exact or refused

This crate has one rule: **reproduce a configuration exactly, or refuse it.**

- For a supported configuration, `Collation::compare` matches the source's
  comparison operator exactly (validated by the differential harness in
  [`harness/`](harness/)).
- Anything not explicitly modelled returns `Error::Unsupported` (or
  `Error::VersionMismatch` / `Error::FeatureDisabled`). The caller falls back to
  comparing on the source database.
- Version is part of the identity. PostgreSQL's ICU collation is gated on the
  `collversion` reported by the catalog; MySQL collations are keyed by pinned
  UCA weight tables.

## Supported configurations

| Engine | Configuration | Notes |
|---|---|---|
| Any | binary / `C` / `POSIX` | bytewise; always available |
| PostgreSQL | ICU provider, UTF-8, deterministic | `ucol_strcollUTF8` + bytewise tie-break |
| PostgreSQL | ICU provider, UTF-8, nondeterministic | no tie-break |
| MySQL | `utf8mb4_0900_ai_ci` | UCA 9.0.0, NO PAD |
| MySQL | `utf8mb4_unicode_ci` | UCA 4.0.0, PAD SPACE |
| MySQL | `utf8mb4_0900_bin` | NO PAD, code-point order (equals bytewise for UTF-8) |
| Oracle | `NLS_SORT=BINARY` | bytewise on `AL32UTF8` `VARCHAR2` |
| Oracle | `UCA1210_DUCET` / `UCA0700_DUCET` | open DUCET tables, Oracle defaults (`AL32UTF8` `VARCHAR2`) |

Everything else is refused: PostgreSQL libc/builtin providers and custom
`collicurules`, MySQL language-tailored `*_0900_*` collations,
`utf8mb4_general_ci`, `utf8mb4_bin` (PAD SPACE), other UCA versions, non-UTF-8
encodings, and Oracle monolingual/`_M`/`BINARY_CI`/`BINARY_AI`/`*_ROOT`/
`*_ORADUCET`/tailored collations, non-`AL32UTF8` charsets, and blank-padded
`CHAR`/`NCHAR`.

## Installation

```toml
[dependencies]
db-collation = "0.2"
```

Default features enable the MySQL UCA backend. The PostgreSQL ICU backend links
ICU4C and is opt-in; the Oracle UCA backend (11 MB of generated DUCET tables) is
also opt-in:

```toml
[dependencies]
db-collation = { version = "0.2", features = ["postgres-icu", "serde"] }
# Oracle: bytewise `BINARY` only.
db-collation = { version = "0.2", features = ["oracle"] }
# Oracle: adds `UCA1210_DUCET` / `UCA0700_DUCET`.
db-collation = { version = "0.2", features = ["oracle-uca"] }
```

## Usage

```rust
use db_collation::{Collation, Result};
use std::cmp::Ordering;

fn main() -> Result<()> {
    let c = Collation::mysql_0900_ai_ci();
    assert_eq!(c.compare("a", "A")?, Ordering::Equal);
    assert!(c.equal("e", "\u{e9}")?);

    // Unsupported configurations fail closed so the caller can fall back.
    assert!(Collation::from_mysql_name_unspecified_version("utf8mb4_general_ci").is_err());
    Ok(())
}
```

`compare` and `equal` return `Result`: a backend failure is surfaced, never
silently reported as equality.

PostgreSQL ICU, with the source version supplied by the caller:

```rust
use db_collation::{Collation, VersionId};

let c = Collation::postgres_icu("und", /* deterministic = */ true, VersionId::new("153.128"))?;
let _ = c.compare("abc", "abd")?;
# Ok::<(), db_collation::Error>(())
```

The caller is responsible for reading the source catalog (provider, locale,
determinism, `collversion`) and passing it in. See
[`docs/CONFIGURATION.md`](docs/CONFIGURATION.md).

Oracle, resolved from the source's `NLS_SORT` and character set:

```rust
use db_collation::{Collation, OracleCollation, OracleUcaVersion};

// `NLS_SORT=BINARY` and the two open DUCET collations are supported.
let bin = Collation::oracle(OracleCollation::from_nls_sort("BINARY", "AL32UTF8"))?;
let uca = Collation::oracle(OracleCollation::uca_ducet(OracleUcaVersion::Uca1210))?;
let _ = (bin.compare("a", "A")?, uca.compare("strasse", "straße")?);

// Everything else is refused, so the caller falls back to the source.
assert!(Collation::oracle(OracleCollation::from_nls_sort("GERMAN", "AL32UTF8")).is_err());
# Ok::<(), db_collation::Error>(())
```

## Thread safety

`Collation` is `Send + Sync`. ICU4C collator handles are neither, so they are
cached per thread; the handle is never shared across threads.

## Testing

Correctness is established by a differential harness that compares **this
crate** (not a reimplementation) against live PostgreSQL, MySQL, and Oracle:

- **0 mismatches** across PostgreSQL 14–18 (ICU 72/76) and MySQL 8.0/8.4/9.4,
  over a 1,414-string scenario-class corpus.
- **0 mismatches** against Oracle 23.5 Free for `BINARY` and both `UCA*_DUCET`
  collations; every non-modelled `NLS_SORT` is asserted refused.
- **Exact over the full BMP**: the candidate's total order matches live MySQL for
  every one of the 63,498 BMP scalar values, for each modelled collation
  (`make harness-bmp`).
- The PostgreSQL candidate is built and run **inside a candidate image derived
  from the oracle image**, so it links the identical ICU data version. MySQL and
  Oracle candidates run on the host (their ordering does not depend on host ICU).
- Unsupported configurations are asserted to be refused, and negative controls
  prove the differ can fail. A runtime comparison failure is never accepted as a
  refusal.

See [`harness/README.md`](harness/README.md).

```sh
make test          # unit + integration tests
make test-all      # all features
make harness       # Docker differential matrix (PostgreSQL + MySQL + Oracle)
make harness-bmp   # full-BMP sweep vs live MySQL
make harness-oracle # Oracle differential matrix (needs Docker)
```

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — how the backends work.
- [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md) — supported matrix and version gates.
- [`docs/DESIGN_DECISIONS.md`](docs/DESIGN_DECISIONS.md) — why the API looks like this.
- [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) — building, testing, tooling.
- [`SECURITY.md`](SECURITY.md) — reporting vulnerabilities and correctness bugs.
- [`CHANGELOG.md`](CHANGELOG.md)

## Minimum Supported Rust Version

Rust **1.85** (edition 2024). MSRV increases are treated as minor-version bumps.

## License

Licensed under the Apache License, Version 2.0. The MySQL weight tables are
generated from the Unicode UCA data (Unicode License v3). See
[`harness/data/PROVENANCE.md`](harness/data/PROVENANCE.md) and
[`THIRD-PARTY-NOTICES`](crates/db-collation/THIRD-PARTY-NOTICES).

## Acknowledgments

Thanks to the projects and data this crate builds on:

- **[TiKV](https://github.com/tikv/tikv)** — while the weight tables are now
  generated directly from the Unicode UCA data, TiKV's `utf8mb4` collation
  implementation was an invaluable reference for reproducing MySQL's semantics
  (implicit-weight ranges, expansion handling, and PAD SPACE behaviour). Thank
  you to the TiKV authors.
- **[Unicode Consortium](https://www.unicode.org/)** — for the Collation
  Algorithm and the `allkeys` data files.
- **[ICU](https://icu.unicode.org/)** — the PostgreSQL backend links ICU4C via
  [`rust_icu`](https://github.com/google/rust_icu).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Unless you explicitly state otherwise,
any contribution intentionally submitted for inclusion in this crate is
licensed as above, without additional terms.
