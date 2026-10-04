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

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from release_notes import extract

CHANGELOG = """\
# Changelog

Intro paragraph.

## [Unreleased]

### Added

- A new thing.

## [0.2.0] - 2026-10-04

### Fixed

- A bug.

## [0.1.0] - 2026-10-03

Initial release.

[Unreleased]: https://example.com/compare/v0.2.0...HEAD
[0.2.0]: https://example.com/releases/tag/v0.2.0
[0.1.0]: https://example.com/releases/tag/v0.1.0
"""


class TestExtract(unittest.TestCase):
    def test_extracts_only_the_requested_section(self):
        notes = extract(CHANGELOG, "0.2.0")
        self.assertIn("A bug.", notes)
        self.assertNotIn("A new thing.", notes)
        self.assertNotIn("Initial release.", notes)
        # The heading itself and the link-reference block are excluded.
        self.assertNotIn("## [0.2.0]", notes)
        self.assertNotIn("https://example.com", notes)

    def test_section_stops_at_next_version_heading(self):
        notes = extract(CHANGELOG, "0.1.0")
        self.assertEqual(notes, "Initial release.")

    def test_unreleased_section_is_addressable(self):
        notes = extract(CHANGELOG, "Unreleased")
        self.assertIn("A new thing.", notes)

    def test_missing_version_raises(self):
        with self.assertRaises(KeyError):
            extract(CHANGELOG, "9.9.9")

    def test_heading_labels_are_not_confused_by_prefix(self):
        # A label that merely starts with the query must not match.
        with self.assertRaises(KeyError):
            extract(CHANGELOG, "0.2")


if __name__ == "__main__":
    unittest.main()
