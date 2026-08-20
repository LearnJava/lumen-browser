#!/usr/bin/env python3
"""WPT-RUN-5 slice 15: does the corpus number leak, and what does a lost shard cost?

`run_corpus.py` scores against `MANIFEST.json`, so every id with no verdict
scores 0. That is the honest choice, but it also means the zero bucket has to
be *explained* before a number built on it is published: an id sitting at zero
because no executor exists for its type is a known ceiling, while an id sitting
at zero because the harness quietly dropped it is a leak, and the headline
cannot tell them apart.

This script splits that bucket three ways and prices the parts:

* **accounting** — every automatable manifest id in the run's scope, classified
  as ran / no-executor-for-type / inside a shard killed on its time budget /
  lost inside a shard that ran to completion. The last bucket is the leak, and
  it should stay empty; anything in it is a defect in the harness, not a
  property of the engine.
* **kill cost** — what the killed shards were actually worth, from the results
  their raw streams salvaged (`rescue_results`). A shard that dies in a
  category where every test times out costs the number nothing; one that dies
  in a healthy category costs real score. Measuring instead of assuming is what
  decides whether a wider budget is a correctness fix or just tidier bookkeeping.
* **wall-clock** — the marginal seconds a TIMEOUT verdict costs against a
  resolved one, fitted across shards. This is what makes a corpus run take a
  day: a test that hangs burns its whole declared ceiling (10 s, or 60 s when
  the manifest marks it `timeout: long`, plus wptrunner's 5 s `extra_timeout`),
  a test that answers costs milliseconds.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/score_audit.py [--out-dir .tmp/wpt-corpus]
        [--manifest PATH] [--json OUT]

Safe to run against a live run's `--out-dir`: it only reads.
"""

import argparse
import collections
import json
import os
import statistics
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import corpus_stats  # noqa: E402
import run_corpus  # noqa: E402


def shard_index(manifest: dict, state: dict) -> tuple:
    """`(id -> shard name, shard name -> planned shard)` for the shards a run holds.

    Re-plans the categories the state file mentions rather than trusting the
    state's own records: `plan_shards` is the only place that knows how a
    category splits, and re-running it is what lets an id be attributed to the
    shard that was supposed to carry it.
    """
    names = {s["name"] for s in state["shards"]}
    categories = sorted({n.split("/")[0].replace(" (bare)", "") for n in names})
    planned = {s["name"]: s for s in run_corpus.plan_shards(manifest, categories)}
    # Longest prefix wins: `/css/CSS2/` must not swallow `/css/CSS2/floats/`.
    prefixes = sorted(((s["prefix"], name) for name, s in planned.items() if s["prefix"]),
                      key=lambda pair: -len(pair[0]))
    bare = {test_id: name for name, s in planned.items() for test_id in (s.get("test_ids") or [])}

    def owner(test_id: str):
        if test_id in bare:
            return bare[test_id]
        for prefix, name in prefixes:
            if test_id.startswith(prefix):
                return name
        return None

    return owner, planned


def accounting(manifest: dict, state: dict, results: dict, empty: list) -> dict:
    """Classify every automatable id of the attempted shards."""
    owner, _planned = shard_index(manifest, state)
    outcome = {s["name"]: s["outcome"] for s in state["shards"]}
    empty_set = set(empty)
    executable = corpus_stats.supported_types()

    per_type = collections.Counter()
    not_run_type = collections.Counter()
    cause = collections.Counter()
    leaked = []

    for test_type, category, test_id in corpus_stats.iter_ids(manifest):
        if test_type in corpus_stats.NON_AUTOMATABLE_TYPES:
            continue
        name = owner(test_id)
        if name is None or name not in outcome:
            continue  # shard not attempted in this run
        per_type[test_type] += 1
        if test_id in results:
            continue
        not_run_type[test_type] += 1
        if test_type not in executable:
            cause[f"no-executor:{test_type}"] += 1
        elif outcome[name] != "ran":
            cause["shard-killed"] += 1
        elif name in empty_set:
            cause["shard-empty"] += 1
        else:
            cause["lost-in-ran-shard"] += 1
            if len(leaked) < 50:
                leaked.append({"type": test_type, "category": category, "id": test_id,
                               "shard": name})
    return {"per_type": dict(per_type), "not_run_by_type": dict(not_run_type),
            "cause": dict(cause), "leaked_examples": leaked}


def kill_cost(manifest: dict, state: dict, out_dir: str) -> dict:
    """What the budget-killed shards scored on the part of them that did run."""
    owner, planned = shard_index(manifest, state)
    rows = []
    for shard in state["shards"]:
        if shard["outcome"] == "ran":
            continue
        name = shard["name"]
        raw = os.path.join(out_dir, name.replace("/", "__") + run_corpus.RAW_SUFFIX)
        rescued = run_corpus.rescue_results(raw)
        score = 0.0
        subtests_total = subtests_passed = 0
        statuses = collections.Counter()
        for result in rescued.values():
            statuses[result.get("status", "")] += 1
            subtests = result.get("subtests") or []
            if subtests:
                passed = sum(1 for s in subtests if s.get("status") == "PASS")
                subtests_total += len(subtests)
                subtests_passed += passed
                score += passed / len(subtests)
            elif result.get("status") == "PASS":
                score += 1.0
        auto = planned.get(name, {}).get("auto_ids", 0)
        rows.append({"shard": name, "auto_ids": auto,
                     "long_ids": planned.get(name, {}).get("long_ids", 0),
                     "rescued": len(rescued), "lost": max(auto - len(rescued), 0),
                     "score": round(score, 2), "seconds": shard.get("seconds"),
                     "subtests": f"{subtests_passed}/{subtests_total}",
                     "statuses": dict(statuses)})
    scored = sum(r["score"] for r in rows)
    rescued = sum(r["rescued"] for r in rows)
    lost = sum(r["lost"] for r in rows)
    return {"shards": rows, "rescued": rescued, "score": round(scored, 2), "lost_ids": lost,
            "projected_lost_score": round(lost * scored / rescued, 2) if rescued else 0.0}


def wall_clock(state: dict, out_dir: str, min_results: int = 20) -> dict:
    """Fit `seconds ~= a*TIMEOUT + b*resolved + c` over the shards that finished.

    A per-shard aggregate rather than a per-test sum on purpose: what a run
    costs is shard wall-clock, and the fixed term is the wptserve boot every
    shard pays whatever it holds.
    """
    rows = []
    for shard in state["shards"]:
        if shard["outcome"] != "ran":
            continue
        path = os.path.join(out_dir, shard["name"].replace("/", "__") + ".json")
        if not os.path.isfile(path) or os.path.getsize(path) == 0:
            continue
        try:
            with open(path, encoding="utf-8") as fh:
                report = json.load(fh)
        except (json.JSONDecodeError, OSError):
            continue
        statuses = collections.Counter(r.get("status", "") for r in report.get("results", []))
        n = sum(statuses.values())
        if n < min_results:
            continue
        timeouts = statuses.get("TIMEOUT", 0) + statuses.get("EXTERNAL-TIMEOUT", 0)
        rows.append((shard["name"], n, timeouts, shard.get("seconds") or 0.0))
    if len(rows) < 3:
        return {"shards": len(rows)}

    design = [[r[2], r[1] - r[2], 1.0] for r in rows]
    target = [r[3] for r in rows]
    normal = [[sum(design[k][i] * design[k][j] for k in range(len(design))) for j in range(3)]
              + [sum(design[k][i] * target[k] for k in range(len(design)))] for i in range(3)]
    for i in range(3):  # gaussian elimination with partial pivoting
        pivot = max(range(i, 3), key=lambda r: abs(normal[r][i]))
        normal[i], normal[pivot] = normal[pivot], normal[i]
        for r in range(3):
            if r != i and normal[i][i]:
                factor = normal[r][i] / normal[i][i]
                normal[r] = [normal[r][c] - factor * normal[i][c] for c in range(4)]
    coef = [normal[i][3] / normal[i][i] for i in range(3)]

    buckets = collections.defaultdict(list)
    for _name, n, timeouts, seconds in rows:
        buckets[min(int(timeouts / n * 5), 4)].append(seconds / n)
    total_seconds = sum(r[3] for r in rows)
    total_timeouts = sum(r[2] for r in rows)
    attributable = total_timeouts * (coef[0] - coef[1])
    return {
        "shards": len(rows), "results": sum(r[1] for r in rows), "timeouts": total_timeouts,
        "seconds_per_timeout": round(coef[0], 2), "seconds_per_resolved": round(coef[1], 2),
        "seconds_per_shard_fixed": round(coef[2], 1),
        "hours_total": round(total_seconds / 3600, 2),
        "hours_on_timeouts": round(attributable / 3600, 2),
        "share_on_timeouts": round(attributable / total_seconds, 3) if total_seconds else 0.0,
        "median_seconds_per_id_by_timeout_share": {
            f"{k * 20}-{k * 20 + 20}%": round(statistics.median(v), 2)
            for k, v in sorted(buckets.items())},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out-dir", default=".tmp/wpt-corpus",
                        help="run directory holding the shard reports and state.json")
    parser.add_argument("--manifest", default=None, help="MANIFEST.json (default: vendored)")
    parser.add_argument("--json", dest="json_out", default=None, help="write the full result here")
    args = parser.parse_args()

    if args.manifest:
        run_corpus.MANIFEST_PATH = args.manifest
    manifest = run_corpus.load_manifest()
    state_path = os.path.join(args.out_dir, "state.json")
    if not os.path.isfile(state_path):
        print(f"no state.json in {args.out_dir} — nothing to audit", file=sys.stderr)
        return 2
    with open(state_path, encoding="utf-8") as fh:
        state = json.load(fh)
    results, _recovered, empty = run_corpus.load_results(args.out_dir)

    acc = accounting(manifest, state, results, empty)
    kills = kill_cost(manifest, state, args.out_dir)
    clock = wall_clock(state, args.out_dir)

    print(f"scope: {len(state['shards'])} shards, "
          f"{sum(acc['per_type'].values())} automatable ids, {len(results)} verdicts")
    print("\n== ids with no verdict, by cause ==")
    for name, n in sorted(acc["cause"].items(), key=lambda kv: -kv[1]):
        print(f"  {name:28} {n:6}")
    leak = acc["cause"].get("lost-in-ran-shard", 0)
    print(f"  {'LEAK' if leak else 'no leak':28} "
          f"{'ids scored 0 inside a shard that ran to completion' if leak else ''}")
    for entry in acc["leaked_examples"][:10]:
        print(f"      {entry['type']:12} {entry['id']}  (shard {entry['shard']})")
    print("\n== not_run share by type ==")
    for test_type, n in sorted(acc["per_type"].items(), key=lambda kv: -kv[1]):
        missing = acc["not_run_by_type"].get(test_type, 0)
        print(f"  {test_type:16} {n:7}  no verdict {missing:6} ({missing / n * 100:.1f}%)")

    print("\n== budget-killed shards: what they were worth ==")
    for row in kills["shards"]:
        print(f"  {row['shard']:18} auto={row['auto_ids']:4} long={row['long_ids']:4} "
              f"rescued={row['rescued']:4} lost={row['lost']:4} score={row['score']:6.2f} "
              f"subtests={row['subtests']:>9} {row['seconds']:.0f}s")
    if kills["shards"]:
        print(f"  total: {kills['rescued']} ids rescued scored {kills['score']}, so the "
              f"{kills['lost_ids']} lost ids project to ~{kills['projected_lost_score']} points")

    if clock.get("results"):
        print(f"\n== wall-clock ({clock['shards']} shards, {clock['results']} verdicts) ==")
        print(f"  seconds ~= {clock['seconds_per_timeout']}*TIMEOUT "
              f"+ {clock['seconds_per_resolved']}*resolved + {clock['seconds_per_shard_fixed']}")
        print(f"  {clock['hours_on_timeouts']} of {clock['hours_total']} hours "
              f"({clock['share_on_timeouts'] * 100:.0f}%) went to tests that returned nothing")
        for label, value in clock["median_seconds_per_id_by_timeout_share"].items():
            print(f"    timeout share {label:8} median {value:5.2f} s/id")

    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump({"accounting": acc, "kill_cost": kills, "wall_clock": clock}, fh,
                      indent=1, ensure_ascii=False)
        print(f"\nwritten: {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
