# Security Policy

`db-collation` is an offline, local-only library. It does not open sockets,
hold database drivers, or perform I/O at runtime; the only external code it calls
is ICU4C through the `postgres-icu` feature. Most "security" concerns for this
crate are really **correctness** concerns, so this document covers both.

## Reporting a vulnerability

Report suspected vulnerabilities or correctness bugs privately, not in a public
issue. Use GitHub's private vulnerability reporting on the repository
(Security → Report a vulnerability) or email the maintainers listed in
`Cargo.toml`.

Please include:

- the affected version and the collation configuration (engine, locale/name,
  provider, determinism, and version token),
- the two inputs and the expected vs. observed comparison result,
- the source database build you are comparing against, if applicable.

We aim to acknowledge reports within a few business days.

## What counts as a vulnerability

- **A wrong comparison for a supported configuration.** This is the most serious
  class: `compare` must match the source database exactly. A silent mismatch is a
  correctness bug with the same impact as a memory-safety bug in a sort.
- **A configuration reported as supported when it is not modelled exactly.**
  Construction must fail closed; accepting an unmodelled configuration is a
  correctness regression.
- **A panic, unbounded allocation, or hang reachable from `compare`/`equal` or
  from construction inputs.**
- **Memory-safety issues** in the ICU FFI shim
  (`crates/db-collation/src/postgres.rs`), the only `unsafe` code in the crate.

## What does *not* count

- A mismatch caused by a caller-supplied version token that does not match the
  source. The version is part of the collation's identity; supply the value from
  `pg_collation_actual_version(oid)` (PostgreSQL) or the matching MySQL name.
  Passing the sentinel `unknown` deliberately weakens the guarantee.
- Differences against a database build the harness does not cover (see the
  supported matrix in `docs/CONFIGURATION.md`).

## Supported versions

The crate is pre-1.0; security and correctness fixes are applied to the latest
`0.1.x` release. The supported engine/collation matrix is documented in
[`docs/CONFIGURATION.md`](docs/CONFIGURATION.md).

## Design guarantees that reduce risk

- `#![deny(unsafe_code)]` crate-wide, with a single reviewed, documented
  `#[allow(unsafe_code)]` scope for the ICU version queries.
- No I/O: the dependency graph is checked by `cargo deny` (`deny.toml`), which
  bans database, HTTP, and TLS crates.
- Exact-or-refuse: unsupported configurations return a typed `Error` so callers
  fall back to comparing on the source rather than trusting an approximation.
