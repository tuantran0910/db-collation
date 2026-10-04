"""Pure (no-database) unit tests: run with `python -m unittest discover harness/tests`.

These run without Docker or a live database. They cover the corpus generator,
the differ, and the negative controls that prove the differ can actually fail.
"""

import tempfile
import unittest
from pathlib import Path
from typing import ClassVar

from harness.corpus import category_counts, generate
from harness.differ import compare
from harness.model import OrderResult, Outcome, Scenario, Spec
from harness.runner import attach_acceptance, candidate_image_tag, container_name, safe_label


class TestCorpus(unittest.TestCase):
    def test_deterministic(self):
        a = generate(seed=1, scale=0.1)
        b = generate(seed=1, scale=0.1)
        self.assertEqual([(s.cat, s.s) for s in a], [(s.cat, s.s) for s in b])

    def test_classes_present(self):
        cats = category_counts(generate(scale=0.1))
        for c in [
            "empty",
            "space_pad",
            "canon_reorder",
            "ignorable",
            "emoji",
            "supplementary",
            "pad_edge",
            "long_prefix",
            "control",
        ]:
            self.assertIn(c, cats)


class TestBmpSweepCorpus(unittest.TestCase):
    def test_covers_full_bmp_without_surrogates_or_nul(self):
        from harness.bmp_sweep import BMP

        self.assertNotIn(0, BMP)
        self.assertTrue(all(not (0xD800 <= cp <= 0xDFFF) for cp in BMP))
        self.assertIn(0xFFFF, BMP)
        # 0x10000 total minus NUL minus the 2048 surrogates.
        self.assertEqual(len(BMP), 0x10000 - 1 - 0x800)

    def test_boundary_witnesses_present_and_in_range(self):
        from harness.bmp_sweep import EXTRA

        for cp in (0x2CEA0, 0x2CEA1, 0x17000, 0x18AFF):
            self.assertIn(cp, EXTRA)
        self.assertTrue(all(cp > 0xFFFF for cp in EXTRA))


class TestWeightDrift(unittest.TestCase):
    """The drift check must compare the whole generated file, not just hex.

    These are pure (no database) negative controls: the check's comparison of a
    tampered file against a freshly generated one must flag edits to lookup code
    and decimal literals, not only hex weight changes.
    """

    def _sample(self):
        # A minimal, generator-shaped table file exercising every token kind.
        return (
            "// header\n"
            "static TABLE: &[u64] = &[0x1, 0x2];\n"
            "pub(super) fn weight(cp: u32) -> Option<u128> {\n"
            "    let u = *TABLE.get(cp as usize)?;\n"
            "    if u == 0xFFFD {\n"
            "        for &(c, v) in LONG_RUNE_MAP {\n"
            "            if c == cp { return Some(v); }\n"
            "        }\n"
            "        return Some(0xFFFDu128);\n"
            "    }\n"
            "    Some(u.into())\n"
            "}\n"
        )

    def test_identical_content_matches(self):
        from harness.check_weights import _same_source

        with tempfile.TemporaryDirectory() as tmp:
            a = Path(tmp) / "a.rs"
            b = Path(tmp) / "b.rs"
            a.write_text(self._sample())
            b.write_text(self._sample())
            self.assertTrue(_same_source(a, b))

    def test_lookup_code_edit_is_detected(self):
        from harness.check_weights import _same_source

        with tempfile.TemporaryDirectory() as tmp:
            a = Path(tmp) / "a.rs"
            b = Path(tmp) / "b.rs"
            a.write_text(self._sample())
            b.write_text(self._sample().replace("return Some(v);", "return Some(0);", 1))
            self.assertFalse(_same_source(a, b))

    def test_decimal_literal_edit_is_detected(self):
        from harness.check_weights import _same_source

        with tempfile.TemporaryDirectory() as tmp:
            a = Path(tmp) / "a.rs"
            b = Path(tmp) / "b.rs"
            a.write_text(self._sample())
            b.write_text(self._sample().replace("cp as usize", "cp as usize + 1", 1))
            self.assertFalse(_same_source(a, b))


class TestDiffer(unittest.TestCase):
    def setUp(self):
        self.corpus = [Scenario(0, "a", "a"), Scenario(1, "b", "b"), Scenario(2, "a", "a")]

    def test_self_is_ok(self):
        o = OrderResult("x", [0, 1, 2], {0: 1, 1: 2, 2: 1})
        self.assertTrue(compare(o, o, self.corpus).ok)

    def test_reordered_sequence_is_not_ok(self):
        src = OrderResult("x", [0, 1, 2], {0: 1, 1: 2, 2: 1})
        reordered = OrderResult("x", [1, 0, 2], {0: 1, 1: 2, 2: 1})
        self.assertFalse(compare(src, reordered, self.corpus).ok)

    def test_partition_disagreement_is_not_ok(self):
        # Same sequence, but the source groups {0,2} equal while the candidate
        # splits them: a pairwise disagreement the differ must catch.
        src = OrderResult("x", [0, 2, 1], {0: 1, 1: 2, 2: 1})
        cand = OrderResult("x", [0, 2, 1], {0: 1, 1: 3, 2: 2})
        self.assertFalse(compare(src, cand, self.corpus).ok)


class TestNegativeControls(unittest.TestCase):
    """Prove the harness is *measurably* able to fail.

    A differential check that has only ever reported EQUIVALENT is unmeasured.
    These seed known-bad candidate orders/ranks and assert the differ rejects
    them, so a green run on real data carries information.
    """

    def setUp(self):
        # Five strings with two equivalence classes: {a,A} < b < c.
        self.corpus = [
            Scenario(0, "x", "a"),
            Scenario(1, "x", "A"),
            Scenario(2, "x", "b"),
            Scenario(3, "x", "c"),
        ]
        self.correct = OrderResult("x", [0, 1, 2, 3], {0: 1, 1: 1, 2: 2, 3: 3})

    def test_correct_order_passes(self):
        self.assertTrue(compare(self.correct, self.correct, self.corpus).ok)

    def test_swapped_pair_is_caught(self):
        bad = OrderResult("x", [0, 1, 3, 2], {0: 1, 1: 1, 2: 3, 3: 2})
        self.assertFalse(compare(self.correct, bad, self.corpus).ok)

    def test_missing_equivalence_is_caught(self):
        # Candidate treats "A" as distinct from "a".
        bad = OrderResult("x", [0, 1, 2, 3], {0: 1, 1: 2, 2: 3, 3: 4})
        self.assertFalse(compare(self.correct, bad, self.corpus).ok)

    def test_phantom_equivalence_is_caught(self):
        # Candidate treats "b" and "c" as equal.
        bad = OrderResult("x", [0, 1, 2, 3], {0: 1, 1: 1, 2: 2, 3: 2})
        self.assertFalse(compare(self.correct, bad, self.corpus).ok)


class TestAcceptance(unittest.TestCase):
    """Fail-closed acceptance: a run is OK only on explicit, complete success.

    These establish that failures *propagate*, which the differ-only controls
    above do not.
    """

    PG_SPECS: ClassVar[tuple[Spec, ...]] = (
        Spec("pg-a", "postgres", "und", True),
        Spec("pg-b", "postgres", "en", True),
    )

    @staticmethod
    def _report(engine, specs, image="postgres:16"):
        return {"image": image, "engine": engine, "specs": specs, "icu_major": "76"}

    def _accept(self, reports, expected_images=None):
        return attach_acceptance(reports, expected_images, self.PG_SPECS, [])

    def test_complete_explicit_success_is_ok(self):
        rep = self._report(
            "postgres",
            [
                {"id": "pg-a", "collation": "und", "ok": True},
                {"id": "pg-b", "collation": "en", "ok": True},
            ],
        )
        self.assertTrue(self._accept([rep], ["postgres:16"])["ok"])

    def test_source_error_fails(self):
        rep = self._report(
            "postgres",
            [
                {"id": "pg-a", "collation": "und", "ok": True},
                {"id": "pg-b", "collation": "en", "error": "boom"},
            ],
        )
        out = self._accept([rep])
        self.assertFalse(out["ok"])
        self.assertFalse(out["reports"][0]["accepted"])

    def test_unexpected_refusal_ok_none_fails(self):
        rep = self._report(
            "postgres",
            [
                {"id": "pg-a", "collation": "und", "ok": True},
                {"id": "pg-b", "collation": "en", "supported": False, "ok": None},
            ],
        )
        self.assertFalse(self._accept([rep])["ok"])

    def test_missing_spec_result_fails(self):
        rep = self._report("postgres", [{"id": "pg-a", "collation": "und", "ok": True}])
        self.assertFalse(self._accept([rep])["ok"])

    def test_empty_run_fails(self):
        self.assertFalse(self._accept([])["ok"])
        self.assertFalse(self._accept([self._report("postgres", [])])["ok"])

    def test_expected_skip_is_ok(self):
        # A spec the matrix marks unsupported may be skipped by an old server
        # (e.g. ICU `rules` before PG 16) without failing the run.
        specs = [
            Spec("pg-a", "postgres", "und", True),
            Spec("pg-rules", "postgres", "und", False, supported=False),
        ]
        rep = self._report(
            "postgres",
            [
                {"id": "pg-a", "collation": "und", "ok": True},
                {
                    "id": "pg-rules",
                    "collation": "und",
                    "skipped": True,
                    "expected_unsupported": True,
                },
            ],
        )
        out = attach_acceptance([rep], None, specs, [])
        self.assertTrue(out["ok"])

    def test_unexpected_skip_fails(self):
        specs = [Spec("pg-a", "postgres", "und", True)]
        rep = self._report(
            "postgres",
            [{"id": "pg-a", "collation": "und", "skipped": True, "expected_unsupported": False}],
        )
        self.assertFalse(attach_acceptance([rep], None, specs, [])["ok"])

    def test_runtime_error_on_expected_unsupported_spec_fails(self):
        # A spec the matrix expects to be refused, but the candidate reported a
        # *runtime* comparison failure (`outcome="error"`, note "compare failed")
        # must never be accepted as a clean refusal.
        specs = [
            Spec("pg-a", "postgres", "und", True),
            Spec("pg-bad", "postgres", "xx-bad", True, supported=False),
        ]
        rep = self._report(
            "postgres",
            [
                {"id": "pg-a", "collation": "und", "ok": True},
                {
                    "id": "pg-bad",
                    "collation": "xx-bad",
                    "supported": False,
                    "outcome": Outcome.ERROR.value,
                    "ok": False,
                    "expected_unsupported": True,
                    "note": "compare failed: backend runtime error",
                },
            ],
        )
        out = attach_acceptance([rep], None, specs, [])
        self.assertFalse(out["ok"])
        self.assertFalse(out["reports"][0]["accepted"])

    def test_clean_refusal_on_expected_unsupported_spec_is_ok(self):
        specs = [
            Spec("pg-a", "postgres", "und", True),
            Spec("pg-bad", "postgres", "xx-bad", True, supported=False),
        ]
        rep = self._report(
            "postgres",
            [
                {"id": "pg-a", "collation": "und", "ok": True},
                {
                    "id": "pg-bad",
                    "collation": "xx-bad",
                    "supported": False,
                    "outcome": Outcome.REFUSED.value,
                    "ok": True,
                    "expected_unsupported": True,
                    "note": "refused: unsupported",
                },
            ],
        )
        out = attach_acceptance([rep], None, specs, [])
        self.assertTrue(out["ok"])

    def test_missing_expected_image_fails(self):
        rep = self._report(
            "postgres",
            [
                {"id": "pg-a", "collation": "und", "ok": True},
                {"id": "pg-b", "collation": "en", "ok": True},
            ],
            image="postgres:16",
        )
        out = self._accept([rep], ["postgres:16", "postgres:17"])
        self.assertFalse(out["ok"])
        self.assertEqual(out["missing_images"], ["postgres:17"])


class TestImageNaming(unittest.TestCase):
    """Docker tags/names must be valid for tagged and digest-pinned references."""

    def test_tag_reference_is_valid(self):
        tag = candidate_image_tag("postgres:16")
        # No ':' after the tag separator, no '/', no '@'.
        name, _, ref = tag.partition(":")
        self.assertTrue(name)
        self.assertNotIn(":", ref)
        self.assertNotIn("/", ref)
        self.assertNotIn("@", ref)

    def test_digest_reference_is_valid_and_unique(self):
        a = candidate_image_tag("postgres:16@sha256:" + "a" * 64)
        b = candidate_image_tag("postgres:16@sha256:" + "b" * 64)
        for tag in (a, b):
            ref = tag.split(":", 1)[1]
            self.assertNotIn(":", ref)
            self.assertNotIn("/", ref)
            self.assertNotIn("@", ref)
        self.assertNotEqual(a, b)

    def test_distinct_tags_distinct_labels(self):
        self.assertNotEqual(safe_label("postgres:16"), safe_label("postgres:17"))

    def test_container_name_under_limit(self):
        name = container_name("harness-pg", "postgres:16@sha256:" + "c" * 64)
        self.assertLessEqual(len(name), 63)
        self.assertRegex(name, r"^[a-zA-Z0-9][a-zA-Z0-9_.-]*$")

    def test_container_name_unique_per_invocation(self):
        # Two runs against the same image must not share a container name, or
        # concurrent runs would `docker rm -f` each other's containers.
        ref = "postgres:16@sha256:" + "d" * 64
        names = {container_name("harness-pg", ref) for _ in range(20)}
        self.assertEqual(len(names), 20)
        for name in names:
            self.assertLessEqual(len(name), 63)


if __name__ == "__main__":
    unittest.main()
