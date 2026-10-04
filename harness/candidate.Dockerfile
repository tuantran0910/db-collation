# syntax=docker/dockerfile:1
#
# Differential-harness candidate image.
#
# Built multi-stage FROM the exact PostgreSQL oracle image so the candidate
# links the identical ICU data version as the server it is compared against.
# The oracle is never modified: it runs the pristine base image, and this
# derived image runs only the harness candidate. Override PG_IMAGE with the
# digest-pinned reference actually under test.
#
#   docker build -f harness/candidate.Dockerfile \
#     --build-arg PG_IMAGE=postgres:16@sha256:... \
#     -t db-collation-candidate:postgres-16 .
#
# PG_IMAGE is the oracle image (also the runtime base, so the candidate links
# the exact same ICU). BUILDER_IMAGE is where the toolchain is installed; it
# defaults to PG_IMAGE, but an EOL oracle (e.g. postgres:15-bullseye) ships a
# dead apt, so it is overridden with the matching `debian:<suite>`, whose
# libicu package version is identical.
ARG PG_IMAGE=postgres:16
ARG BUILDER_IMAGE
ARG DEBIAN_SUITE
# A Docker-safe key for the target cache. It must not contain characters that
# are invalid in a BuildKit cache-mount id (notably ':'), or distinct images
# collide and reuse each other's ICU-linked binaries.
ARG TARGET_CACHE_ID=default

FROM ${BUILDER_IMAGE:-${PG_IMAGE}} AS builder

# Re-declare the global build args in this stage's scope (a pre-FROM ARG is not
# visible inside a stage unless redeclared).
ARG DEBIAN_SUITE
ARG TARGET_CACHE_ID=default

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

# Package versions are intentionally unpinned: they must match the base image's
# libicu so the candidate links the same collation data as the oracle.
COPY harness/apt-install-build-deps.sh /usr/local/bin/apt-install-build-deps.sh
# hadolint ignore=DL3008
RUN bash /usr/local/bin/apt-install-build-deps.sh "${DEBIAN_SUITE}"

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo
# The workspace pins its toolchain in rust-toolchain.toml; rustup installs it
# on first use.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain none

ENV PATH=/usr/local/cargo/bin:${PATH}
WORKDIR /src
COPY . .
# Cache mounts keep dependency compilation across rebuilds; the binary is copied
# out within the RUN because the cache mount is not part of the stage layer.
# The target cache is keyed by the base image (via TARGET_CACHE_ID, sanitized by
# the caller) so a binary built against one ICU version is never reused for
# another (which would fail to link with a missing libicuuc.so.X).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=target-${TARGET_CACHE_ID},target=/src/target \
    cargo build --release -p harness-candidate \
    && cp /src/target/release/harness-candidate /usr/local/bin/harness-candidate

FROM ${PG_IMAGE} AS runtime

COPY --from=builder /usr/local/bin/harness-candidate /usr/local/bin/harness-candidate
ENTRYPOINT ["/usr/local/bin/harness-candidate"]
