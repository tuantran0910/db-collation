"""Docker orchestration + matrix execution for the differential harness.

The oracle is a pristine, digest-pinnable database image. The candidate is the
real `db-collation` crate (see `candidate.py`), never a Python reimplementation.
"""

import dataclasses
import hashlib
import json
import re
import subprocess
import time
import uuid
from pathlib import Path

from .candidate import ContainerCandidate, HostCandidate
from .differ import compare
from .model import Outcome
from .oracles import MySqlOracle, PostgresOracle, SourceUnsupported

HARNESS_DIR = Path(__file__).resolve().parent


def sh(cmd, **kw):
    return subprocess.run(cmd, text=True, capture_output=True, **kw)


def image_present(image):
    return sh(["docker", "image", "inspect", image]).returncode == 0


def image_icu_major(image):
    r = sh(
        [
            "docker",
            "run",
            "--rm",
            image,
            "sh",
            "-c",
            "ls /usr/lib/*/libicui18n.so.* 2>/dev/null || ls /usr/lib/*/libicui18n.a 2>/dev/null",
        ]
    )
    m = re.search(r"libicui18n\.so\.(\d+)", r.stdout)
    return m.group(1) if m else None


def safe_label(reference):
    """A Docker-safe label for an image reference (tag or digest).

    Image references contain `:`, `/`, and `@`, none of which are valid in a
    Docker tag. Collapse them, keep a short digest/version suffix for
    uniqueness, and hash the full reference so distinct digests never collide.
    """
    digest = hashlib.sha256(reference.encode()).hexdigest()[:12]
    readable = re.sub(r"[^a-z0-9]+", "-", reference.lower()).strip("-")[:40]
    readable = readable.strip("-") or "image"
    return f"{readable}-{digest}"


def candidate_image_tag(image):
    return "db-collation-candidate:" + safe_label(image)


def run_suffix():
    """A short, per-process suffix so concurrent harness runs never share a
    fixed container name (and therefore never `docker rm -f` each other's)."""
    return uuid.uuid4().hex[:8]


def container_name(prefix, image, suffix=None):
    suffix = suffix if suffix is not None else run_suffix()
    # Reserve room for the suffix, then trim the image label to fit 63 chars.
    tail = f"-{suffix}"
    head = f"{prefix}-{safe_label(image)}"[: 63 - len(tail)].rstrip("-")
    return f"{head}{tail}"


def build_candidate_image(image, root):
    """Build the candidate image derived FROM `image` (same ICU as the oracle).

    For an EOL oracle whose apt is retired, the builder stage uses the matching
    `debian:<suite>` image (same libicu); the runtime stage is still `image`.
    """
    from .matrix import PG_BUILDER_IMAGE

    tag = candidate_image_tag(image)
    cmd = [
        "docker",
        "build",
        "-f",
        str(HARNESS_DIR / "candidate.Dockerfile"),
        "--build-arg",
        f"PG_IMAGE={image}",
        # Sanitized cache key: a raw image reference contains ':', which is
        # invalid in a BuildKit cache-mount id and would let distinct ICU
        # versions collide and reuse the wrong binary.
        "--build-arg",
        f"TARGET_CACHE_ID={safe_label(image)}",
    ]
    if builder := PG_BUILDER_IMAGE.get(image):
        builder_image, suite = builder
        cmd += [
            "--build-arg",
            f"BUILDER_IMAGE={builder_image}",
            "--build-arg",
            f"DEBIAN_SUITE={suite}",
        ]
    cmd += ["-t", tag, str(root)]
    r = sh(cmd)
    if r.returncode != 0:
        raise RuntimeError(f"candidate image build failed:\n{r.stderr[-800:]}")
    return tag


def start_postgres(image, name, port):
    sh(["docker", "rm", "-f", name])
    r = sh(
        [
            "docker",
            "run",
            "-d",
            "--name",
            name,
            "-e",
            "POSTGRES_PASSWORD=cdc-review",
            "-p",
            f"127.0.0.1:{port}:5432",
            image,
        ]
    )
    if r.returncode != 0:
        raise RuntimeError(r.stderr)
    for _ in range(60):
        if sh(["docker", "exec", name, "pg_isready", "-U", "postgres"]).returncode == 0:
            return
        time.sleep(1)
    raise RuntimeError("postgres did not become ready")


def start_mysql(image, name, port):
    sh(["docker", "rm", "-f", name])
    r = sh(
        [
            "docker",
            "run",
            "-d",
            "--name",
            name,
            "-e",
            "MYSQL_ROOT_PASSWORD=cdc-review",
            "-p",
            f"127.0.0.1:{port}:3306",
            image,
        ]
    )
    if r.returncode != 0:
        raise RuntimeError(r.stderr)
    for _ in range(90):
        if (
            sh(["docker", "exec", name, "mysqladmin", "ping", "-uroot", "-pcdc-review"]).returncode
            == 0
        ):
            return
        time.sleep(2)
    raise RuntimeError("mysql did not become ready")


def stop(name):
    sh(["docker", "rm", "-f", name])


def image_id(reference):
    """The immutable image ID (digest) for a reference, or None."""
    r = sh(["docker", "image", "inspect", "--format", "{{.Id}}", reference])
    return r.stdout.strip() if r.returncode == 0 else None


def server_version(container, engine):
    """The server patch version reported inside the container, or None."""
    if engine == "postgres":
        r = sh(
            [
                "docker",
                "exec",
                container,
                "psql",
                "-U",
                "postgres",
                "-tAc",
                "SHOW server_version",
            ]
        )
    else:
        r = sh(
            [
                "docker",
                "exec",
                container,
                "mysql",
                "-uroot",
                "-pcdc-review",
                "-N",
                "-e",
                "SELECT VERSION()",
            ]
        )
    return r.stdout.strip() if r.returncode == 0 else None


def run_pg_image(image, corpus, specs, workdir, root, port=18432, keep=False):
    name = container_name("harness-pg", image)
    report = {"image": image, "engine": "postgres", "specs": [], "icu_major": None}
    if not image_present(image):
        raise RuntimeError(f"image {image} not present (docker pull it first)")
    report["icu_major"] = image_icu_major(image)

    # Candidate image derived FROM the oracle image => identical ICU.
    cand_image = build_candidate_image(image, root)
    report["candidate_image"] = cand_image
    report["oracle_image_id"] = image_id(image)
    report["candidate_image_id"] = image_id(cand_image)
    candidate = ContainerCandidate(cand_image)

    oracle = None
    try:
        start_postgres(image, name, port)
        report["server_version"] = server_version(name, engine="postgres")
        for _ in range(30):
            try:
                oracle = PostgresOracle(port=port)
                break
            except Exception:
                time.sleep(1)
        if oracle is None:
            raise RuntimeError("postgres accepted pg_isready but not connections")
        oracle.reset_table(corpus)
        sources = {}
        for spec in specs:
            try:
                sources[spec.id] = oracle.order(spec, corpus)
            except SourceUnsupported as e:
                # The server is too old for this feature (e.g. ICU `rules` before
                # PG 16). Expected skip, recorded so acceptance can account for it.
                report["specs"].append(
                    {
                        "id": spec.id,
                        "collation": spec.collation,
                        "deterministic": spec.deterministic,
                        "supported": False,
                        "ok": None,
                        "skipped": True,
                        "expected_unsupported": not spec.supported,
                        "note": f"source unsupported on this server: {e}",
                    }
                )
            except Exception as e:
                report["specs"].append(
                    {
                        "id": spec.id,
                        "collation": spec.collation,
                        "deterministic": spec.deterministic,
                        "error": str(e).splitlines()[0],
                    }
                )

        # Feed the source's own version to the candidate so the gate is exercised
        # with real data, then compare.
        specs_with_version = [
            {
                "id": s.id,
                "engine": "postgres",
                "collation": s.collation,
                "deterministic": s.deterministic,
                "version": sources[s.id].version,
                "rules": s.rules,
                "encoding": s.encoding,
            }
            for s in specs
            if s.id in sources
        ]
        candidates = candidate.run(corpus, specs_with_version, workdir)
        oracle.close()

        for spec in specs:
            if spec.id not in sources:
                continue
            src = sources[spec.id]
            cand = candidates[spec.id]
            # A runtime comparison failure is always a defect, never an accepted
            # refusal, even for a spec the matrix expects to be unsupported.
            if cand.outcome is Outcome.ERROR:
                report["specs"].append(
                    {
                        "id": spec.id,
                        "collation": spec.collation,
                        "deterministic": spec.deterministic,
                        "supported": False,
                        "outcome": Outcome.ERROR.value,
                        "ok": False,
                        "expected_unsupported": not spec.supported,
                        "note": cand.note,
                        "source_version": src.version,
                    }
                )
                continue
            if not cand.supported:
                # A clean construction refusal is correct only for a spec the
                # matrix expects to be unsupported; an unexpected refusal fails.
                expected_unsupported = not spec.supported
                report["specs"].append(
                    {
                        "id": spec.id,
                        "collation": spec.collation,
                        "deterministic": spec.deterministic,
                        "supported": False,
                        "outcome": Outcome.REFUSED.value,
                        "ok": expected_unsupported,
                        "expected_unsupported": expected_unsupported,
                        "note": cand.note,
                        "source_version": src.version,
                    }
                )
                continue
            if not spec.supported:
                report["specs"].append(
                    {
                        "id": spec.id,
                        "collation": spec.collation,
                        "deterministic": spec.deterministic,
                        "supported": True,
                        "ok": False,
                        "expected_unsupported": True,
                        "note": "crate accepted a configuration expected to be refused",
                    }
                )
                continue
            diff = compare(src, cand, corpus)
            version_ok = src.version == cand.version
            report["specs"].append(
                {
                    "id": spec.id,
                    "collation": spec.collation,
                    "deterministic": spec.deterministic,
                    "source_version": src.version,
                    "candidate_version": cand.version,
                    "version_ok": version_ok,
                    "diff": dataclasses.asdict(diff),
                    "ok": diff.ok and version_ok,
                }
            )
    finally:
        if not keep:
            stop(name)
    return report


def run_mysql_image(image, corpus, specs, port=18306, keep=False):
    name = container_name("harness-mysql", image)
    report = {"image": image, "engine": "mysql", "specs": []}
    if not image_present(image):
        raise RuntimeError(f"image {image} not present (docker pull it first)")

    # MySQL ordering is ICU-independent; the real crate runs on the host.
    candidate = HostCandidate()
    specs_with_version = [{"id": s.id, "engine": "mysql", "collation": s.collation} for s in specs]
    candidates = candidate.run(corpus, specs_with_version)

    report["oracle_image_id"] = image_id(image)
    oracle = None
    try:
        start_mysql(image, name, port)
        report["server_version"] = server_version(name, engine="mysql")
        for _ in range(30):
            try:
                oracle = MySqlOracle(port=port)
                break
            except Exception:
                time.sleep(2)
        if oracle is None:
            raise RuntimeError("mysql accepted ping but not application connections")
        oracle.reset_table(corpus)
        for spec in specs:
            try:
                src = oracle.order(spec, corpus)
            except Exception as e:
                report["specs"].append(
                    {"id": spec.id, "collation": spec.collation, "error": str(e).splitlines()[0]}
                )
                continue
            cand = candidates[spec.id]
            expected_supported = spec.supported
            # A runtime comparison failure is always a defect, never an accepted
            # refusal, even for a spec the matrix expects to be unsupported.
            if cand.outcome is Outcome.ERROR:
                report["specs"].append(
                    {
                        "id": spec.id,
                        "collation": spec.collation,
                        "supported": False,
                        "outcome": Outcome.ERROR.value,
                        "ok": False,
                        "expected_unsupported": not expected_supported,
                        "note": cand.note,
                        "source_order_hash": hash(tuple(src.order)),
                    }
                )
                continue
            if not cand.supported:
                # Must be a clean construction refusal for a spec the crate is
                # expected to refuse.
                ok = not expected_supported
                report["specs"].append(
                    {
                        "id": spec.id,
                        "collation": spec.collation,
                        "supported": False,
                        "outcome": Outcome.REFUSED.value,
                        "ok": ok,
                        "expected_unsupported": not expected_supported,
                        "note": cand.note,
                        "source_order_hash": hash(tuple(src.order)),
                    }
                )
                continue
            if not expected_supported:
                # The crate accepted a configuration the matrix expects it to
                # refuse: that is a support-boundary regression, not a pass,
                # even if the finite corpus happens to agree.
                report["specs"].append(
                    {
                        "id": spec.id,
                        "collation": spec.collation,
                        "supported": True,
                        "ok": False,
                        "expected_unsupported": True,
                        "note": "crate accepted a configuration expected to be refused",
                    }
                )
                continue
            diff = compare(src, cand, corpus)
            report["specs"].append(
                {
                    "id": spec.id,
                    "collation": spec.collation,
                    "supported": True,
                    "diff": dataclasses.asdict(diff),
                    "ok": diff.ok,
                }
            )
        oracle.close()
    finally:
        if not keep:
            stop(name)
    return report


def run_matrix(
    engine, images, corpus, pg_specs, mysql_specs, out, root, expected_images=None, keep=False
):
    reports = []
    workdir = Path(root) / ".work"
    workdir.mkdir(exist_ok=True)
    if engine in ("pg", "postgres", "all"):
        for image in images:
            if not image.startswith("postgres"):
                continue
            print(f"\n=== {image} ===")
            reports.append(run_pg_image(image, corpus, pg_specs, workdir, root, keep=keep))
    if engine in ("mysql", "all"):
        for image in images:
            if not image.startswith("mysql"):
                continue
            print(f"\n=== {image} ===")
            reports.append(run_mysql_image(image, corpus, mysql_specs, keep=keep))
    if out:
        Path(out).write_text(json.dumps(reports, indent=2))
        print(f"\nreport -> {out}")

    reports = attach_acceptance(reports, expected_images, pg_specs, mysql_specs)
    return reports


def _expected_spec_ids(report, pg_specs, mysql_specs):
    ids = [s.id for s in (pg_specs if report["engine"] == "postgres" else mysql_specs)]
    return ids


def attach_acceptance(reports, expected_images, pg_specs, mysql_specs):
    """Annotate each report with a fail-closed acceptance verdict.

    A report is acceptable only if it lists every expected spec and each one is
    explicitly ``ok is True``. Source errors, missing results, unexpected
    refusals (``ok is None``), and unexpected support/refusal all fail.
    """
    for report in reports:
        failures = []
        listed = {s["id"]: s for s in report["specs"]}
        for sid in _expected_spec_ids(report, pg_specs, mysql_specs):
            s = listed.get(sid)
            if s is None:
                failures.append(f"{sid}: missing result")
                continue
            if s.get("skipped"):
                # A source-unsupported feature on this server is acceptable only
                # for a spec the matrix already marks unsupported.
                if s.get("expected_unsupported", False):
                    continue
                failures.append(f"{sid}: source unexpectedly unsupported: {s.get('note', '')}")
            elif "error" in s:
                failures.append(f"{sid}: source error: {s['error']}")
            elif s.get("ok") is not True:
                note = s.get("note", "")
                failures.append(f"{sid}: not explicitly successful (ok={s.get('ok')!s}) {note}")
        report["acceptance_failures"] = failures
        report["accepted"] = not failures

    if expected_images is not None:
        got = {r["image"] for r in reports}
        missing_images = sorted(set(expected_images) - got)
    else:
        missing_images = []
    empty = not reports or all(not r["specs"] for r in reports)
    return {
        "reports": reports,
        "missing_images": missing_images,
        "empty": empty,
        "ok": bool(reports)
        and not empty
        and not missing_images
        and all(r["accepted"] for r in reports),
    }
