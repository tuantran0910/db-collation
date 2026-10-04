# db-collation — development tasks
#
# Run `make` or `make help` to list targets.

CARGO ?= cargo
DOCKER ?= docker
IMAGE ?= postgres:16
PKG ?= db-collation
WORKSPACE ?= --workspace
# ICU is keg-only on macOS; expose it to pkg-config for the postgres-icu feature.
ICU_PREFIX ?= $(shell brew --prefix icu4c 2>/dev/null)
ifneq ($(ICU_PREFIX),)
export PKG_CONFIG_PATH := $(ICU_PREFIX)/lib/pkgconfig:$(PKG_CONFIG_PATH)
export DYLD_LIBRARY_PATH := $(ICU_PREFIX)/lib:$(DYLD_LIBRARY_PATH)
endif

PYTHON ?= python3
HARNESS_PY ?= harness/.venv/bin/python
RUFF ?= ruff
RUFF_ARGS ?= --config harness/ruff.toml
DOCKERFMT ?= dockerfmt
HADOLINT ?= hadolint
# All Dockerfiles in the repo.
DOCKERFILES := $(shell find . -iname '*dockerfile*' -not -path './target/*' -not -path './.git/*' 2>/dev/null)

.DEFAULT_GOAL := help
.PHONY: help fmt fmt-check clippy check check-all test test-all doc doc-open \
	build build-release deny msrv ci ci-full check-features audit outdated clean clean-all \
	harness harness-bmp harness-oracle deep gen-weights gen-uca check-weights-drift candidate candidate-image \
	release-check release-notes \
	unit integration bench example venv \
	py-fmt py-fmt-check py-lint py-fix py-test \
	docker-fmt docker-fmt-check docker-lint

help: ## Show this help
	@grep -hE '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

## --- Formatting and linting ------------------------------------------------

fmt: ## Format all code
	$(CARGO) fmt --all

fmt-check: ## Check formatting (CI)
	$(CARGO) fmt --all --check

clippy: ## Lint all targets and features, deny warnings
	$(CARGO) clippy $(WORKSPACE) --all-targets --all-features -- -D warnings

# Isolated feature combinations that must lint on their own. Workspace feature
# unification can otherwise hide dead code or broken cfg that only appears for a
# single feature (e.g. a helper used solely under `oracle-uca`).
check-features: ## Lint each isolated db-collation feature combo, deny warnings
	$(CARGO) clippy -p db-collation --all-targets --no-default-features -- -D warnings
	$(CARGO) clippy -p db-collation --all-targets --no-default-features --features oracle -- -D warnings
	$(CARGO) clippy -p db-collation --all-targets --no-default-features --features oracle-uca -- -D warnings
	$(CARGO) clippy -p db-collation --all-targets --no-default-features --features mysql-uca -- -D warnings
	$(CARGO) clippy -p db-collation --all-targets --no-default-features --features postgres-icu -- -D warnings

## --- Checking and building -------------------------------------------------

check: ## Fast type-check, default features
	$(CARGO) check $(WORKSPACE)

check-all: ## Fast type-check, all features
	$(CARGO) check $(WORKSPACE) --all-features

build: ## Debug build, all features
	$(CARGO) build $(WORKSPACE) --all-features

build-release: ## Release build, all features
	$(CARGO) build $(WORKSPACE) --all-features --release

## --- Testing ---------------------------------------------------------------

test: ## Run unit and integration tests (default features)
	$(CARGO) test $(WORKSPACE)

test-all: ## Run tests with all features
	$(CARGO) test $(WORKSPACE) --all-features

unit: ## Run library unit tests only
	$(CARGO) test -p $(PKG) --lib --all-features

integration: ## Run integration tests only
	$(CARGO) test -p $(PKG) --test integration --all-features

example: ## Build and run examples
	$(CARGO) run -p $(PKG) --example basic_mysql
	$(CARGO) run -p $(PKG) --example postgres_icu --features postgres-icu

bench: ## Run Criterion benchmarks
	$(CARGO) bench -p $(PKG) --all-features

## --- Documentation ---------------------------------------------------------

doc: ## Build documentation, denying warnings
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc $(WORKSPACE) --all-features --no-deps

doc-open: ## Build and open documentation
	$(CARGO) doc $(WORKSPACE) --all-features --no-deps --open

## --- Quality gates ---------------------------------------------------------

deny: ## Check licenses, advisories, and banned dependencies
	$(CARGO) deny check

audit: ## Check for security advisories
	$(CARGO) audit

outdated: ## List outdated dependencies
	$(CARGO) outdated --workspace

msrv: ## Build with the MSRV toolchain (1.85)
	$(CARGO) +1.85 build $(WORKSPACE) --all-features

## --- Python (harness) ------------------------------------------------------

venv: ## Create the harness Python venv and install its dependencies
	$(PYTHON) -m venv harness/.venv
	harness/.venv/bin/python -m pip install --upgrade pip
	harness/.venv/bin/python -m pip install -r harness/requirements.txt

py-fmt: ## Format harness Python
	$(RUFF) format $(RUFF_ARGS) harness

py-fmt-check: ## Check harness Python formatting
	$(RUFF) format --check $(RUFF_ARGS) harness

py-lint: ## Lint harness Python
	$(RUFF) check $(RUFF_ARGS) harness

py-fix: ## Auto-fix harness Python lint issues
	$(RUFF) check --fix $(RUFF_ARGS) harness

py-test: ## Run harness pure unit tests (no Docker)
	$(HARNESS_PY) -m unittest discover -s harness/tests

## --- Dockerfiles -----------------------------------------------------------

docker-fmt: ## Format Dockerfiles
	$(DOCKERFMT) -w $(DOCKERFILES)

docker-fmt-check: ## Check Dockerfile formatting (CI)
	$(DOCKERFMT) --check $(DOCKERFILES)

docker-lint: ## Lint Dockerfiles (CI)
	$(HADOLINT) $(DOCKERFILES)

# What CI runs on every change: formatting, lints, tests, docs, Python, and
# Dockerfile checks. This does NOT run the Docker differential matrix, the
# feature matrix, MSRV, or cargo-deny (CI runs those as separate jobs).
ci: fmt-check clippy check-features test-all doc py-fmt-check py-lint py-test docker-fmt-check docker-lint ## Fast checks CI runs (no Docker/DB)

# The Docker-free portion of everything CI runs: the fast suite plus MSRV,
# cargo-deny, and the weight-table drift check. It deliberately excludes the
# Docker differential harness, the full-BMP sweep, and the isolated feature
# matrix, which need Docker and/or extra toolchains (run `make harness`,
# `make harness-bmp`, and the per-feature checks separately).
ci-full: ci msrv deny check-weights-drift ## Fast suite + MSRV, deny, drift, isolated features (no Docker/DB)

## --- Harness and data ------------------------------------------------------

harness: candidate ## Run the Docker differential matrix (builds the host candidate first)
	$(HARNESS_PY) -m harness.run --engine all --out /private/tmp/harness-matrix.json

harness-bmp: candidate ## Full-BMP direct-operator sweep vs MySQL (MYSQL_IMAGE, default mysql:8.4)
	$(HARNESS_PY) -m harness.bmp_sweep --image $(or $(MYSQL_IMAGE),mysql:8.4)

harness-oracle: candidate ## Differential matrix vs live Oracle Free (needs Docker)
	$(HARNESS_PY) -m harness.run --engine oracle --out /private/tmp/harness-oracle.json

deep: harness harness-bmp harness-oracle ## Full matrix plus BMP and Oracle sweeps (slow; needs Docker)

candidate: ## Build the candidate binary on the host (real crate)
	$(CARGO) build --release -p harness-candidate

candidate-image: ## Build the candidate image for IMAGE (default postgres:16)
	$(HARNESS_PY) -m harness.build_candidate --image $(IMAGE)

gen-weights: ## Regenerate MySQL weight tables from harness/data
	$(HARNESS_PY) harness/gen_weights.py

gen-uca: ## Regenerate Oracle DUCET tables from harness/data
	$(HARNESS_PY) -m harness.gen_uca

check-weights-drift: ## Fail if regenerating the weight tables changes them (CI)
	$(HARNESS_PY) -m harness.check_weights

## --- Releasing -------------------------------------------------------------

release-check: ## Verify the crate packages and builds as it would for release (dry-run)
	$(CARGO) publish -p $(PKG) --all-features --locked --dry-run

release-notes: ## Print the CHANGELOG section for VERSION (VERSION=x.y.z; default: crate version)
	$(PYTHON) tools/release_notes.py CHANGELOG.md $(or $(VERSION),$(shell $(CARGO) metadata --no-deps --format-version 1 | $(PYTHON) -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="db-collation"))'))

## --- Cleanup ---------------------------------------------------------------

clean: ## Remove build artifacts
	$(CARGO) clean

clean-all: clean ## Remove build artifacts and the harness venv/cache
	rm -rf harness/.venv .work
	find harness -type d -name __pycache__ -prune -exec rm -rf {} +
