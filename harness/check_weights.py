"""Fail if regenerating the weight tables would change them.

Ensures the committed `crates/db-collation/src/mysql/table_*.rs` match a fresh
run of `harness/gen_weights.py` (i.e. the generator, not hand edits, is the
source of truth). Used by CI and `make check-weights-drift`.

Both sides are rustfmt-formatted before comparison, so the check compares the
**complete generated file** — every token, including lookup code and decimal
literals — not just hex weight literals. A change to `return Some(v)` in the
lookup helper is therefore detected.

The Oracle generator is checked the same way, covering both `table_uca*.rs` and
their vendored normalization data (decomposition/combining-class tables), the
implicit ranges and the Han intervals: editing any of those changes the output.

    python -m harness.check_weights
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from . import gen_uca, gen_weights

ROOT = Path(__file__).resolve().parent.parent
MYSQL_DIR = ROOT / "crates" / "db-collation" / "src" / "mysql"
ORACLE_DIR = ROOT / "crates" / "db-collation" / "src" / "oracle"


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
            if not _same_source(MYSQL_DIR / name, tmp / name):
                failures.append(name)

        # Oracle DUCET tables. `emit` formats in place; compare the full source.
        gen_uca.emit(
            "allkeys-12.1.0.txt",
            tmp / "table_uca1210.rs",
            "12.1.0",
            "Unicode Collation Algorithm 12.1.0 DUCET (Oracle `UCA1210_DUCET`).",
            gen_uca.HAN_CORE_1210,
        )
        gen_uca.emit(
            "allkeys-7.0.0.txt",
            tmp / "table_uca0700.rs",
            "7.0.0",
            "Unicode Collation Algorithm 7.0.0 DUCET (Oracle `UCA0700_DUCET`).",
            gen_uca.HAN_CORE_0700,
        )
        for name in ("table_uca1210.rs", "table_uca0700.rs"):
            if not _same_source(ORACLE_DIR / name, tmp / name):
                failures.append(name)

    if failures:
        print(
            "weight-table drift detected; run `make gen-weights` and `make gen-uca`:",
            file=sys.stderr,
        )
        for name in failures:
            print(f"  {name}", file=sys.stderr)
        return 1
    print("weight tables are up to date")
    return 0


if __name__ == "__main__":
    sys.exit(main())
