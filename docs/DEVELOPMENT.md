# Development

## Prerequisites

- Rust **1.85** or newer (edition 2024).
- `rustfmt` and `clippy` components: `rustup component add rustfmt clippy`.
- `cargo-deny` for license/advisory checks (optional): `cargo install cargo-deny`.
- Docker for the differential harness (optional).
- ICU4C development libraries for the `postgres-icu` feature. On macOS:
  `brew install icu4c`. On Debian/Ubuntu: `apt-get install libicu-dev`.

For the PostgreSQL ICU feature, point pkg-config at ICU if it is keg-only:

```sh
export PKG_CONFIG_PATH="$(brew --prefix icu4c)/lib/pkgconfig"
```

## Common tasks

Everything is wrapped in the [`Makefile`](../Makefile):

```sh
make help        # list targets
make fmt         # format
make fmt-check   # check formatting (CI)
make clippy      # lint, all targets and features
make check       # fast type-check, default features
make check-all   # fast type-check, all features
make test        # unit + integration tests
make test-all    # tests with all features
make doc         # build docs (deny warnings)
make doc-open    # build and open docs
make build       # debug build, all features
make build-release
make deny        # license + advisory + ban checks
make msrv        # build with the MSRV toolchain
make ci          # everything CI runs
make clean
make harness     # run the Docker differential matrix (needs Docker)
make gen-weights # regenerate the MySQL weight tables from harness/data
```

## Feature flags

| Feature | Default | Description |
|---|---|---|
| `mysql-uca` | yes | MySQL UCA weight-table backend |
| `postgres-icu` | no | PostgreSQL ICU backend (links ICU4C) |
| `serde` | no | `Serialize`/`Deserialize` for descriptions |

Build and test the full surface with `make check-all` / `make test-all`.

## The differential harness

The harness (`harness/`) is a Python orchestrator that compares the **real
crate** (the `harness-candidate` binary) against live PostgreSQL and MySQL. The
PostgreSQL candidate is built and run inside a candidate image derived `FROM`
the oracle image, so it links the identical ICU data version; MySQL runs the
binary on the host. It is the acceptance criterion for backend changes; see
[`harness/README.md`](../harness/README.md).

It uses its own virtual environment, provisioned from
`harness/requirements.txt`. From the repository root:

```sh
make venv        # create harness/.venv and install its dependencies
make harness     # run the Docker differential matrix
make harness-bmp # full-BMP sweep of every BMP scalar against live MySQL
make deep        # both of the above
```

`make venv` also underpins `make py-test`, which runs the harness's pure unit
tests without Docker. The Docker-backed matrix pulls its images on first run.

The **full-BMP sweep** (`harness/bmp_sweep.py`) is a stronger check than the
scenario corpus: it compares the candidate's total order over all 63,498 BMP
scalar values against live MySQL for each modelled collation. It is not part of
`make ci` (it needs Docker and MySQL); run it via `make harness-bmp`.

## Regenerating weight tables

The generated tables in `crates/db-collation/src/mysql/` are produced from the
Unicode UCA `allkeys` files in `harness/data/`:

```sh
make gen-weights
```

Do not edit the generated files by hand.

## Code style

- No `unsafe` (`unsafe_code = "deny"`).
- `clippy::all`, `pedantic`, `nursery`, and `cargo` lints are enabled workspace-wide.
- Formatting is enforced by `rustfmt` with the repository's `rustfmt.toml`.
- Every public item is documented; `missing_docs` is a warning.

## Commit messages

Conventional Commits are encouraged:

```
feat(mysql): reproduce PAD SPACE padding correctly
fix(postgres): handle empty locale
docs: document the version gate
test: add supplementary default-ignorable cases
```

## Releasing

1. Update `CHANGELOG.md`.
2. Bump `version` in the root `Cargo.toml` `[workspace.package]`.
3. Run `make ci`.
4. `cargo publish -p db-collation`.
5. Tag the release: `git tag -a vX.Y.Z -m "vX.Y.Z"` and push the tag.
