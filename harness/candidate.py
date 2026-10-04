"""Candidate adapter: the real `db-collation` library as the candidate under test.

There is no Python reimplementation of the algorithms. The Rust binary built
from `crates/harness-candidate` (which depends on `db-collation`) orders the
corpus; this module only runs it and parses its JSON.

Two placements, chosen per engine:

* PostgreSQL: the candidate is built and run **inside the candidate image**,
  which is derived `FROM` the same image as the oracle, so it links the exact
  same ICU data. The harness passes the source's reported `collversion` so the
  library's version gate is exercised.
* MySQL: the binary runs on the **host**; MySQL ordering depends on the pinned
  UCA weight tables, not on the host ICU.

A spec the library refuses is returned as `OrderResult(supported=False)`.
"""

import json
import subprocess
import tempfile
import uuid
from pathlib import Path

from .model import OrderResult, Outcome

# Where the candidate binary lives inside the candidate image.
CONTAINER_BINARY = "/usr/local/bin/harness-candidate"


def _load_candidate_json(path, specs_by_id):
    out = {}
    for item in json.loads(Path(path).read_text()):
        raw_outcome = item.get("outcome")
        outcome = Outcome(raw_outcome) if raw_outcome is not None else None
        out[item["spec_id"]] = OrderResult(
            spec_id=item["spec_id"],
            order=item["order"],
            rank={int(k): int(v) for k, v in item["rank"].items()},
            version=item.get("version"),
            supported=item.get("supported", True),
            note=item.get("note", ""),
            outcome=outcome,
        )
    return out


def _write_inputs(workdir, corpus, specs_with_version):
    corpus_path = Path(workdir) / "corpus.json"
    specs_path = Path(workdir) / "specs.json"
    out_path = Path(workdir) / "candidate.json"
    corpus_path.write_text(
        json.dumps([{"id": s.id, "cat": s.cat, "s": s.s} for s in corpus], ensure_ascii=False)
    )
    specs_path.write_text(json.dumps(specs_with_version))
    return corpus_path, specs_path, out_path


class HostCandidate:
    """Runs the candidate binary on the host (used for MySQL)."""

    def __init__(self, binary="target/release/harness-candidate"):
        self.binary = Path(binary)

    def run(self, corpus, specs_with_version):
        if not self.binary.exists():
            raise RuntimeError(f"candidate binary {self.binary} not built; run `make candidate`")
        with tempfile.TemporaryDirectory() as tmp:
            corpus_path, specs_path, out_path = _write_inputs(tmp, corpus, specs_with_version)
            proc = subprocess.run(
                [str(self.binary), str(corpus_path), str(specs_path), str(out_path)],
                text=True,
                capture_output=True,
            )
            if proc.returncode != 0:
                raise RuntimeError(f"candidate failed: {proc.stderr.strip()}")
            return _load_candidate_json(out_path, specs_with_version)


class ContainerCandidate:
    """Runs the candidate binary inside a container (used for PostgreSQL).

    The image is derived from the oracle image, so the binary links the same
    ICU. The Rust toolchain is never invoked here: the binary is baked into the
    image at build time as the entrypoint (`CONTAINER_BINARY`).
    """

    def __init__(self, image, timeout=600):
        self.image = image
        self.timeout = timeout

    def run(self, corpus, specs_with_version, mountdir):
        workdir = Path(mountdir).resolve()
        _, _, out_path = _write_inputs(workdir, corpus, specs_with_version)
        out_path.write_text("")  # ensure the file exists for the bind mount
        # A unique, owned name so cleanup only ever touches this invocation's
        # container, even if several harness runs are in flight.
        name = f"harness-candidate-{uuid.uuid4().hex[:12]}"
        cmd = [
            "docker",
            "run",
            "--rm",
            "--name",
            name,
            "-v",
            f"{workdir}:/work",
            self.image,
            "/work/corpus.json",
            "/work/specs.json",
            "/work/candidate.json",
        ]
        proc = None
        try:
            proc = subprocess.run(
                cmd,
                text=True,
                capture_output=True,
                timeout=self.timeout,
            )
        except subprocess.TimeoutExpired as exc:
            raise RuntimeError(f"candidate container timed out after {self.timeout}s") from exc
        finally:
            # `docker run` is a client: on timeout (or a client crash) the
            # daemon-managed container keeps running. Force-remove the owned
            # container by name; ignore "no such container" when the normal
            # `--rm` already cleaned it up.
            subprocess.run(
                ["docker", "rm", "-f", name],
                text=True,
                capture_output=True,
                check=False,
            )
        if proc.returncode != 0:
            raise RuntimeError(
                f"candidate container failed: {(proc.stderr or proc.stdout).strip()}"
            )
        return _load_candidate_json(out_path, specs_with_version)
