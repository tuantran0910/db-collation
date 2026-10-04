"""Fail if regenerating the weight tables would change them.

Ensures the committed `crates/db-collation/src/mysql/table_*.rs` match a fresh
run of `harness/gen_weights.py` (i.e. the generator, not hand edits, is the
source of truth). Used by CI and `make check-weights-drift`.

Both sides are rustfmt-formatted before comparison, so the check compares the
**complete generated file** — every token, including lookup code and decimal
literals — not just hex weight literals. A change to `return Some(v)` in the
lookup helper is therefore detected.

    python -m harness.check_weights
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from . import gen_weights

ROOT = Path(__file__).resolve().parent.parent
TABLE_DIR = ROOT / "crates" / "db-collation" / "src" / "mysql"


def _rustfmt(path):
    """Return `path`'s source formatted with rustfmt (edition 2024).

    The source is piped through stdin rather than named on the command line:
    with a path argument, `rustfmt --emit stdout` prepends a `path:` header line
    that would otherwise make every comparison differ.
    """
    rustfmt = shutil.which("rustfmt")
    if rustfmt is None:
        raise RuntimeError("rustfmt not found; required for weight-table drift checking")
    proc = subprocess.run(
        [rustfmt, "--edition", "2024", "--emit", "stdout"],
        input=Path(path).read_text(),
        text=True,
        capture_output=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"rustfmt failed on {path}: {proc.stderr.strip()}")
    return proc.stdout


def _same_source(a, b):
    """True if two Rust sources are identical after rustfmt normalization."""
    return _rustfmt(a) == _rustfmt(b)


def main() -> int:
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        gen_weights.emit(
            gen_weights.DATA / "allkeys-9.0.0.txt",
            tmp / "table_0900.rs",
            "0900",
            "9.0.0",
            "allkeys-9.0.0.txt",
            0x2CEA0,
            None,
        )
        gen_weights.emit(
            gen_weights.DATA / "allkeys-4.0.0.txt",
            tmp / "table_0400.rs",
            "0400",
            "4.0.0",
            "allkeys-4.0.0.txt",
            0xFFFF,
            gen_weights.UCA400_OVERFLOW_PAGE,
        )
        for name in ("table_0900.rs", "table_0400.rs"):
            if not _same_source(TABLE_DIR / name, tmp / name):
                failures.append(name)

    if failures:
        print("weight-table drift detected; run `make gen-weights`:", file=sys.stderr)
        for name in failures:
            print(f"  {name}", file=sys.stderr)
        return 1
    print("weight tables are up to date")
    return 0


if __name__ == "__main__":
    sys.exit(main())
