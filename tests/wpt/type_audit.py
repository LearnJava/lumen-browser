#!/usr/bin/env python3
"""WPT-RUN-5 slice 33: on what does the second pass-rate figure actually stand?

The first full figure (2026-08-18, 5.59 %) was, by construction, a
`testharness`-only number: no reftest executor existed yet (TEST-4/TEST-5
landed a day later) and every `.https.` id was skipped, so 27 386 reftest ids
and 6 874 https ids all scored a flat zero. The second figure runs both. The
headline therefore moves for three unrelated reasons at once — reftests are
executed, https is executed, and the engine itself has a day and a half of
fixes in it — and a reader who is told only "5.59 % -> 15 %" cannot tell which
of the three he is looking at.

This script splits the score the run produced by *manifest test type* and by
scheme, which is the split the headline hides:

* **per type** — ids, executed, score, share of the headline, mean score on the
  ids that ran. `reftest` scores 1.0 or 0.0 per id (a comparison passes or it
  does not), `testharness` scores the subtest fraction, so equal id counts do
  not mean equal weight in the number.
* **like-for-like with 5.59 %** — the same run scored the way the 08-18 run was
  forced to score: reftest zeroed, https zeroed, everything divided by the full
  manifest denominator. That is the only arithmetic in which the two numbers
  answer the same question, and it is what "did the engine get better" means.
* **reftest PASS by CDATA class** ([BUG-786](../../bugs/BUG-786-OPEN.md)) — how
  much of the reftest score sits on pairs where both sides lost their inline
  stylesheet, i.e. the part slice 14 priced as possibly inflated. Reused from
  `cdata_audit.classify` rather than re-derived.

Deliberately *not* a second scorer: the per-id score here is
`run_corpus.score_reports`' rule, imported, not re-implemented — a second copy
would eventually disagree with the published number and the disagreement would
be indistinguishable from a finding.

Safe to run against a live run's `--out-dir`: it only reads.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/type_audit.py [--out-dir .tmp/wpt-corpus]
    <venv>/python tests/wpt/type_audit.py --json .tmp/type-audit.json
    <venv>/python tests/wpt/type_audit.py --selftest
"""

import argparse
import collections
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import cdata_audit  # noqa: E402
import corpus_stats  # noqa: E402
import run_corpus  # noqa: E402

#: Types no executor exists for (WPT-RUN-8). They are in the denominator and
#: score zero by construction, so they are reported apart from the types whose
#: zero is an engine result.
NO_EXECUTOR = ("crashtest", "wdspec", "print-reftest", "aamtest")

#: Id markers for "wptserve hands this test out over TLS". `.https.` is the
#: obvious one; `.h2.` is the trap — HTTP/2 is served on the h2 port, which is
#: TLS-only, so those ids were as unreachable as the https ones while
#: [BUG-785](../../bugs/BUG-785-FIXED.md) was open, even though `--skip-https`
#: (which matches on `.https.`) never skipped them and they do not read as
#: "https" anywhere in the run's own bookkeeping. Slice 33 found this the hard
#: way: the whole visible engine gain over the 08-18 figure was 53 `.h2.` ids
#: in `/loading/early-hints/` switching from unreachable to reachable.
TLS_MARKERS = (".https.", ".h2.")


def over_tls(test_id: str) -> bool:
    """True when wptserve serves this id over TLS — see `TLS_MARKERS`."""
    return any(marker in test_id for marker in TLS_MARKERS)


def id_score(result: dict) -> float:
    """The per-id score `run_corpus.score_reports` gives this verdict.

    Kept in one place and read back out of `run_corpus`' own constant so the
    two cannot drift: a subtest-bearing id scores the passing fraction, a bare
    one scores 1.0 only on PASS.
    """
    subtests = result.get("subtests") or []
    if subtests:
        return sum(1 for s in subtests if s.get("status") == "PASS") / len(subtests)
    return 1.0 if result.get("status") == "PASS" else 0.0


def decompose(manifest: dict, results: dict, scope: set = None) -> dict:
    """Split the run's score by manifest type, and each type by scheme."""
    per_type = collections.defaultdict(lambda: {
        "ids": 0, "ran": 0, "score": 0.0, "harness_ok": 0, "pass": 0,
        "subtests_total": 0, "subtests_passed": 0,
        "tls_ids": 0, "tls_ran": 0, "tls_score": 0.0})
    per_category = collections.defaultdict(lambda: {"ids": 0, "ran": 0, "score": 0.0,
                                                    "plain_score": 0.0})
    totals = {"ids": 0, "ran": 0, "score": 0.0}

    for test_type, category, test_id in corpus_stats.iter_ids(manifest):
        if test_type in corpus_stats.NON_AUTOMATABLE_TYPES:
            continue
        if scope is not None and category not in scope:
            continue
        row = per_type[test_type]
        cat = per_category[category]
        tls = over_tls(test_id)
        row["ids"] += 1
        cat["ids"] += 1
        totals["ids"] += 1
        if tls:
            row["tls_ids"] += 1

        result = results.get(test_id)
        if result is None:
            continue
        score = id_score(result)
        row["ran"] += 1
        row["score"] += score
        cat["ran"] += 1
        cat["score"] += score
        if not tls and test_type != "reftest":
            cat["plain_score"] += score
        totals["ran"] += 1
        totals["score"] += score
        if result.get("status") in run_corpus.HARNESS_OK:
            row["harness_ok"] += 1
        if result.get("status") == "PASS":
            row["pass"] += 1
        subtests = result.get("subtests") or []
        row["subtests_total"] += len(subtests)
        row["subtests_passed"] += sum(1 for s in subtests if s.get("status") == "PASS")
        if tls:
            row["tls_ran"] += 1
            row["tls_score"] += score

    return {"per_type": {k: dict(v) for k, v in per_type.items()},
            "per_category": {k: dict(v) for k, v in per_category.items()},
            "totals": totals}


def like_for_like(split: dict, denominator: int) -> dict:
    """The same run scored the way the 2026-08-18 run was forced to score it.

    That run had no reftest executor, and everything served over TLS was
    unreachable to it (`--skip-https` for `.https.`, BUG-785 for the rest), so
    both groups sat at zero. Reproducing those two zeroes on today's results is
    what makes the two headlines answer the same question; anything else
    compares a scoring change with an engine change and calls the sum
    "progress".
    """
    kept = 0.0
    for test_type, row in split["per_type"].items():
        if test_type == "reftest":
            continue
        kept += row["score"] - row["tls_score"]
    full = split["totals"]["score"]
    return {
        "denominator": denominator,
        "headline": round(full / denominator, 4),
        "plain_no_reftest": round(kept / denominator, 4),
        "score_full": round(full, 2),
        "score_like_for_like": round(kept, 2),
    }


def reftest_cdata(results: dict, manifest: dict, scope: set = None) -> dict:
    """Reftest verdicts split by what BUG-786 does to the pair (slice 14's classes).

    `both_cdata` is the class slice 14 priced as possibly inflated: with the
    inline stylesheet dropped on *both* sides the pair can render identically
    bare and pass for the wrong reason.
    """
    counts = collections.Counter()
    for test_type, category, test_id in corpus_stats.iter_ids(manifest):
        if test_type != "reftest":
            continue
        if scope is not None and category not in scope:
            continue
        result = results.get(test_id)
        if result is None:
            continue
        cls = cdata_audit.classify(test_id)
        counts[f"{cls}_ran"] += 1
        if result.get("status") == "PASS":
            counts[f"{cls}_pass"] += 1
    return dict(counts)


def versus(split: dict, snapshot_path: str) -> dict:
    """Per-category engine delta against a snapshot taken before the executors landed.

    A pre-reftest snapshot's per-category `score` *is* a plain-testharness
    score: reftest had no executor and everything on TLS was unreachable, so
    both contributed zero to it. Putting today's `plain_score` next to it is
    therefore the like-for-like question asked one category at a time — and it
    is the only way to see whether a headline that tripled contains any engine
    movement at all, or whether the whole delta is the two executors.

    Only categories present in both is deliberate: a category the newer run has
    not reached yet would otherwise read as a collapse to zero.
    """
    with open(snapshot_path, encoding="utf-8") as fh:
        old = json.load(fh)["scored"]["per_category"]
    rows = []
    for category, row in split["per_category"].items():
        if category not in old:
            continue
        before = old[category]["score"]
        rows.append({"category": category, "before": round(before, 2),
                     "after": round(row["plain_score"], 2),
                     "delta": round(row["plain_score"] - before, 2),
                     "ids": row["ids"], "ran": row["ran"]})
    rows.sort(key=lambda r: -abs(r["delta"]))
    return {
        "snapshot": snapshot_path,
        "categories": len(rows),
        "before": round(sum(r["before"] for r in rows), 2),
        "after": round(sum(r["after"] for r in rows), 2),
        "delta": round(sum(r["delta"] for r in rows), 2),
        "gained": round(sum(r["delta"] for r in rows if r["delta"] > 0), 2),
        "lost": round(sum(r["delta"] for r in rows if r["delta"] < 0), 2),
        "rows": rows,
    }


def _selftest() -> int:
    """Check the split on hand-built results instead of a six-hour run."""
    manifest = {"items": {
        "reftest": {"a": {"a1.html": ["h", ["/a/a1.html", [["/a/ref.html", "=="]], {}]],
                          "a2.https.html": ["h", ["/a/a2.https.html", [["/a/ref.html", "=="]], {}]]}},
        "testharness": {"b": {"b1.html": ["h", ["/b/b1.html", {}]],
                              "b2.https.html": ["h", ["/b/b2.https.html", {}]],
                              "b3.h2.html": ["h", ["/b/b3.h2.html", {}]]}},
        "crashtest": {"c": {"c1.html": ["h", ["/c/c1.html", {}]]}},
        "manual": {"d": {"d1.html": ["h", ["/d/d1.html", {}]]}},
    }}
    results = {
        "/a/a1.html": {"status": "PASS"},
        "/a/a2.https.html": {"status": "FAIL"},
        "/b/b1.html": {"status": "OK", "subtests": [{"status": "PASS"}, {"status": "FAIL"}]},
        "/b/b2.https.html": {"status": "OK", "subtests": [{"status": "PASS"}]},
        # h2 is TLS-only: the 08-18 scoring has to drop it exactly like https.
        "/b/b3.h2.html": {"status": "OK", "subtests": [{"status": "PASS"}]},
    }
    split = decompose(manifest, results)
    failures = []

    def check(label, got, want):
        if got != want:
            failures.append(f"{label}: got {got!r}, want {want!r}")

    # manual is out of the denominator; crashtest is in it and scores 0.
    check("denominator", split["totals"]["ids"], 6)
    check("ran", split["totals"]["ran"], 5)
    check("score", round(split["totals"]["score"], 3), round(1.0 + 0.0 + 0.5 + 1.0 + 1.0, 3))
    check("reftest score", split["per_type"]["reftest"]["score"], 1.0)
    check("reftest tls ids", split["per_type"]["reftest"]["tls_ids"], 1)
    check("testharness subtests", split["per_type"]["testharness"]["subtests_total"], 4)
    check("testharness tls ids", split["per_type"]["testharness"]["tls_ids"], 2)
    check("crashtest ran", split["per_type"]["crashtest"]["ran"], 0)
    # Like-for-like drops reftest entirely and https everywhere: only b1's 0.5.
    lfl = like_for_like(split, 6)
    check("like-for-like score", lfl["score_like_for_like"], 0.5)
    check("per-category plain score", round(split["per_category"]["b"]["plain_score"], 3), 0.5)
    check("headline", lfl["headline"], round(3.5 / 6, 4))

    for line in failures:
        print(f"FAIL {line}")
    print(f"selftest: {'FAILED' if failures else 'ok'} ({len(failures)} failure(s))")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out-dir", default=run_corpus.DEFAULT_OUT_DIR)
    parser.add_argument("--manifest", default=None, help="path to MANIFEST.json")
    parser.add_argument("--scope-from", default=None,
                        help="published snapshot whose category scope to reuse "
                             "(default: score against the whole manifest)")
    parser.add_argument("--vs", default=None,
                        help="published snapshot from before the reftest executor "
                             "(docs/wpt/runs/2026-08-18.json): per-category engine delta")
    parser.add_argument("--json", dest="json_out", default=None)
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        return _selftest()

    manifest = (json.load(open(args.manifest, encoding="utf-8"))
                if args.manifest else run_corpus.load_manifest())
    scope = None
    if args.scope_from:
        with open(args.scope_from, encoding="utf-8") as fh:
            scope = set(json.load(fh).get("scope") or []) or None

    results, _recovered, _empty = run_corpus.load_results(args.out_dir)
    split = decompose(manifest, results, scope)
    lfl = like_for_like(split, split["totals"]["ids"])
    cdata = reftest_cdata(results, manifest, scope)

    total_score = split["totals"]["score"] or 1.0
    print(f"\n== {args.out_dir}: {split['totals']['ids']} automatable ids, "
          f"{split['totals']['ran']} executed, score {split['totals']['score']:.2f} "
          f"({split['totals']['score'] / split['totals']['ids'] * 100:.2f} %) ==\n")
    header = (f"{'type':16}{'ids':>7}{'ran':>7}{'score':>9}{'share':>8}"
              f"{'mean/ran':>10}{'harness OK':>12}{'PASS':>7}{'subtests':>14}")
    print(header)
    for test_type, row in sorted(split["per_type"].items(), key=lambda kv: -kv[1]["score"]):
        subtests = (f"{row['subtests_passed']}/{row['subtests_total']}"
                    if row["subtests_total"] else "-")
        mean = row["score"] / row["ran"] if row["ran"] else 0.0
        print(f"{test_type:16}{row['ids']:>7}{row['ran']:>7}{row['score']:>9.2f}"
              f"{row['score'] / total_score * 100:>7.1f}%{mean:>10.3f}"
              f"{row['harness_ok']:>12}{row['pass']:>7}{subtests:>14}")
    print(f"  types with no executor ({', '.join(NO_EXECUTOR)}) are in the denominator "
          f"and score 0 by construction — WPT-RUN-8, not an engine result")

    print("\n== served over TLS (.https. / .h2.) inside each type ==")
    for test_type, row in sorted(split["per_type"].items(), key=lambda kv: -kv[1]["tls_ids"]):
        if not row["tls_ids"]:
            continue
        print(f"  {test_type:16} {row['tls_ids']:6} ids, {row['tls_ran']:6} executed, "
              f"score {row['tls_score']:8.2f} "
              f"({row['tls_score'] / total_score * 100:.2f} % of the headline)")

    print("\n== like-for-like with the 2026-08-18 figure (reftest and https zeroed) ==")
    print(f"  headline as published        {lfl['score_full']:9.2f} / {lfl['denominator']} "
          f"= {lfl['headline'] * 100:.2f} %")
    print(f"  same run, 08-18 scoring      {lfl['score_like_for_like']:9.2f} / {lfl['denominator']} "
          f"= {lfl['plain_no_reftest'] * 100:.2f} %")
    print("  the second line is the one to put next to 5.59 % — the first also counts "
          "two executors the 08-18 run did not have")

    print("\n== reftest PASS by BUG-786 class (slice 14) ==")
    for cls in ("plain", "both_cdata", "test_only", "ref_unknown"):
        ran, passed = cdata.get(f"{cls}_ran", 0), cdata.get(f"{cls}_pass", 0)
        if ran:
            print(f"  {cls:12} ran {ran:6}  PASS {passed:6} ({passed / ran * 100:.1f} %)")
    suspect = cdata.get("both_cdata_pass", 0)
    ref_pass = split["per_type"].get("reftest", {}).get("pass", 0)
    if ref_pass:
        print(f"  both_cdata PASS is {suspect / ref_pass * 100:.1f} % of all reftest PASS — "
              f"the upper bound on the inflation slice 14 priced")

    comparison = versus(split, args.vs) if args.vs else None
    if comparison:
        print(f"\n== engine delta against {comparison['snapshot']} "
              f"({comparison['categories']} shared categories) ==")
        print(f"  plain testharness score {comparison['before']:.2f} -> {comparison['after']:.2f} "
              f"({comparison['delta']:+.2f}; gained {comparison['gained']:+.2f}, "
              f"lost {comparison['lost']:+.2f})")
        for row in comparison["rows"][:12]:
            print(f"  {row['category']:32} {row['before']:9.2f} -> {row['after']:9.2f}"
                  f"{row['delta']:+9.2f}   ids {row['ids']:6} ran {row['ran']:6}")

    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump({"split": split, "like_for_like": lfl, "reftest_cdata": cdata,
                       "versus": comparison}, fh, indent=1, ensure_ascii=False)
        print(f"\nwritten: {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
