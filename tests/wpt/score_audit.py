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

Slice 16 adds the same audit for a number that arrives *without* its run
directory — a snapshot published by another machine, which is all a reader of
`docs/wpt/pass-rate.md` ever gets. Same three-way split, plus two things only a
second run can answer: whether the two agree where their coverage overlaps
(they must — the engine is deterministic), and what they are worth added up.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/score_audit.py [--out-dir .tmp/wpt-corpus]
        [--manifest PATH] [--json OUT]
    <venv>/python tests/wpt/score_audit.py --snapshot docs/wpt/runs/A.json
        [--compare docs/wpt/runs/B.json] [--json OUT]
    <venv>/python tests/wpt/score_audit.py --selftest

Slice 24 adds one distinction to the snapshot mode: a shard that produced no
verdicts because it holds no test type this harness can execute is separated
from one that produced none although it should have. Both look identical in a
snapshot (same empty report, same exit code 64), so the manifest decides —
without it the tool called 14 benign shards of the Linux half and 15 of the
Windows half "HOLLOW", the same word its real 143-shard hole gets.

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
    by_shard = collections.Counter()
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
            continue
        if outcome[name] != "ran":
            label = "shard-killed"
        elif name.replace("/", "__") in empty_set:
            # `empty` (from `load_results`) holds on-disk report names, which
            # replace "/" with "__" the same way run_corpus.py's own report
            # paths do; `name` here is the state.json shard name and keeps
            # the "/". Comparing the two forms directly always misses on any
            # multi-segment category, misclassifying a legitimately empty
            # shard as a leak.
            label = "shard-empty"
        else:
            label = "lost-in-ran-shard"
            if len(leaked) < 50:
                leaked.append({"type": test_type, "category": category, "id": test_id,
                               "shard": name})
        cause[label] += 1
        # Which shard lost them is the actionable half: "503 ids lost" is a
        # number, "503 ids lost by these six shards, all killed on their
        # budget" is a decision about whether the run is publishable. The
        # cause travels with the shard because one `ran` shard losing ids is
        # a defect while another is a known empty report, and a reader must
        # not have to guess which.
        by_shard[(name, outcome[name], label)] += 1
    lost_by_shard = [{"shard": name, "outcome": shard_outcome, "cause": label, "ids": count}
                     for (name, shard_outcome, label), count in
                     sorted(by_shard.items(), key=lambda kv: -kv[1])]
    return {"per_type": dict(per_type), "not_run_by_type": dict(not_run_type),
            "cause": dict(cause), "lost_by_shard": lost_by_shard,
            "leaked_examples": leaked}


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


def runnable_ids_per_shard(shards: list, manifest: dict = None) -> dict:
    """`shard name -> how many ids of a type this harness can execute it holds`.

    A shard that produced no verdicts is not automatically a hole: a directory
    whose only automatable tests are `crashtest`/`print-reftest`/`aamtest` has
    nothing wptrunner will run for lumen, so it answers "No tests ran" and
    leaves an empty report *by construction* (WPT-RUN-5 slice 24 — corpus-wide
    6 shards / 50 ids). Telling that apart from a shard that should have run
    and did not needs the manifest, because the two are indistinguishable in a
    snapshot: the run's own `outcome` says `no-tests` only for runs scored
    since slice 16, and the exit code cannot be used at all — `64` merely means
    something logged CRITICAL, which on a machine where the shard cannot bind
    its own ports is *every* shard (339 of 358 rc=64 shards in the Linux half
    carried full results).

    Counting is by prefix and deliberately generous: a parent prefix also
    counts the ids of the sub-shards split out of it, so the answer errs
    towards "this shard did hold runnable tests" — the direction that keeps an
    alarm rather than silences one. `test_ids` shards (the bare-directory
    case) are exact.
    """
    if manifest is None:
        try:
            manifest = run_corpus.load_manifest()
        except (OSError, ValueError):
            return {}
    executable = corpus_stats.supported_types()
    by_category = collections.defaultdict(list)
    for test_type, category, test_id in corpus_stats.iter_ids(manifest):
        if test_type in executable:
            by_category[category].append(test_id)

    counts = {}
    for shard in shards:
        category = shard["name"].split(" ")[0].split("/")[0]
        ids = by_category.get(category, ())
        explicit = shard.get("test_ids")
        if explicit:
            wanted = set(explicit)
            counts[shard["name"]] = sum(1 for test_id in ids if test_id in wanted)
        elif shard.get("prefix"):
            prefix = shard["prefix"]
            counts[shard["name"]] = sum(1 for test_id in ids if test_id.startswith(prefix))
        else:
            # No way to attribute ids to it — leave it unclassified, which
            # keeps it in the alarming bucket.
            counts[shard["name"]] = None
    return counts


def snapshot_accounting(snapshot: dict, manifest: dict = None) -> dict:
    """Decompose a published run snapshot's zero bucket without its run directory.

    `accounting` above needs the shard reports; a number published by *another
    machine* arrives as `docs/wpt/runs/<date>.json` alone, and that is exactly
    the number a reader is asked to believe. Everything needed is in there:
    `per_category.by_type` gives the no-executor ceiling, the shard list gives
    which shards died, and `empty_shards` gives which produced no verdicts at
    all. What is left over after those three is a hole nobody has explained.

    Attribution is per category, because a snapshot records `not_run` per
    category and shard membership by name — so a category's unexplained
    remainder is exact, while its split between "empty shard" and "killed
    shard" is an upper bound on each (a shard's `ids` counts non-automatable
    ids too). The remainder is what the stop-signal is about, and it is not
    affected by that.

    The no-executor ceiling is read from the harness as it is *now*
    (`corpus_stats.supported_types`), so a snapshot taken before an executor
    landed has those ids counted as unexplained rather than as ceiling. That is
    the honest reading — `docs/wpt/runs/2026-08-18.json` predates TEST-4 and its
    24 260 css reftests really did sit at zero — but it is not a defect of that
    run, and the audit of an old number has to be read with its date in hand.
    """
    scored = snapshot["scored"]
    shards = snapshot.get("shards", [])
    executable = corpus_stats.supported_types()
    # `empty_shards` names shards by their *report file*, so `/` is already
    # `__` there while the shard list keeps the real category path.
    empty = set(scored.get("empty_shards", []))
    # A shard with nothing this harness can execute produces an empty report on
    # purpose, and calling that "hollow" next to a run whose shards really were
    # lost (the Windows half's 158) is the tool crying wolf on its own output.
    runnable = runnable_ids_per_shard(shards, manifest)
    for shard in shards:
        produced_nothing = shard["name"].replace("/", "__") in empty
        shard["runnable_ids"] = runnable.get(shard["name"])
        shard["no_runnable_type"] = bool(
            produced_nothing
            and (runnable.get(shard["name"]) == 0 or shard.get("outcome") == "no-tests"))
        shard["hollow"] = produced_nothing and not shard["no_runnable_type"]
    by_category = collections.defaultdict(list)
    for shard in shards:
        by_category[shard["name"].split("/")[0]].append(shard)

    cause = collections.Counter()
    unexplained = []
    for category, row in scored["per_category"].items():
        missing = row["not_run"]
        if not missing:
            continue
        no_executor = sum(n for t, n in row["by_type"].items()
                          if t not in executable and t not in corpus_stats.NON_AUTOMATABLE_TYPES)
        taken = min(no_executor, missing)
        cause["no-executor-for-type"] += taken
        missing -= taken
        for label, pick in (("shard-produced-nothing", lambda s: s["hollow"]),
                            ("shard-killed", lambda s: not s["hollow"] and s["outcome"] != "ran"),
                            ("shard-ended-by-signal",
                             lambda s: not s["hollow"] and s["outcome"] == "ran"
                             and s.get("returncode") not in run_corpus.WPTRUNNER_RETURNCODES)):
            if not missing:
                break
            ids = sum(s.get("auto_ids") or s["ids"] for s in by_category[category] if pick(s))
            taken = min(ids, missing)
            cause[label] += taken
            missing -= taken
        if missing:
            cause["unexplained"] += missing
            unexplained.append((category, missing))
    return {"cause": dict(cause),
            "unexplained_by_category": sorted(unexplained, key=lambda kv: -kv[1]),
            "hollow_shards": [s for s in shards if s["hollow"]],
            "no_runnable_type_shards": [s for s in shards if s["no_runnable_type"]],
            "signalled_shards": [s for s in shards if not s["hollow"] and s["outcome"] == "ran"
                                 and s.get("returncode") not in run_corpus.WPTRUNNER_RETURNCODES]}


def compare_snapshots(left: dict, right: dict) -> dict:
    """Do two runs of the same corpus agree, and what do they add up to together?

    Two machines running disjoint-ish halves of the corpus cannot have their
    headline numbers compared — different coverage, different moment — but they
    *can* be checked against each other on the categories both reached, and
    that check is what decides whether the halves may be added up at all:

    * **agreement** — a category both ran to the same depth must produce the
      same numbers, since the engine is deterministic. Categories whose every
      counter matches are reported as such; the rest are ranked by how far the
      per-executed-id score drifts, which is the platform-independent rate.
    * **fusion** — per category, the run that executed more ids is the better
      observation of it, so `max(ran)` per category is a defensible joint
      figure. Reported next to both halves rather than instead of them.
    """
    lp, rp = left["scored"]["per_category"], right["scored"]["per_category"]
    shared = sorted(set(lp) & set(rp))
    identical = [c for c in shared if all(lp[c][k] == rp[c][k] for k in
                                          ("ids", "ran", "not_run", "harness_ok",
                                           "subtests_passed", "score"))]
    drift = []
    for c in shared:
        a, b = lp[c], rp[c]
        if c in identical or not (a["ran"] and b["ran"]):
            continue
        drift.append({"category": c, "ids": a["ids"], "left_ran": a["ran"], "right_ran": b["ran"],
                      "left_per_ran": round(a["score"] / a["ran"], 4),
                      "right_per_ran": round(b["score"] / b["ran"], 4)})
    drift.sort(key=lambda d: -abs(d["left_per_ran"] - d["right_per_ran"]))

    fused = {c: (rp[c] if c in rp and rp[c]["ran"] > lp[c]["ran"] else lp[c]) for c in lp}
    for c in rp:
        fused.setdefault(c, rp[c])

    def totals(rows):
        score = sum(v["score"] for v in rows.values())
        ids = sum(v["ids"] for v in rows.values())
        ran = sum(v["ran"] for v in rows.values())
        return {"score": round(score, 1), "ids": ids, "ran": ran,
                "pass_rate": round(score / ids, 4) if ids else 0.0,
                "executed_rate": round(score / ran, 4) if ran else 0.0,
                "coverage": round(ran / ids, 4) if ids else 0.0}

    return {"shared_categories": len(shared), "identical": len(identical),
            "drift": drift[:15],
            "shared_totals": {"left": totals({c: lp[c] for c in shared}),
                              "right": totals({c: rp[c] for c in shared})},
            "left": totals(lp), "right": totals(rp), "fused": totals(fused),
            "fused_from_right": sorted(c for c in fused if fused[c] is rp.get(c))}


def print_snapshot_audit(path: str, snapshot: dict) -> dict:
    """Print the zero-bucket decomposition of one published snapshot."""
    scored = snapshot["scored"]
    totals = scored["totals"]
    acc = snapshot_accounting(snapshot)
    print(f"== {path} ==")
    print(f"  commit {snapshot.get('commit')}, binary {snapshot.get('binary')}, "
          f"{len(snapshot.get('shards', []))} shards, finished {snapshot.get('finished')}")
    print(f"  headline {totals['pass_rate'] * 100:.2f}%  "
          f"({totals['score']:.0f} of {totals['ids']} ids), executed {totals['ran']} "
          f"({totals['ran'] / totals['ids'] * 100:.1f}% of the denominator)")
    print("  ids with no verdict, by cause:")
    for name, n in sorted(acc["cause"].items(), key=lambda kv: -kv[1]):
        print(f"    {name:24} {n:6}")
    # A snapshot scored by run_corpus.py since WPT-RUN-5 slice 19 carries the
    # per-shard attribution the run itself computed, which is exact — this
    # function's own split is a per-category upper bound (see
    # `snapshot_accounting`). Print the exact figure next to it when it is
    # there rather than replacing the approximation, so the two can be read
    # against each other on the same snapshot.
    exact = scored.get("coverage")
    if exact:
        acc["exact_coverage"] = exact
        print(f"    recorded by the run itself: {exact['no_executor']} with no executor, "
              f"{exact['lost']} lost in {len(exact['lost_by_shard'])} shard(s), "
              f"{exact['lost_in_ran_shards']} of those inside a shard that reported success")
    hollow = acc["hollow_shards"]
    if hollow:
        ids = sum(s["ids"] for s in hollow)
        # `ids` is the shard's whole manifest count; only the runnable part of
        # it could ever have carried a verdict, so that is the size of the hole.
        runnable_lost = sum(s["runnable_ids"] or 0 for s in hollow)
        print(f"    HOLLOW SHARDS: {len(hollow)} shards ({ids} ids, {runnable_lost} of them "
              f"runnable) produced no verdicts; "
              f"median {statistics.median(s['seconds'] for s in hollow):.0f}s")
        for shard in sorted(hollow, key=lambda s: -s["ids"])[:5]:
            runnable = shard.get("runnable_ids")
            print(f"      {shard['name']:28} ids={shard['ids']:5} "
                  f"runnable={'?' if runnable is None else runnable:>5} "
                  f"rc={shard['returncode']} {shard['seconds']:.0f}s")
    benign = acc["no_runnable_type_shards"]
    if benign:
        print(f"    no runnable test type: {len(benign)} shards "
              f"({sum(s['ids'] for s in benign)} ids) hold nothing this harness executes, so an "
              f"empty report is what they are supposed to produce — not a hole "
              f"(the ids are already in the no-executor ceiling)")
    signalled = acc["signalled_shards"]
    if signalled:
        print(f"    ENDED BY SIGNAL: {len(signalled)} shards ({sum(s['ids'] for s in signalled)} ids) "
              f"exited on a code wptrunner never returns, so they were stopped from outside and "
              f"only what their raw stream held was scored")
        for shard in sorted(signalled, key=lambda s: -s["ids"])[:5]:
            print(f"      {shard['name']:28} ids={shard['ids']:5} rc={shard['returncode']} "
                  f"{shard['seconds']:.0f}s")
    if acc["cause"].get("unexplained"):
        print(f"    UNEXPLAINED: {acc['cause']['unexplained']} ids sit at zero with no cause — "
              f"worst: " + ", ".join(f"{c}({n})" for c, n in acc["unexplained_by_category"][:5]))
    return acc


def print_comparison(cmp: dict) -> None:
    """Print the agreement and fusion of two snapshots."""
    print("\n== two runs against each other ==")
    print(f"  {cmp['shared_categories']} shared categories, {cmp['identical']} identical "
          f"in every counter")
    left, right = cmp["shared_totals"]["left"], cmp["shared_totals"]["right"]
    print(f"  on shared coverage: left {left['score']:.0f}/{left['ran']} executed ids "
          f"= {left['executed_rate'] * 100:.2f}%, right {right['score']:.0f}/{right['ran']} "
          f"= {right['executed_rate'] * 100:.2f}%")
    for row in cmp["drift"][:8]:
        print(f"    {row['category']:24} ran {row['left_ran']:6}/{row['right_ran']:6}  "
              f"per-executed-id {row['left_per_ran']:.3f} vs {row['right_per_ran']:.3f}")
    fused = cmp["fused"]
    print(f"  fused (per category, whichever run executed more): "
          f"{fused['pass_rate'] * 100:.2f}% of the manifest "
          f"({fused['score']:.0f}/{fused['ids']}), coverage {fused['coverage'] * 100:.1f}%, "
          f"executed-id rate {fused['executed_rate'] * 100:.2f}%")
    print(f"  categories the fusion took from the second run: {len(cmp['fused_from_right'])}")


def _selftest() -> int:
    """Prove the empty-shard classifier on a hand-made snapshot.

    The case it has to get right — a shard whose whole content is a type this
    harness cannot execute — is indistinguishable from a lost shard by exit
    code (`64` only means something logged CRITICAL) and, on any run scored
    before slice 16, by `outcome` too. So the discriminator is the manifest,
    and the four ways it can answer are checked here rather than waited for on
    an 11-hour run.
    """
    def leaf():
        return ["hash", [None, {}]]

    manifest = {"items": {
        "testharness": {"dom": {"ok.html": leaf()}, "referrer-policy": {"r.html": leaf()}},
        "crashtest": {"print": {"c.html": leaf()}},
        "manual": {"appmanifest": {"m.html": leaf()}},
    }}
    shards = [
        {"name": "print", "prefix": "/print/", "ids": 1, "outcome": "ran", "returncode": 64,
         "seconds": 3},
        {"name": "referrer-policy", "prefix": "/referrer-policy/", "ids": 1, "outcome": "ran",
         "returncode": 64, "seconds": 12},
        {"name": "appmanifest", "prefix": "/appmanifest/", "ids": 1, "outcome": "no-tests",
         "returncode": 64, "seconds": 3},
        {"name": "dom", "prefix": "/dom/", "ids": 1, "outcome": "ran", "returncode": 1,
         "seconds": 20},
    ]
    snapshot = {"shards": shards, "scored": {
        "totals": {"ids": 3, "ran": 1, "score": 1.0, "pass_rate": 0.33},
        "empty_shards": ["print", "referrer-policy", "appmanifest"],
        "per_category": {
            "print": {"not_run": 1, "by_type": {"crashtest": 1}},
            "referrer-policy": {"not_run": 1, "by_type": {"testharness": 1}},
            "dom": {"not_run": 0, "by_type": {"testharness": 1}},
        }}}

    acc = snapshot_accounting(snapshot, manifest)
    hollow = {s["name"] for s in acc["hollow_shards"]}
    benign = {s["name"] for s in acc["no_runnable_type_shards"]}
    checks = [
        ("no-executor-only shard is not called hollow", "print" in benign),
        ("shard holding runnable tests stays hollow", hollow == {"referrer-policy"}),
        ("`no-tests` outcome is believed without the manifest", "appmanifest" in benign),
        ("a shard that produced verdicts is neither", "dom" not in hollow | benign),
        ("hollow shard carries its runnable count",
         acc["hollow_shards"][0]["runnable_ids"] == 1),
        ("benign shard ids stay in the no-executor ceiling",
         acc["cause"].get("no-executor-for-type") == 1),
        ("hole is still counted as a hole",
         acc["cause"].get("shard-produced-nothing") == 1),
    ]
    for label, ok in checks:
        print(f"  {'PASS' if ok else 'FAIL'}  {label}")
    failed = [label for label, ok in checks if not ok]
    print(f"selftest: {'PASS' if not failed else 'FAIL (' + ', '.join(failed) + ')'}")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out-dir", default=".tmp/wpt-corpus",
                        help="run directory holding the shard reports and state.json")
    parser.add_argument("--manifest", default=None, help="MANIFEST.json (default: vendored)")
    parser.add_argument("--json", dest="json_out", default=None, help="write the full result here")
    parser.add_argument("--snapshot", default=None,
                        help="audit a published run snapshot (docs/wpt/runs/*.json) instead of a "
                             "run directory — the only thing another machine's number ships with")
    parser.add_argument("--compare", default=None,
                        help="second snapshot: check the two runs against each other on the "
                             "categories both reached, and report the fused figure")
    parser.add_argument("--selftest", action="store_true",
                        help="check the empty-shard classifier on a hand-made snapshot")
    args = parser.parse_args()

    if args.selftest:
        return _selftest()

    if args.snapshot:
        with open(args.snapshot, encoding="utf-8") as fh:
            snapshot = json.load(fh)
        result = {"snapshot": print_snapshot_audit(args.snapshot, snapshot)}
        if args.compare:
            with open(args.compare, encoding="utf-8") as fh:
                other = json.load(fh)
            print()
            result["compare_snapshot"] = print_snapshot_audit(args.compare, other)
            result["comparison"] = compare_snapshots(snapshot, other)
            print_comparison(result["comparison"])
        if args.json_out:
            with open(args.json_out, "w", encoding="utf-8") as fh:
                json.dump(result, fh, indent=1, ensure_ascii=False)
            print(f"\nwritten: {args.json_out}")
        return 0

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
    for entry in acc["lost_by_shard"][:10]:
        print(f"      lost by {entry['shard']} ({entry['cause']}): {entry['ids']}")
    if len(acc["lost_by_shard"]) > 10:
        print(f"      ... and {len(acc['lost_by_shard']) - 10} more shard(s)")
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
