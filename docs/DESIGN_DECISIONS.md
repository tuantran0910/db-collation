# Design decisions

Short records of the decisions behind the API and behaviour, with the reasoning.

## D1 — Local-only, pure library

**Decision:** `db-collation` performs no I/O and does not depend on any database
driver.

**Why:** the point of the library is to remove per-comparison round trips. If it
could query the source, it would not solve the problem and would reintroduce
latency, retries, and timeouts. Keeping it pure also makes it trivially testable
and safe to embed anywhere, including latency-sensitive callers.

**Consequence:** metadata discovery is the caller's job. Construction takes a
fully resolved description.

## D2 — Exact or refused, never approximate

**Decision:** unsupported configurations return an error; the crate never maps an
unknown collation onto a "close" one.

**Why:** selecting the same locale name with a different ICU version reversed
`あ` and `U+11DB0`. An approximation produces silently wrong ordering, which is
far harder to detect downstream than an explicit refusal. A wrong answer is
worse than a slow one.

**Consequence:** the supported matrix is deliberately small. Growing it is a
matter of adding a verified backend, not of loosening validation.

## D3 — `compare` is the primitive; no stable sort keys

**Decision:** the public API exposes `compare(&str, &str) -> Result<Ordering>`.
Sort-key generation is not part of the stable surface.

`compare` returns `Result` so that a backend failure (e.g. an ICU error) is
surfaced to the caller instead of being silently reported as equality. For the
pure binary and UCA-table backends the result is always `Ok`.

**Why:** PostgreSQL's deterministic collations break ICU equality ties with a
bytewise comparison. A deterministic collation's order is therefore a tuple
(ICU comparison result, then the original bytes); it is not captured by a single
opaque byte-string key of the kind callers expect from `strxfrm`, because the
tie-break is defined on the *values*, not on their transformed weights.
Exposing a naive "sort key" would invite callers to compare keys instead of
values and get wrong results. (An ordered tuple `(full ICU key, original bytes)`
could represent it, and PostgreSQL's `pg_strxfrm` omits the tie-break entirely —
but neither is a single stable key, so the stable API keeps `compare` as the
primitive.)

## D4 — Version is part of the collation identity

**Decision:** PostgreSQL descriptions carry a `VersionId` (`collversion`); MySQL
descriptions carry a pinned UCA version via the weight tables.

**Why:** the same locale with different collation data is a different order.
Version is not a runtime detail to be checked "if convenient"; it is part of what
the collation *is*.

**Consequence:** a version change detected at recovery must invalidate persisted
ordering state.

## D5 — ICU handles are cached per thread, not stored

**Decision:** `Collation` stores configuration only. ICU4C `UCollator` handles
live in a thread-local cache.

**Why:** `UCollator` is `!Send + !Sync` and ICU objects are not internally
synchronized. Storing one in a `Send + Sync` value would be unsound or would
require serializing all comparisons behind a mutex.

**Consequence:** `Collation: Send + Sync` (compile-time asserted), and comparison
is contention-free across threads.

## D6 — Generate the weight tables from the Unicode UCA data

**Decision:** the MySQL backend's weight tables are generated directly from the
Unicode UCA `allkeys` files by `harness/gen_weights.py`, not transcribed from a
third party.

**Why:** the source of truth for UCA weights is the Unicode data, and MySQL's
comparison behavior is a well-defined transform of it (primary weights only,
MySQL element order, MySQL's implicit ranges and PAD SPACE). Generating from
`allkeys` keeps a single, licensed origin and removes a dependency on any
intermediate project's transcription. The differential harness — not a table
copy — is what proves exactness against MySQL 8.0/8.4/9.4.

## D7 — `#[non_exhaustive]` and closed enums

**Decision:** configuration types are `#[non_exhaustive]`; backends are a closed
enum.

**Why:** new backends and fields should be additive without breaking callers,
while construction remains exhaustive over what is actually supported. A caller
cannot invent a backend this crate cannot guarantee.

## D8 — Differential testing is the acceptance criterion

**Decision:** a change to a backend is accepted only if the differential harness
still reports zero mismatches for every supported configuration.

**Why:** finite corpora and single-locale spot checks are insufficient proof.
The harness enumerates scenario *classes* and checks every pairwise relation
against a live source, over multiple versions.
