#!/usr/bin/env python3
"""Per-test comparison of two corpus-run snapshots (WPT-RUN-17).

    python tests/wpt/run_diff.py docs/wpt/runs/A.json docs/wpt/runs/B.json
    python tests/wpt/run_diff.py A.json B.json --prefix css/css-break,quirks
    python tests/wpt/run_diff.py A.json B.json --json --min-delta 0.2

Both snapshots must carry an `ids_file` (`run_corpus.py --run-json` writes it
next to the snapshot, format in `ids_file.py`). The aggregates in the `.json`
say *that* a category fell; this says *which tests* did it, with the status
each moved between (`PASS->FAIL`, `OK->TIMEOUT`) and the score difference,
where the score is the one `score_reports` added to the category — so the
per-category sums here are the snapshot's own `per_category.score` deltas.

A test present in only one snapshot (the corpus changed, or a prefix run
covered less) counts as `(absent)` with score 0 on the other side, which makes
the category deltas add up to the difference of the two denominators' scores.

Exit status: 0 always when the comparison ran (a regression is a finding, not
an error); 2 when a snapshot has no usable `ids_file`.
"""

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ids_file  # noqa: E402

ABSENT = "(absent)"
#: A score difference below this is float noise from the 4-digit rounding in
#: the ids file, never a real change.
EPSILON = 1e-4


class NoIdsFile(Exception):
    """The snapshot has no per-test data to compare."""


def load_snapshot(path: str) -> tuple:
    """`(snapshot dict, {id: row})` for a `--run-json` snapshot."""
    with open(path, encoding="utf-8") as fh:
        snapshot = json.load(fh)
    name = snapshot.get("ids_file")
    if not name:
        raise NoIdsFile(f"{path}: the snapshot has no `ids_file` — it predates WPT-RUN-17, "
                        f"per-test results were not kept")
    ids_path = os.path.join(os.path.dirname(os.path.abspath(path)), name)
    if not os.path.isfile(ids_path):
        raise NoIdsFile(f"{path}: `ids_file` {name} is missing next to the snapshot")
    return snapshot, ids_file.read(ids_path)


def category_of(test_id: str) -> str:
    """The manifest's category of an id: its first path segment (a root-level
    file is its own category). A `?variant` belongs to the file."""
    path = test_id.lstrip("/")
    for sep in ("?", "#"):
        path = path.split(sep, 1)[0]
    return path.split("/", 1)[0]


def under(test_id: str, prefixes: list) -> bool:
    """Whether `test_id` lies under any of `prefixes` (no leading/trailing
    slash); no prefixes selects everything. Same rule as `run_corpus._under`."""
    if not prefixes:
        return True
    path = test_id.lstrip("/")
    return any(path == p or path.startswith((p + "/", p + "?")) for p in prefixes)


def parse_prefixes(text) -> list:
    if not text:
        return []
    return [p.strip().strip("/") for p in text.split(",") if p.strip().strip("/")]


def _side(row) -> dict:
    return row or {"type": "", "status": ABSENT, "passed": 0, "total": 0, "score": 0.0, "shard": ""}


def compare(old: dict, new: dict, prefixes: list = None, min_delta: float = 0.0) -> dict:
    """Diff two `{id: row}` tables.

    Returns `{"categories": {name: {...}}, "worse": [...], "better": [...],
    "moved": [...], "totals": {...}}`. Entries are sorted most-changed first.
    `moved` holds tests whose status changed at an unchanged score (`FAIL ->
    TIMEOUT`): they do not move the pass rate but they are what a flaky or
    resource-starved run looks like. `min_delta` hides score changes smaller
    than it (it never hides a status-only move, which has no score to compare).
    """
    categories, worse, better, moved = {}, [], [], []
    old_total = new_total = 0.0
    for test_id in sorted(set(old) | set(new)):
        if not under(test_id, prefixes or []):
            continue
        a, b = _side(old.get(test_id)), _side(new.get(test_id))
        delta = round(b["score"] - a["score"], 4)
        old_total += a["score"]
        new_total += b["score"]
        row = categories.setdefault(category_of(test_id), {
            "old_score": 0.0, "new_score": 0.0, "worse": 0, "better": 0, "moved": 0})
        row["old_score"] += a["score"]
        row["new_score"] += b["score"]
        entry = {"id": test_id, "category": category_of(test_id), "type": b["type"] or a["type"],
                 "old_status": a["status"], "new_status": b["status"],
                 "old_subtests": f"{a['passed']}/{a['total']}",
                 "new_subtests": f"{b['passed']}/{b['total']}",
                 "old_score": a["score"], "new_score": b["score"], "delta": delta,
                 "shard": b["shard"] or a["shard"]}
        if abs(delta) > EPSILON:
            if abs(delta) < min_delta:
                continue
            (better if delta > 0 else worse).append(entry)
            row["better" if delta > 0 else "worse"] += 1
        elif a["status"] != b["status"]:
            moved.append(entry)
            row["moved"] += 1
    for row in categories.values():
        row["old_score"] = round(row["old_score"], 2)
        row["new_score"] = round(row["new_score"], 2)
        row["delta"] = round(row["new_score"] - row["old_score"], 2)
    worse.sort(key=lambda e: (e["delta"], e["id"]))
    better.sort(key=lambda e: (-e["delta"], e["id"]))
    moved.sort(key=lambda e: e["id"])
    return {"categories": categories, "worse": worse, "better": better, "moved": moved,
            "totals": {"old_score": round(old_total, 2), "new_score": round(new_total, 2),
                       "delta": round(new_total - old_total, 2),
                       "worse": len(worse), "better": len(better), "moved": len(moved)}}


def transition(entry: dict) -> str:
    """`OK 3/5 -> OK 1/5`, or `PASS -> FAIL` when neither side has subtests."""
    def side(status, subtests):
        return status if subtests == "0/0" else f"{status} {subtests}"
    return (f"{side(entry['old_status'], entry['old_subtests'])} -> "
            f"{side(entry['new_status'], entry['new_subtests'])}")


def worst_tests(old: dict, new: dict, category: str, limit: int = 10) -> list:
    """The `limit` tests of `category` that lost the most score — what
    `quality_report.py` prints under a fallen category."""
    result = compare(old, new, [category])
    return result["worse"][:limit]


def render(result: dict, limit: int, show_moved: bool) -> str:
    out = []
    totals = result["totals"]
    out.append(f"score {totals['old_score']} -> {totals['new_score']} ({totals['delta']:+.2f}); "
               f"{totals['worse']} tests worse, {totals['better']} better, "
               f"{totals['moved']} changed status at the same score")
    changed = [(name, row) for name, row in result["categories"].items()
               if row["worse"] or row["better"] or row["moved"]]
    changed.sort(key=lambda item: (item[1]["delta"], item[0]))
    if changed:
        out += ["", "categories (score old -> new, tests worse / better / moved):"]
        for name, row in changed:
            out.append(f"  {name}: {row['old_score']} -> {row['new_score']} ({row['delta']:+.2f})  "
                       f"{row['worse']} / {row['better']} / {row['moved']}")
    for title, key in (("WORSE", "worse"), ("BETTER", "better")) + ((("MOVED", "moved"),) if show_moved else ()):
        entries = result[key]
        if not entries:
            continue
        out += ["", f"{title} ({len(entries)}{', first ' + str(limit) if len(entries) > limit else ''}):"]
        for entry in entries[:limit]:
            tail = f"  {entry['delta']:+.2f}" if key != "moved" else ""
            out.append(f"  {entry['id']}  {transition(entry)}{tail}")
    if result["moved"] and not show_moved:
        out += ["", f"({len(result['moved'])} more tests changed status at the same score "
                    f"— --show-moved)"]
    return "\n".join(out)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("old", help="older snapshot (docs/wpt/runs/*.json)")
    parser.add_argument("new", help="newer snapshot")
    parser.add_argument("--prefix", default=None,
                        help="comma-separated path prefixes relative to the WPT root "
                             "(`css/css-break`, `quirks`); default: everything")
    parser.add_argument("--min-delta", type=float, default=0.0,
                        help="hide score changes smaller than this (0.5 hides a subtest "
                             "flipping in a two-subtest test)")
    parser.add_argument("--limit", type=int, default=20, help="tests listed per section (default 20)")
    parser.add_argument("--show-moved", action="store_true",
                        help="also list tests whose status changed at an unchanged score")
    parser.add_argument("--json", action="store_true", help="machine-readable output")
    args = parser.parse_args(argv)

    try:
        _a, old = load_snapshot(args.old)
        _b, new = load_snapshot(args.new)
    except NoIdsFile as exc:
        print(f"run_diff: {exc}", file=sys.stderr)
        return 2
    result = compare(old, new, parse_prefixes(args.prefix), args.min_delta)
    if args.json:
        json.dump(result, sys.stdout, indent=2, sort_keys=True)
        print()
    else:
        print(render(result, args.limit, args.show_moved))
    return 0


if __name__ == "__main__":
    sys.exit(main())
