"""Compare a source order/ranks to a candidate order/ranks."""

from collections import Counter, defaultdict

from .model import DiffResult


def _normalize_rank(rank) -> dict:
    return {int(k): int(v) for k, v in rank.items()}


def _groups(rank_by_id: dict):
    g = defaultdict(set)
    for i, r in rank_by_id.items():
        g[r].add(i)
    return [frozenset(g[k]) for k in sorted(g)]


def compare_orders(source_order, source_rank, cand_order, cand_rank):
    """Compare two total orders by sequence and equivalence partition.

    For a total preorder this is equivalent to checking every pairwise relation
    but runs in O(n) rather than O(n^2), so it scales to full-BMP sweeps. Returns
    `(ok, reason, examples)`.
    """
    sr = _normalize_rank(source_rank)
    cr = _normalize_rank(cand_rank)
    if _groups(sr) != _groups(cr):
        # Report the first few codepoints whose equivalence class differs.
        sg = {i: r for i, r in sr.items()}
        cg = {i: r for i, r in cr.items()}
        # Map each side's class index to its members for a readable diff.
        s_rep = {frozenset(v): k for k, v in enumerate(_groups(sr))}
        c_rep = {frozenset(v): k for k, v in enumerate(_groups(cr))}
        bad = [
            i
            for i in sg
            if s_rep.get(frozenset(_members(sr, sg[i])))
            != c_rep.get(frozenset(_members(cr, cg[i])))
        ]
        examples = [hex(i) for i in bad[:8]]
        return False, "equivalence partition differs", examples
    if list(source_order) != list(cand_order):
        return False, "order sequence differs", []
    return True, "", []


def _members(rank_by_id, r):
    return {i for i, v in rank_by_id.items() if v == r}


def compare(source, candidate, corpus) -> DiffResult:
    """Full pairwise + partition comparison. `source`/`candidate` are OrderResult."""
    sr = _normalize_rank(source.rank)
    cr = _normalize_rank(candidate.rank)
    n = len(corpus)
    s_order = list(source.order)
    c_order = list(candidate.order)

    seq_ok = s_order == c_order
    partition_ok = _groups(sr) == _groups(cr)

    # Full pairwise relation check over ranks.
    pairs = 0
    disagreements = []
    cats = Counter()
    for i in range(n):
        si, ci = sr[i], cr[i]
        for j in range(i + 1, n):
            pairs += 1
            srel = (si > sr[j]) - (si < sr[j])
            crel = (ci > cr[j]) - (ci < cr[j])
            if srel != crel:
                disagreements.append((i, j))
                key = tuple(sorted((corpus[i].cat, corpus[j].cat)))
                cats[key] += 1
    examples = [
        (corpus[i].cat, corpus[i].s, corpus[j].cat, corpus[j].s) for i, j in disagreements[:8]
    ]
    return DiffResult(
        spec_id=source.spec_id,
        seq_ok=seq_ok,
        partition_ok=partition_ok,
        pair_disagreements=len(disagreements),
        pairs_checked=pairs,
        top_categories=cats.most_common(10),
        examples=examples,
    )
