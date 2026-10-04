"""CLI for the db-collation differential harness.

Examples:
    python -m harness.run --engine pg
    python -m harness.run --engine mysql --scale 0.5
    python -m harness.run --engine all --images postgres:16 mysql:8.0
"""

import argparse
import sys
from pathlib import Path

from . import matrix
from .corpus import category_counts, generate
from .runner import run_matrix


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", choices=["pg", "postgres", "mysql", "oracle", "all"], default="all")
    ap.add_argument("--scale", type=float, default=1.0)
    ap.add_argument("--seed", type=int, default=20261003)
    ap.add_argument(
        "--images", nargs="*", default=None, help="override images, e.g. postgres:16 mysql:8.0"
    )
    ap.add_argument("--out", default=None)
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args(argv)

    corpus = generate(seed=args.seed, scale=args.scale)
    print(f"corpus: {len(corpus)} strings, {len(category_counts(corpus))} scenario classes")

    images = args.images or (matrix.PG_IMAGES + matrix.MYSQL_IMAGES + matrix.ORACLE_IMAGES)
    expected_images = [
        img
        for img in images
        if (args.engine in ("pg", "postgres", "all") and img.startswith("postgres"))
        or (args.engine in ("mysql", "all") and img.startswith("mysql"))
        or (args.engine in ("oracle", "all") and img.startswith("gvenzl/oracle"))
    ]
    root = Path(__file__).resolve().parent.parent
    result = run_matrix(
        args.engine,
        images,
        corpus,
        matrix.PG_SPECS,
        matrix.MYSQL_SPECS,
        args.out,
        root,
        expected_images=expected_images,
        keep=args.keep,
        oracle_specs=matrix.ORACLE_SPECS,
    )
    for rep in result["reports"]:
        if rep["engine"] == "postgres":
            _print_pg(rep)
        elif rep["engine"] == "oracle":
            _print_oracle(rep)
        else:
            _print_mysql(rep)

    return _report_verdict(result)


def _report_verdict(result):
    """Print and return a fail-closed exit status for a harness run."""
    if result["empty"]:
        print("\nFAIL: no results produced", file=sys.stderr)
    if result["missing_images"]:
        print(f"\nFAIL: missing images: {result['missing_images']}", file=sys.stderr)
    for rep in result["reports"]:
        for failure in rep.get("acceptance_failures", []):
            print(f"FAIL [{rep['image']}] {failure}", file=sys.stderr)
    if result["ok"]:
        print("\nOK: all expected configurations exact")
        return 0
    return 1


def _print_pg(rep):
    print(f"  ICU major (image): {rep['icu_major']}")
    for s in rep["specs"]:
        if "error" in s:
            print(f"  {s['id']:16} SOURCE ERROR: {s['error']}")
            continue
        if not s.get("supported", True):
            print(f"  {s['id']:16} {s['collation']:20} refused: {s['note']}")
            continue
        d = s["diff"]
        print(
            f"  {s['id']:16} {s['collation']:20} det={s['deterministic']!s:5} "
            f"ok={s['ok']!s:5} ver_ok={s.get('version_ok')!s:5} "
            f"pairs_bad={d['pair_disagreements']:6} "
            f"src_ver={s['source_version']} cand_ver={s['candidate_version']}"
        )
        if not s["ok"]:
            print(f"      cats={d['top_categories']}")
            for ex in d["examples"][:3]:
                print(f"      e.g. {ex}")


def _print_oracle(rep):
    print(f"  server: {rep.get('server_version')}")
    for s in rep["specs"]:
        if "error" in s:
            print(f"  {s['id']:24} SOURCE ERROR: {s['error']}")
            continue
        if not s.get("supported", True):
            tag = "refused (expected)" if s.get("expected_unsupported") else "refused (UNEXPECTED)"
            print(f"  {s['id']:24} {s['collation']:20} ok={s.get('ok')!s:5} {tag}")
            continue
        d = s["diff"]
        print(
            f"  {s['id']:24} {s['collation']:20} ok={s['ok']!s:5} "
            f"pairs_bad={d['pair_disagreements']:6} src_ver={s.get('source_version')} "
            f"cand_ver={s.get('candidate_version')}"
        )
        if not s["ok"]:
            print(f"      cats={d['top_categories']}")
            for ex in d["examples"][:3]:
                print(f"      e.g. {ex}")


def _print_mysql(rep):
    for s in rep["specs"]:
        if "error" in s:
            print(f"  {s['id']:28} SOURCE ERROR: {s['error']}")
            continue
        if not s.get("supported", True):
            tag = "refused (expected)" if s.get("expected_unsupported") else "refused (UNEXPECTED)"
            print(f"  {s['id']:28} {s['collation']:24} ok={s.get('ok')!s:5} {tag}")
            continue
        d = s["diff"]
        print(
            f"  {s['id']:28} {s['collation']:24} ok={s['ok']!s:5} "
            f"pairs_bad={d['pair_disagreements']:6}"
        )
        if not s["ok"]:
            print(f"      cats={d['top_categories']}")
            for ex in d["examples"][:3]:
                print(f"      e.g. {ex}")


if __name__ == "__main__":
    sys.exit(main())
