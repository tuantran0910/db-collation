# AGENTS.md

Guidance for AI agents and contributors working in the `db-collation`
repository. Read this before making changes.

## What this project is

`db-collation` is an **offline, exact** string-comparison library. It reproduces
the ordering semantics of an explicitly identified database collation and
version, locally. It is a pure function of a collation description and the input
bytes.

The one rule that outranks everything else:

> **Exact, or refuse.** For a supported configuration, `compare` must match the
> source database's comparison operator exactly. For anything not explicitly
> modelled, construction must return an error so the caller can fall back to
> comparing on the source. Never approximate; never silently accept.

A wrong comparison is worse than an error. If you cannot prove exactness for a
configuration (ideally with the differential harness), it must be refused.

## Hard constraints

- **No I/O.** The library never opens a socket, never holds a database driver,
  and never issues a query. Real-database access exists only in `harness/`, at
  test time. `deny.toml` bans database/TLS crates from the dependency graph.
- **No `unsafe`** (`unsafe_code = "deny"`). If a backend needs FFI, it goes
  through an existing crate (`rust_icu_sys`) behind a feature and is reviewed
  carefully.
- **Exact-or-refused** (above). New supported configurations require harness
  evidence; new unsupported ones must fail closed.
- **Version is part of identity.** Do not treat a collation name or locale alone
  as sufficient. PostgreSQL ICU gates on the `collversion`; MySQL pins UCA weight
  tables.

## Repository layout

```
crates/db-collation/        the library (the product)
  src/spec.rs               public API: Collation, descriptions, dispatch
  src/error.rs              Error / Result
  src/mysql.rs              MySQL UCA backend (+ src/mysql/table_*.rs GENERATED)
  src/postgres.rs           PostgreSQL ICU backend
  src/oracle.rs             Oracle backend (+ src/oracle/uca.rs, table_*.rs GENERATED)
  src/thread_local.rs       per-thread ICU collator cache
  tests/                    integration tests
  examples/                 runnable examples
  benches/                  Criterion benchmarks (harness = false)
crates/harness-candidate/   the candidate under test: drives db-collation over a corpus
harness/                    Python differential harness (oracle + differ; test-time only)
  candidate.py              runs the real crate binary (host and in-container)
  candidate.Dockerfile      multi-stage candidate image derived FROM the oracle image
  run.py / runner.py        orchestration; runner builds the candidate image per PG image
  oracles.py                live PG/MySQL/Oracle adapters (the only DB I/O)
  data/                     Unicode UCA allkeys + UnicodeData files (generation inputs)
  gen_weights.py            regenerates src/mysql/table_*.rs
  gen_uca.py                regenerates src/oracle/table_*.rs
docs/                       architecture, configuration, decisions, development
```

## Commands

Use the `Makefile`; run `make help` for the full list.

```sh
make fmt clippy test-all     # before every commit
make ci                      # fast checks CI runs (fmt, clippy, tests, docs, py, docker)
make ci-full                 # fast suite + MSRV, cargo-deny, drift (no Docker/DB)
make candidate               # build the candidate binary on the host (MySQL/Oracle path)
make harness                 # differential matrix vs live PG/MySQL/Oracle (needs Docker)
make harness-bmp             # full-BMP sweep vs live MySQL (needs Docker)
make harness-oracle          # differential matrix vs live Oracle (needs Docker)
make deep                    # harness + harness-bmp + harness-oracle
make gen-weights             # regenerate MySQL weight tables
make gen-uca                 # regenerate Oracle DUCET tables (or `python -m harness.gen_uca`)
```

Requirements: Rust 1.85+, `rustfmt`, `clippy`, and ICU4C for the `postgres-icu`
feature. On macOS, `make` exports `PKG_CONFIG_PATH` for Homebrew's keg-only
`icu4c`; on Linux install `libicu-dev`.

## The golden rule for backend changes

Any change that can affect comparison output for a supported configuration must
keep the differential harness green:

```sh
make harness
```

If you add a configuration, add it to `harness/matrix.py` **and** to
`docs/CONFIGURATION.md`. If the harness cannot prove exactness, refuse the
configuration (return `Error::Unsupported`) rather than approximating.

The harness compares the **real crate** against live databases — never a
reimplementation. For PostgreSQL the candidate binary is built and run inside a
candidate image derived `FROM` the oracle image, so it links the identical ICU
data version; MySQL and Oracle run the same binary on the host (their ordering
does not depend on the host ICU). The harness also asserts the candidate
reproduces the source's collation data version (`collversion` for PostgreSQL,
the `UCA*` token for Oracle), and includes negative controls so a green run is
measurably meaningful.

The candidate reports a three-valued outcome (`ok` / `refused` / `error`). A
clean construction refusal is the only acceptable non-support; a **runtime
comparison failure is always a defect** and must never be accepted as an expected
refusal. When adding a candidate image, keep the BuildKit target-cache id
Docker-safe (it must not contain `:`), or distinct ICU versions will collide and
reuse each other's linked binaries.

For changes to the MySQL weight generator, also run `make harness-bmp`, which
checks the candidate's total order over every BMP scalar against live MySQL.
For changes to the Oracle generator (`harness/gen_uca.py`) or UCA backend, run
`make harness-oracle`.

## Editing rules

- **Generated files are off-limits.** `crates/db-collation/src/mysql/table_*.rs`
  are generated from `harness/data/` by `harness/gen_weights.py`, and
  `crates/db-collation/src/oracle/table_*.rs` by `harness/gen_uca.py`. Edit the
  generator, not the output; regenerate with `make gen-weights` / `make gen-uca`.
  They are derived from the Unicode UCA `allkeys` and `UnicodeData` files in
  `harness/data/`.
- **Public API changes** follow the Rust API Guidelines: newtypes with private
  fields, `#[non_exhaustive]` on configuration enums, constructors instead of
  struct literals, `Debug` on public types.
- **Keep `compare` the primitive.** Sort-key generation is not part of the stable
  API: the contract is a three-way comparison, and a PostgreSQL deterministic
  collation's bytewise tie-break cannot be reproduced by a single ICU sort key,
  which encodes only the primary ICU comparison.
- **Threading:** `Collation` is `Send + Sync`; ICU `UCollator` handles are
  `!Send + !Sync` and live only in the per-thread cache. Never store or share a
  handle.
- **Errors must be observable.** `Collation::compare`/`equal` return `Result`;
  never swallow a backend error and report `Ordering::Equal`.
- **Test names start with `test_`** (Rust `#[test]` functions and Python
  `unittest` methods alike), e.g. `test_pad_space_ignores_trailing_space`.
- Lints are strict (`clippy::all/pedantic/nursery/cargo`, `-D warnings` in CI).
  Do not silence a lint without a comment explaining why.
- Python in `harness/` is formatted and linted with `ruff` (`make py-lint`).

## Documentation to keep in sync

When behavior changes, update the matching doc in the same change:

- supported matrix / version gates → `docs/CONFIGURATION.md` and `README.md`
- design rationale → `docs/DESIGN_DECISIONS.md`
- backend internals → `docs/ARCHITECTURE.md`
- user-visible changes → `CHANGELOG.md` under `[Unreleased]`

## Known open issues

- The PostgreSQL ICU backend calls ICU4C directly (`rust_icu_sys`) for
  `ucol_getVersion`/`u_getVersion`, which `rust_icu_ucol` does not expose. This
  is the crate's only `unsafe`; it is scoped to `src/postgres.rs` behind
  `#[allow(unsafe_code)]` with per-call safety comments. Review any change there
  with extra care.
- The PostgreSQL ICU version gate matches the source's full canonical
  `collversion` token (the `u_versionToString` form, e.g. `153.128.46`),
  normalizing only trailing zeros. There is no bypass: a source whose
  `collversion` is the empty string or the sentinel `unknown` is refused with
  `Error::Unsupported`, and a mismatch is refused. Only runs where the candidate
  and source are built against the identical ICU data version can be exact.

## Releasing

Releases are tag-triggered and automated by `.github/workflows/release.yml`.
Bump `version` in the root `Cargo.toml`, finalise `CHANGELOG.md` (move
`[Unreleased]` under a dated heading), merge to `main`, wait for `ci-success`,
then push a `vX.Y.Z` tag. The workflow verifies the tag matches the manifest and
that CI passed, runs `cargo publish --dry-run`, publishes via crates.io Trusted
Publishing (OIDC, no stored token), and creates a GitHub Release from the
changelog. See `docs/DEVELOPMENT.md`. Preview locally with `make release-check`
and `make release-notes`.

Pick the version with SemVer: while the crate is pre-1.0, a breaking API change
bumps the **minor** (e.g. `0.1.0` → `0.2.0`), and additions/fixes bump the
patch. The tag must equal the manifest version exactly.

Trusted Publishing is a one-time prerequisite: the crate must be published
manually once, and the crates.io Trusted Publisher (repository + workflow file)
must be configured before the OIDC publish step will succeed.

## Commits and PRs

- Conventional Commits, e.g. `fix(mysql): reproduce PAD SPACE padding`,
  `feat(postgres): enforce collversion gate`.
- Only commit when explicitly asked.
- A PR is ready when `make ci` passes and the harness is green for all supported
  configurations.
- Never commit secrets, `.work/`, `target/`, or generated caches.
