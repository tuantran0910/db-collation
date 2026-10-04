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
from harness.runner import (
    attach_acceptance,
    candidate_image_tag,
    container_name,
    safe_label,
    supported_refusal_is_a_failure,
)


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


class TestSweepLifecycle(unittest.TestCase):
    """The BMP sweep must own its container and clean it up on every exit path.

    `start_mysql` runs inside the sweep's `try`, so a readiness failure must
    still remove the container this invocation created. The container name must
    also be unique per run, so concurrent sweeps never `docker rm -f` each
    other's.
    """

    @staticmethod
    def _patch(calls, *, raise_on_start):
        from unittest import mock

        def fake_start(image, name, port):
            calls.append(("start", name))
            if raise_on_start:
                raise RuntimeError("mysql did not become ready")

        def fake_stop(name):
            calls.append(("stop", name))

        # A stub oracle so the success path does not wait on a real database.
        oracle = mock.MagicMock()
        oracle.order.return_value = mock.MagicMock(order=[], rank={})

        patches = [
            mock.patch(
                "harness.bmp_sweep.candidate_order",
                return_value={
                    "utf8mb4_0900_ai_ci": mock.MagicMock(supported=False, note="stub"),
                },
            ),
            mock.patch("harness.bmp_sweep.start_mysql", side_effect=fake_start),
            mock.patch("harness.bmp_sweep.stop", side_effect=fake_stop),
            mock.patch("harness.bmp_sweep.MySqlOracle", return_value=oracle),
            mock.patch("harness.bmp_sweep.compare_orders", return_value=(True, "", [])),
        ]
        return patches, oracle

    def _run(self, calls, *, raise_on_start):
        from contextlib import ExitStack

        from harness.bmp_sweep import run_sweep

        patches, _oracle = self._patch(calls, raise_on_start=raise_on_start)
        with ExitStack() as stack:
            for patch in patches:
                stack.enter_context(patch)
            # The stubbed candidate refuses the collation after a successful
            # start; that still exercises the cleanup path.
            if raise_on_start:
                with self.assertRaises(RuntimeError):
                    run_sweep("mysql:8.4", ["utf8mb4_0900_ai_ci"])
            else:
                run_sweep("mysql:8.4", ["utf8mb4_0900_ai_ci"])

    def test_startup_failure_still_removes_container(self):
        calls = []
        self._run(calls, raise_on_start=True)
        # Exactly one start, then exactly one stop for the same (owned) name.
        starts = [n for k, n in calls if k == "start"]
        stops = [n for k, n in calls if k == "stop"]
        self.assertEqual(len(starts), 1)
        self.assertEqual(stops, starts)

    def test_cleanup_happens_after_successful_start(self):
        calls = []
        self._run(calls, raise_on_start=False)
        starts = [n for k, n in calls if k == "start"]
        stops = [n for k, n in calls if k == "stop"]
        self.assertEqual(stops, starts)

    def test_container_name_is_unique_per_invocation(self):
        calls = []
        self._run(calls, raise_on_start=True)
        self._run(calls, raise_on_start=True)
        starts = [n for k, n in calls if k == "start"]
        self.assertEqual(len(starts), 2)
        self.assertNotEqual(starts[0], starts[1])


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

    def _emit_oracle(self, tmp, name, uca, doc, han_core):
        from harness import gen_uca

        dst = Path(tmp) / name
        gen_uca.emit(
            "allkeys-12.1.0.txt" if uca == "12.1.0" else "allkeys-7.0.0.txt",
            dst,
            uca,
            doc,
            han_core,
        )
        return dst

    def test_oracle_generated_tables_match_committed(self):
        # The committed Oracle tables must equal a fresh generation. This is the
        # ongoing regression guard for the Oracle generator, normalization data,
        # implicit ranges and Han intervals.
        from harness import gen_uca
        from harness.check_weights import ORACLE_DIR, _same_source

        with tempfile.TemporaryDirectory() as tmp:
            t1210 = self._emit_oracle(
                tmp,
                "table_uca1210.rs",
                "12.1.0",
                "Unicode Collation Algorithm 12.1.0 DUCET (Oracle `UCA1210_DUCET`).",
                gen_uca.HAN_CORE_1210,
            )
            t0700 = self._emit_oracle(
                tmp,
                "table_uca0700.rs",
                "7.0.0",
                "Unicode Collation Algorithm 7.0.0 DUCET (Oracle `UCA0700_DUCET`).",
                gen_uca.HAN_CORE_0700,
            )
            self.assertTrue(_same_source(ORACLE_DIR / "table_uca1210.rs", t1210))
            self.assertTrue(_same_source(ORACLE_DIR / "table_uca0700.rs", t0700))

    def test_oracle_han_range_edit_is_detected(self):
        # A tampered Han interval must produce a table that differs from the
        # committed one: the drift check must flag it.
        from harness import gen_uca
        from harness.check_weights import ORACLE_DIR, _same_source

        with tempfile.TemporaryDirectory() as tmp:
            extended = [*gen_uca.HAN_CORE_1210, (0x3134B, 0x3134F)]
            tampered = self._emit_oracle(
                tmp,
                "table_uca1210.rs",
                "12.1.0",
                "Unicode Collation Algorithm 12.1.0 DUCET (Oracle `UCA1210_DUCET`).",
                extended,
            )
            self.assertFalse(_same_source(ORACLE_DIR / "table_uca1210.rs", tampered))

    def test_oracle_implicit_range_edit_is_detected(self):
        # Changing the injected implicit ranges must change the generated table.
        from unittest import mock

        from harness import gen_uca
        from harness.check_weights import ORACLE_DIR, _same_source

        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.object(gen_uca, "ORACLE_IMPLICIT_RANGES", [(0x17000, 0x18AFF, 0xFB02)]):
                tampered = self._emit_oracle(
                    tmp,
                    "table_uca1210.rs",
                    "12.1.0",
                    "Unicode Collation Algorithm 12.1.0 DUCET (Oracle `UCA1210_DUCET`).",
                    gen_uca.HAN_CORE_1210,
                )
            self.assertFalse(_same_source(ORACLE_DIR / "table_uca1210.rs", tampered))

    def test_oracle_normalization_data_edit_is_detected(self):
        # Repinning the normalization UnicodeData must change the generated
        # table (the decomposition/combining-class arrays).
        from unittest import mock

        from harness import gen_uca
        from harness.check_weights import ORACLE_DIR, _same_source

        with tempfile.TemporaryDirectory() as tmp:
            with mock.patch.object(gen_uca, "NORMALIZATION_DATA", "UnicodeData-12.1.0.txt"):
                tampered = self._emit_oracle(
                    tmp,
                    "table_uca1210.rs",
                    "12.1.0",
                    "Unicode Collation Algorithm 12.1.0 DUCET (Oracle `UCA1210_DUCET`).",
                    gen_uca.HAN_CORE_1210,
                )
            self.assertFalse(_same_source(ORACLE_DIR / "table_uca1210.rs", tampered))


class TestOracleFallbackContract(unittest.TestCase):
    """A supported configuration must never pass by refusing the whole corpus.

    The runner accepts a candidate refusal only for a spec that is *expected* to
    be unsupported. A supported spec that merely reports a per-input fallback for
    every string must be a failure, otherwise a broken candidate could look green
    without a single comparison.
    """

    def test_supported_spec_refusal_is_a_failure(self):
        self.assertFalse(supported_refusal_is_a_failure(True))

    def test_expected_unsupported_spec_refusal_is_ok(self):
        self.assertTrue(supported_refusal_is_a_failure(False))


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


class TestOracleMatrix(unittest.TestCase):
    """The Oracle matrix must separate modelled collations from refusals.

    Structural only (no database): every supported name is a resolved
    `NLS_SORT` the crate claims exact support for, and every unsupported name is
    one Oracle offers but the crate refuses.
    """

    def test_supported_names_are_the_modelled_set(self):
        from harness import matrix

        self.assertEqual(matrix.ORACLE_SUPPORTED, ["BINARY", "UCA1210_DUCET", "UCA0700_DUCET"])
        ids = {s.id for s in matrix.ORACLE_SPECS if s.supported}
        self.assertEqual(ids, {"or-" + c for c in matrix.ORACLE_SUPPORTED})

    def test_refused_names_cover_each_refusal_family(self):
        from harness import matrix

        refused = set(matrix.ORACLE_UNSUPPORTED)
        for name in ["GERMAN", "GENERIC_M", "BINARY_CI", "UCA1210_ROOT", "UCA1210_ORADUCET"]:
            self.assertIn(name, refused)
        ids = {s.id for s in matrix.ORACLE_SPECS if not s.supported}
        self.assertEqual(ids, {"or-" + c for c in matrix.ORACLE_UNSUPPORTED})

    def test_oracle_image_is_digest_pinned(self):
        from harness import matrix

        self.assertTrue(matrix.ORACLE_IMAGES)
        for image in matrix.ORACLE_IMAGES:
            self.assertIn("@sha256:", image)


if __name__ == "__main__":
    unittest.main()
