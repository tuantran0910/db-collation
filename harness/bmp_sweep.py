"""Full-BMP direct-operator sweep.

For every BMP scalar value (and the implicit-range endpoints beyond the table),
compare `db-collation`'s total order against live MySQL's, using the same
equivalence-partition + sequence check the differential harness uses. This turns
"exact on the scenario corpus" into "exact on effectively all BMP codepoints".

    python -m harness.bmp_sweep --image mysql:8.4 --collation utf8mb4_0900_ai_ci
    python -m harness.bmp_sweep --image mysql:8.4           # all modelled MySQL collations

The candidate binary must be built first (`make candidate`).
"""

import argparse
import sys
import time
from pathlib import Path

from .candidate import HostCandidate
from .differ import compare_orders
from .oracles import MySqlOracle
from .runner import start_mysql, stop

ROOT = Path(__file__).resolve().parent.parent

# BMP scalars minus surrogates and NUL (NUL is not storable in the harness
# table as a ``VARCHAR`` value). Code points are the scenario ids.
BMP = [cp for cp in range(0x0001, 0x10000) if not (0xD800 <= cp <= 0xDFFF)]

# Beyond-BMP boundary witnesses that exercise the generator/runtime implicit
# paths (CJK Ext-B/E endpoints, Tangut, Hangul syllable range).
EXTRA = [
    0x20000,
    0x2A6D6,
    0x2A700,
    0x2B740,
    0x2B820,
    0x2CEA0,
    0x2CEA1,
    0x2CEA2,
    0x17000,
    0x18AFF,
    0x1F600,
]


def build_corpus():
    from .model import Scenario

    rows = BMP + EXTRA
    return [Scenario(cp, "bmp", chr(cp)) for cp in rows]


def mysql_order(oracle, collation, corpus):
    from .model import Spec

    spec = Spec("bmp-" + collation, "mysql", collation)
    return oracle.order(spec, corpus)


def candidate_order(corpus):
    cand = HostCandidate()
    specs = [
        {"id": "bmp-" + c, "engine": "mysql", "collation": c}
        for c in ("utf8mb4_0900_ai_ci", "utf8mb4_unicode_ci", "utf8mb4_0900_bin")
    ]
    # The candidate emits order/rank keyed by id (codepoint).
    results = cand.run(corpus, specs)
    # Group by collation for easy lookup.
    return {sid[len("bmp-") :]: results[sid] for sid in results}


def run_sweep(image, collations, port=18306, keep=False):
    corpus = build_corpus()
    print(f"BMP sweep: {len(corpus)} code points")

    # Candidate orders (host, ICU-independent).
    t0 = time.time()
    cands = candidate_order(corpus)
    print(f"candidate orders computed in {time.time() - t0:.1f}s")

    name = "harness-bmp"
    start_mysql(image, name, port)
    oracle = None
    try:
        for _ in range(30):
            try:
                oracle = MySqlOracle(port=port)
                break
            except Exception:
                time.sleep(2)
        if oracle is None:
            raise RuntimeError("mysql not reachable")
        oracle.reset_table(corpus)
        failures = []
        for collation in collations:
            print(f"\n=== {collation} ===")
            src = mysql_order(oracle, collation, corpus)
            cand = cands[collation]
            if not cand.supported:
                failures.append(f"{collation}: candidate refused")
                print(f"  refused: {cand.note}")
                continue
            ok, reason, examples = compare_orders(src.order, src.rank, cand.order, cand.rank)
            n = len(corpus)
            classes = len(set(src.rank.values()))
            if ok:
                print(f"  exact: {n} code points, {classes} equivalence classes")
            else:
                failures.append(f"{collation}: {reason}")
                print(f"  MISMATCH: {reason}; e.g. {examples}")
        oracle.close()
    finally:
        if not keep:
            stop(name)

    if failures:
        for f in failures:
            print(f"FAIL: {f}", file=sys.stderr)
        return 1
    print("\nOK: full-BMP order exact for all collations")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", default="mysql:8.4")
    ap.add_argument("--collation", action="append", default=None)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args(argv)
    collations = args.collation or [
        "utf8mb4_0900_ai_ci",
        "utf8mb4_unicode_ci",
        "utf8mb4_0900_bin",
    ]
    return run_sweep(args.image, collations, keep=args.keep)


if __name__ == "__main__":
    sys.exit(main())
