"""Shared data types for the db-collation differential harness."""

from dataclasses import dataclass, field
from enum import StrEnum


class Outcome(StrEnum):
    """How the candidate resolved a spec.

    Distinct from a plain bool so the harness can tell a clean construction
    *refusal* (expected for an unsupported configuration) from a runtime
    comparison *error* (always a defect, never an accepted refusal).
    """

    OK = "ok"
    REFUSED = "refused"
    ERROR = "error"


@dataclass
class Scenario:
    id: int
    cat: str
    s: str


@dataclass
class Spec:
    """A single collation configuration to validate."""

    id: str
    engine: str  # 'postgres' | 'mysql'
    collation: str  # PG ICU locale / MySQL collation name
    deterministic: bool = True  # PostgreSQL only
    supported: bool = True  # whether the candidate claims exact support
    rules: str | None = None  # PostgreSQL custom ICU rules (collicurules)
    encoding: str = "UTF8"  # source encoding
    options: dict = field(default_factory=dict)


@dataclass
class OrderResult:
    """Source or candidate total order over the corpus."""

    spec_id: str
    order: list  # list[int] ids in collation order
    rank: dict  # {id: equivalence-class rank}
    version: str | None = None  # source collation data version (PG: collversion)
    supported: bool = True
    note: str = ""
    # Candidate resolution. `None` for a source oracle result, which has no such
    # notion.
    outcome: Outcome | None = None


@dataclass
class DiffResult:
    spec_id: str
    seq_ok: bool
    partition_ok: bool
    pair_disagreements: int
    pairs_checked: int
    top_categories: list
    examples: list

    @property
    def ok(self) -> bool:
        return self.seq_ok and self.partition_ok and self.pair_disagreements == 0
