#!/usr/bin/env python3
# Copyright 2026 db-collation contributors
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Extract one version's section from CHANGELOG.md for release notes.

The changelog follows [Keep a Changelog]: versions are level-2 headings of the
form ``## [X.Y.Z] - YYYY-MM-DD`` (or ``## [Unreleased]``), and the file ends
with a link-reference block. This tool prints the body under the requested
version heading (the heading itself excluded), stopping at the next ``## [``
heading or the link-reference block, prefixed with the version as a top-level
heading so GitHub renders the release body full width.

Usage:
    python tools/release_notes.py CHANGELOG.md 0.1.0

Exits non-zero if the version heading is not found, so the release workflow
fails loudly rather than publishing an empty release body.

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# A level-2 heading whose text is ``[something]`` (optionally ``[x] - date``).
_VERSION_HEADING = re.compile(r"^## \[(?P<label>[^\]]+)\](?P<rest>.*)$")
# The link-reference block: ``[Unreleased]: https://...`` etc. at column 0.
_LINK_REFERENCE = re.compile(r"^\[[^\]]+\]:\s")


def extract(changelog: str, version: str) -> str:
    """Return the body under ``## [<version>]`` with surrounding blanks trimmed.

    Raises ``KeyError`` if the version heading is absent.
    """
    lines = changelog.splitlines()
    start = None
    for i, line in enumerate(lines):
        match = _VERSION_HEADING.match(line)
        if match and match.group("label").strip() == version:
            start = i + 1
            break
    if start is None:
        raise KeyError(version)

    body: list[str] = []
    for line in lines[start:]:
        if _VERSION_HEADING.match(line) or _LINK_REFERENCE.match(line):
            break
        body.append(line)
    return "\n".join(body).strip()


def document(changelog: str, version: str) -> str:
    """Return the release notes as a standalone document.

    GitHub lays out a release body full width only when it begins with a
    top-level heading; a body that starts at ``###`` is rendered as a nested
    fragment and indented. So the extracted section (which starts at ``###``)
    is prefixed with the version as an H1 title.

    Raises ``KeyError`` if the version heading is absent.
    """
    lines = changelog.splitlines()
    title = version
    for line in lines:
        match = _VERSION_HEADING.match(line)
        if match and match.group("label").strip() == version:
            title = (match.group("label").strip() + match.group("rest")).strip()
            break
    body = extract(changelog, version)
    return f"# {title}\n\n{body}"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "changelog",
        nargs="?",
        default="CHANGELOG.md",
        help="path to the changelog (default: CHANGELOG.md)",
    )
    parser.add_argument("version", help="the version label to extract, e.g. 0.1.0")
    args = parser.parse_args(argv)

    text = Path(args.changelog).read_text(encoding="utf-8")
    try:
        notes = document(text, args.version)
    except KeyError:
        print(f"error: no '## [{args.version}]' section in {args.changelog}", file=sys.stderr)
        return 1
    if not extract(text, args.version):
        print(f"error: '## [{args.version}]' section is empty", file=sys.stderr)
        return 1
    print(notes)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
