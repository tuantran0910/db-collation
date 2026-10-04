# Contributing

Thanks for your interest in `db-collation`. Bug reports, feature requests, and
pull requests are welcome.

## Before you start

This crate has one non-negotiable property: **exactness**. A change that makes an
unsupported configuration appear supported, or that changes comparison output for
a supported one, is a correctness regression even if tests pass. Read
[`docs/DESIGN_DECISIONS.md`](docs/DESIGN_DECISIONS.md) first.

## Development setup

1. Install Rust 1.85+ with `rustfmt` and `clippy` (pinned by `rust-toolchain.toml`).
2. For the ICU backend, install ICU4C and set `PKG_CONFIG_PATH` if keg-only.
3. `make venv` creates the harness Python venv and installs its dependencies.
4. Run `make ci` to reproduce the full check suite.

See [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) for all targets.

## The golden rule for backend changes

Any change to a backend must keep the differential harness green:

```sh
make harness
```

If you add a configuration, add it to the harness *and* to the supported matrix
in [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md). If you cannot make the
harness prove exactness, the configuration must be refused, not approximated.

## Pull request checklist

- [ ] `make ci` passes (fmt, clippy, tests, docs, deny).
- [ ] New behaviour is covered by tests.
- [ ] `harness` still reports zero mismatches for all supported configurations.
- [ ] Public items are documented.
- [ ] `CHANGELOG.md` has an entry under `[Unreleased]`.
- [ ] No new runtime dependency introduces I/O or a database driver.

## Reporting bugs

Please include:

- the collation description you constructed,
- the two strings and the ordering you got vs expected,
- the source database version and collation metadata
  (`pg_collation` row or `information_schema.COLLATIONS`),
- whether the differential harness reproduces it.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate, as defined in the Apache-2.0 license, shall be
licensed as Apache-2.0, without any additional terms or conditions.
