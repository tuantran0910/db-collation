"""Build the differential-harness candidate image for a given oracle image.

Thin CLI over `runner.build_candidate_image` so the tag/name convention lives in
exactly one place (and stays valid for digest-pinned references).

    python -m harness.build_candidate --image postgres:16
"""

import argparse
import sys
from pathlib import Path

from .runner import build_candidate_image


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True, help="oracle image reference (tag or digest)")
    args = ap.parse_args(argv)
    root = Path(__file__).resolve().parent.parent
    tag = build_candidate_image(args.image, root)
    print(f"built candidate image {tag} from {args.image}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
