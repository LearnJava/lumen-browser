#!/usr/bin/env python3
"""WPT-RUN-5: what does a reftest verdict actually rest on — pixels or an empty page?

Slice 14 starts from an arithmetic fact about the pass-rate rather than from a
suspected defect: of the score the partial run has accumulated, **76.8 % comes
from `reftest` PASSes** and only 23.2 % from testharness subtests. So whatever
the second headline number means, it means it about reftests, and the one way a
reftest can be scored without the engine rendering anything is a pair where both
sides come out empty.

Slices 10/12/13 measured one road to that state (the author stylesheet never
reaching the engine, [BUG-786](../../bugs/BUG-786-OPEN.md) and friends). This
audit does not ask *why* a page is empty and does not strip anything: it renders
the test and its reference as served and asks the display list whether either
painted a single item. That catches every road at once — a failed subresource, an
element the engine does not build a box for, a script that never ran, CSS that
never parsed — and it also classifies the FAIL side, which nothing has looked at
yet: a FAIL where we paint nothing against a reference full of content is a
different kind of debt than a FAIL where both sides paint and differ.

Buckets, per verdict, as `(test, reference)`:

* `blank/blank`  — on a PASS, a candidate vacuous PASS (see the bound below);
* `blank/content` — the engine painted nothing where the reference has pixels;
* `content/blank` — the reverse, usually the engine painting what should not be;
* `content/content` — a real comparison: both sides painted, the verdict is about
  the difference between them.

Reading the `blank/blank` PASS count as inflation needs one subtraction. A
reference that is *supposed* to be empty makes an empty test page the correct
result — the whole "must not render" family of CSS2.1 asserts exactly that. Those
are recognised mechanically (`ref_blank_by_design`) when the reference document
has no painting content in source at all; the remainder is an upper bound, in the
same sense slice 12's number is one.

`!=` reftests are excluded, not counted: there a blank/blank pair is a FAIL by
construction, so the question this audit asks does not apply to them.

Pages are served from a local static server, exactly as `inert_style_audit.py`
does and for the same reason (the corpus run holds the wptserve ports), so ids
needing `.sub.` substitution or server pipes land in `error` rather than being
mis-scored.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/blank_render_audit.py [--sample 400] [--control 400]
        [--jobs 2] [--json OUT]
"""

import argparse
import collections
import concurrent.futures
import json
import os
import random
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import cdata_audit
import corpus_stats
import inert_style_audit
import run_corpus

REPO_ROOT = inert_style_audit.REPO_ROOT
WPT_ROOT = inert_style_audit.WPT_ROOT

#: The display list is the audit's whole instrument: `--dump-display-list` prints
#: one line per paint item and nothing at all for a page that paints nothing, so
#: "did this page render anything" is exactly "is the dump non-empty". Layout is
#: deliberately NOT consulted here (unlike the inert-CSS audit, which needs it to
#: separate an applied-but-invisible declaration from a lost stylesheet): a
#: reftest is decided on pixels, and boxes that move without painting cannot
#: change its verdict.
DUMP_MODE = "--dump-display-list"


def reftest_refs(manifest: dict) -> dict:
    """`{test_id: (ref_url, relation)}` for every reftest in the manifest.

    Only the first reference of the first alternative is taken. A chained or
    multi-alternative reftest is rare in the vendored corpus and taking the head
    of the chain keeps the audit's question ("was there anything to compare")
    answerable without re-implementing wptrunner's ref resolution.
    """
    refs = {}
    items = manifest.get("items", {}).get("reftest", {})

    def walk(node, path):
        for key, value in node.items():
            if isinstance(value, dict):
                walk(value, path + (key,))
                continue
            entry = value[1] if len(value) > 1 else None
            if not entry or not isinstance(entry, list) or len(entry) < 2:
                continue
            ref_list = entry[1]
            if not ref_list:
                continue
            test_id = "/" + "/".join(path + (key,))
            refs[test_id] = (ref_list[0][0], ref_list[0][1])

    walk(items, ())
    return refs


#: Three states, not two. "Did the page paint anything at all" turned out to be
#: almost always yes and therefore almost blind: a WPT reftest carries its own
#: instructions ("Test passes if there is a green square"), and that sentence is a
#: `DrawText` item on both sides of the pair. So a page whose display list holds
#: nothing but text has painted its prose and none of its assertion — for every
#: reftest except the ones whose subject *is* text, that is as empty as a blank
#: page. `paint` means at least one item that is not text: a rect, an image, a
#: border, a shadow.
def page_state(out: bytes) -> str:
    """`blank` / `text` / `paint` for one display-list dump."""
    lines = [line for line in out.decode("utf-8", "replace").splitlines() if line.strip()]
    if not lines:
        return "blank"
    return "text" if all(line.lstrip().startswith("DrawText") for line in lines) else "paint"


def dump_state(binary: str, port: int, url_path: str, timeout: int) -> tuple:
    """`(state, dump)` for one document, or `("error", None)`."""
    if not url_path.startswith("/"):
        return "error", None
    out = inert_style_audit.dump(binary, DUMP_MODE,
                                 f"http://127.0.0.1:{port}{url_path}", timeout)
    if out is None:
        return "error", None
    return page_state(out), out


#: The audit's second instrument, and the one that decides whether an empty
#: verdict is *wrong*. It reads the REFERENCE, not the test — the test's own
#: source cannot answer the question. Hand-checking the first flagged sample
#: showed why: `css/CSS2/ui/outline-width-046.xht` sets `outline: solid red;
#: outline-width: 0mm`, so text-only output is the correct rendering and the
#: paint declaration in its source means the opposite of what it looks like.
#: References carry no such trap: the CSS2.1 "no red / filler text" family is
#: prose by construction, and a reference that wants pixels says so plainly with
#: a background, a border, an image or a table. So: reference asks for pixels and
#: we rendered prose -> we failed to render the reference too, and the pair
#: matched on the instruction text alone.
_TAG_RE = re.compile(r"<(img|canvas|svg|video|iframe|input|table|hr)\b", re.I)
_PAINT_DECL_RE = re.compile(
    r"(background(-color|-image)?|border(-\w+)?|outline|box-shadow|list-style|bgcolor)"
    r"\s*[:=]\s*[^;\"']*", re.I)
#: A zero-width or `none` paint declaration asks for no pixels. Left in the regex
#: above and filtered here so the two readings stay visible side by side.
_INERT_VALUE_RE = re.compile(r"(:|=)\s*(none|0\w*|transparent|inherit)\s*$", re.I)


def ref_wants_paint(ref_url: str) -> bool:
    """True if the reference document's source asks for something other than text."""
    source = inert_style_audit.source_of(ref_url)
    if source is None:
        return False
    if _TAG_RE.search(source):
        return True
    return any(not _INERT_VALUE_RE.search(match.group(0))
               for match in _PAINT_DECL_RE.finditer(source))


def cdata_class(test_id: str, ref_url: str) -> str:
    """Which side of the pair carries a CDATA-wrapped stylesheet."""
    test, ref = side_has_cdata(test_id), side_has_cdata(ref_url)
    if test and ref:
        return "both"
    if test:
        return "test_only"
    return "ref_only" if ref else "neither"


def classify(test_id: str, ref_url: str, binary: str, port: int, timeout: int) -> str:
    """`<test>/<ref>` state pair, or `error` if either document failed to dump.

    A pair that renders identically *and* paints nothing but text is reported as
    its own bucket (`text=text`) rather than as `text/text`: byte-equal dumps are
    what the reftest comparison itself saw, so that bucket is the one where the
    verdict was decided on prose alone.
    """
    test_state, test_dump = dump_state(binary, port, test_id.split("#")[0], timeout)
    if test_state == "error":
        return "error"
    ref_state, ref_dump = dump_state(binary, port, ref_url.split("#")[0], timeout)
    if ref_state == "error":
        return "error"
    if test_state == "text" and ref_state == "text" and test_dump == ref_dump:
        return "text=text"
    return f"{test_state}/{ref_state}"


def audit(pairs: list, binary: str, port: int, timeout: int, jobs: int) -> dict:
    """`classify` over `(test_id, ref_url)` pairs, in parallel."""
    verdicts = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = {pool.submit(classify, tid, ref, binary, port, timeout): tid
                   for tid, ref in pairs}
        done = 0
        for future in concurrent.futures.as_completed(futures):
            verdicts[futures[future]] = future.result()
            done += 1
            if done % 100 == 0:
                print(f"  … {done}/{len(pairs)}", file=sys.stderr, flush=True)
    return verdicts


BUCKETS = ("blank/blank", "text=text", "text/text", "blank/text", "text/blank",
           "blank/paint", "paint/blank", "text/paint", "paint/text",
           "paint/paint", "error")


def summarise(name: str, verdicts: dict) -> dict:
    """Print and return the bucket histogram of one group."""
    counts = collections.Counter(verdicts.values())
    total = sum(counts.values())
    print(f"\n{name}: {total} reftests")
    for bucket in BUCKETS:
        n = counts.get(bucket, 0)
        share = f"{100.0 * n / total:.1f} %" if total else "-"
        print(f"  {bucket:<16} {n:>6}  {share}")
    return dict(counts)


#: `--repair` mode. Everything above counts what the CDATA defect *reaches*;
#: this prices what it *does*. The server can hand back the same document with
#: the `<![CDATA[` markers taken out of its `<style>` blocks — the page as the
#: engine would see it with [BUG-786](../../bugs/BUG-786-OPEN.md) fixed — so
#: rendering both sides of a pair twice, as served and repaired, sorts the pair
#: into the verdict it has now and the verdict it would have after the fix.
#: Both directions matter and they pull opposite ways: a FAIL caused only by the
#: reference losing its stylesheet becomes a PASS (the fix RAISES the number), a
#: PASS that only existed because both sides came out equally bare becomes a FAIL
#: (the fix LOWERS it). No previous slice could tell those apart; slices 10/12/13
#: bounded the second one alone and left the standing caveat one-sided.
REPAIR_MOVES = ("stays_pass", "pass_to_fail", "fail_to_pass", "stays_fail", "error")


def with_flag(url_path: str, flag: str) -> str:
    """`url_path` with `flag` appended as a query parameter."""
    return url_path + ("&" if "?" in url_path else "?") + flag


def side_has_cdata(url_path: str) -> bool:
    """True if this document carries a CDATA marker inside `<style>`."""
    return bool(cdata_audit.has_style_cdata(url_path.split("?")[0]))


def repair_pair(test_id: str, ref_url: str, binary: str, port: int, timeout: int) -> str:
    """Which way the CDATA fix would move this pair's verdict.

    The verdict proxy is byte equality of the two display lists, not a screenshot
    diff: it is *sufficient* for a pixel-identical pair and not necessary, so a
    pair reported as moving is a pair that really moves, while a pair reported as
    staying may still move by a difference too small to reach the display list.
    Every count out of this function is therefore a lower bound, which is the
    direction a caveat about an inflated headline needs.
    """
    _, test_now = dump_state(binary, port, test_id.split("#")[0], timeout)
    _, ref_now = dump_state(binary, port, ref_url.split("#")[0], timeout)
    if test_now is None or ref_now is None:
        return "error"
    test_path, ref_path = test_id.split("#")[0], ref_url.split("#")[0]
    if side_has_cdata(test_path):
        _, test_fixed = dump_state(binary, port,
                                   with_flag(test_path, inert_style_audit.REPAIR_FLAG),
                                   timeout)
    else:
        test_fixed = test_now
    if side_has_cdata(ref_path):
        _, ref_fixed = dump_state(binary, port,
                                  with_flag(ref_path, inert_style_audit.REPAIR_FLAG),
                                  timeout)
    else:
        ref_fixed = ref_now
    if test_fixed is None or ref_fixed is None:
        return "error"
    now, fixed = test_now == ref_now, test_fixed == ref_fixed
    if now and fixed:
        return "stays_pass"
    if now and not fixed:
        return "pass_to_fail"
    if fixed and not now:
        return "fail_to_pass"
    return "stays_fail"


def main_repair(args, refs: dict, verdicts: dict) -> int:
    """Price BUG-786 on a sample of the pairs it touches, in both directions."""
    population = collections.Counter()
    pairs = {"PASS": [], "FAIL": []}
    for tid, status in verdicts.items():
        if status not in pairs:
            continue
        ref = refs.get(tid) or refs.get(tid.split("?")[0])
        if not ref or ref[1] != "==":
            continue
        if not (side_has_cdata(tid) or side_has_cdata(ref[0])):
            continue
        population[status] += 1
        pairs[status].append((tid, ref[0]))
    rng = random.Random(args.seed)
    for status in pairs:
        pairs[status].sort()
        if args.repair and args.repair < len(pairs[status]):
            pairs[status] = sorted(rng.sample(pairs[status], args.repair))
    print(f"executed pairs with CDATA on either side: {population['PASS']} PASS, "
          f"{population['FAIL']} FAIL "
          f"(auditing {len(pairs['PASS'])} and {len(pairs['FAIL'])})")

    server, port = inert_style_audit.start_server()
    try:
        moves = {}
        for status in ("PASS", "FAIL"):
            with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
                futures = {pool.submit(repair_pair, tid, ref, args.binary, port,
                                       args.timeout): tid
                           for tid, ref in pairs[status]}
                result = {}
                for future in concurrent.futures.as_completed(futures):
                    result[futures[future]] = future.result()
                    if len(result) % 50 == 0:
                        print(f"  … {status} {len(result)}/{len(futures)}",
                              file=sys.stderr, flush=True)
            moves[status] = result
    finally:
        server.shutdown()

    summary = {}
    for status in ("PASS", "FAIL"):
        counts = collections.Counter(moves[status].values())
        audited = sum(n for move, n in counts.items() if move != "error")
        print(f"\nrecorded {status} ({audited} audited, {counts.get('error', 0)} error):")
        for move in REPAIR_MOVES:
            if move == "error":
                continue
            n = counts.get(move, 0)
            share = f"{100.0 * n / audited:.1f} %" if audited else "-"
            print(f"  {move:<14} {n:>5}  {share}")
        # The proxy and the recorded verdict do not always agree, and where they
        # disagree the pair says nothing: a run PASS whose display lists differ
        # is a pair whose difference did not reach the pixels the reftest
        # compares, so this instrument cannot see its move either way. Rates are
        # therefore quoted twice — over the pairs where the proxy reproduces the
        # recorded verdict (what the fix does to pairs we can read), and over
        # everything audited (a lower bound, counting every unreadable pair as a
        # non-mover).
        moved_key = "pass_to_fail" if status == "PASS" else "fail_to_pass"
        held_key = "stays_pass" if status == "PASS" else "stays_fail"
        moved = counts.get(moved_key, 0)
        agree = moved + counts.get(held_key, 0)
        summary[status] = {"counts": dict(counts), "audited": audited,
                           "agreeing": agree, "moved": moved,
                           "population": population[status]}
        if agree:
            print(f"  proxy reproduces the recorded verdict on {agree} of {audited} "
                  f"pairs; of those {moved} move ({100.0 * moved / agree:.1f} %)")
        if audited:
            print(f"  -> over {population[status]} such pairs in the run: at least "
                  f"{round(population[status] * moved / audited)} verdicts move, "
                  f"{round(population[status] * moved / agree) if agree else 0} if the "
                  f"unreadable pairs behave like the readable ones")
    for status, key in (("PASS", "pass_to_fail"), ("FAIL", "fail_to_pass")):
        for tid, move in sorted(moves[status].items()):
            if move == key:
                print(f"  e.g. {key}: {tid}")
                break
    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump({"summary": summary, "moves": moves}, fh, indent=2)
        print(f"\nwrote {args.json}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", default=cdata_audit.DEFAULT_MANIFEST)
    parser.add_argument("--out-dir", default=os.path.join(REPO_ROOT, ".tmp", "wpt-corpus"),
                        help="run directory with per-shard wptreport.json files")
    parser.add_argument("--binary", default=os.path.join("target", "dev-release", "lumen"))
    parser.add_argument("--sample", type=int, default=400,
                        help="seeded random sample of PASSes to audit (0 = all). Two "
                             "dumps per id at ~1 s each, on a machine that is also "
                             "running the corpus it reads — a sample is what keeps the "
                             "measurement from perturbing what it measures")
    parser.add_argument("--control", type=int, default=400,
                        help="size of the reftest-FAIL sample (0 = skip). Not a control "
                             "in the slice-12 sense: the FAIL split is a result here, "
                             "not a noise floor")
    parser.add_argument("--jobs", type=int, default=2,
                        help="concurrent dumps; keep low while a corpus run is in flight")
    parser.add_argument("--timeout", type=int, default=30, help="seconds per dump")
    parser.add_argument("--seed", type=int, default=814, help="sample seed")
    parser.add_argument("--repair", type=int, default=0, metavar="N",
                        help="instead of the blank audit, price BUG-786: render both "
                             "sides of N recorded-PASS and N recorded-FAIL pairs as "
                             "served and with the CDATA markers removed, and report "
                             "which verdicts the fix would move (0 = blank audit)")
    parser.add_argument("--json", default=None, help="write the numbers here as JSON")
    args = parser.parse_args()

    with open(args.manifest, encoding="utf-8") as fh:
        manifest = json.load(fh)
    refs = reftest_refs(manifest)
    verdicts = inert_style_audit.reftest_verdicts(args.out_dir, manifest)
    if args.repair:
        return main_repair(args, refs, verdicts)

    def pairs_for(status):
        out = []
        for tid, st in verdicts.items():
            if st != status:
                continue
            ref = refs.get(tid) or refs.get(tid.split("?")[0])
            if not ref or ref[1] != "==":
                continue
            out.append((tid, ref[0]))
        return sorted(out)

    passes, fails = pairs_for("PASS"), pairs_for("FAIL")
    excluded = sum(1 for tid, st in verdicts.items()
                   if (refs.get(tid) or ("", "=="))[1] == "!=")
    rng = random.Random(args.seed)
    if args.sample and args.sample < len(passes):
        passes = sorted(rng.sample(passes, args.sample))
    if not args.control:
        fails = []
    elif args.control < len(fails):
        fails = sorted(rng.sample(fails, args.control))
    print(f"executed reftests in {args.out_dir}: {len(verdicts)} "
          f"({len(passes)} PASS and {len(fails)} FAIL audited, {excluded} `!=` excluded)")

    server, port = inert_style_audit.start_server()
    try:
        pass_verdicts = audit(passes, args.binary, port, args.timeout, args.jobs)
        fail_verdicts = audit(fails, args.binary, port, args.timeout, args.jobs)
    finally:
        server.shutdown()

    pass_counts = summarise("PASS", pass_verdicts)
    fail_counts = summarise("FAIL", fail_verdicts)

    ref_of = dict(passes)
    empty_buckets = ("blank/blank", "text=text")
    empty = sorted(tid for tid, b in pass_verdicts.items() if b in empty_buckets)
    prose_ref = [tid for tid in empty if not ref_wants_paint(ref_of[tid])]
    rest = [tid for tid in empty if tid not in prose_ref]
    negative = [tid for tid in rest if inert_style_audit.is_negative_test(tid)]
    vacuous = sorted(tid for tid in rest if tid not in negative)
    audited = sum(n for b, n in pass_counts.items() if b != "error")
    print(f"\nof the {len(empty)} PASSes decided on an empty or text-only rendering, "
          f"{len(prose_ref)} have a reference that asks for no pixels either (the "
          f"CSS2.1 \"no red, filler text\" family — a correct verdict) and "
          f"{len(negative)} more declare `flags=invalid`, i.e. inert output is the "
          f"assertion. Upper bound on vacuous reftest PASS: {len(vacuous)} of "
          f"{audited} audited ({100.0 * len(vacuous) / max(audited, 1):.1f} %)")

    # Two-sided on purpose. `cdata_audit.classify` reads the TEST document only,
    # so a plain `.html` test comparing against an `.xht` reference — the single
    # commonest shape in the corpus, 4 704 manifest ids against 539 shared CSS2.1
    # references — comes back `plain` and reads as "not BUG-786" when the bug is
    # exactly what blanked the reference. Slice 14 found that by hand and the
    # split is computed on both sides from here on.
    cdata_split = collections.Counter(
        cdata_class(tid, ref_of[tid]) for tid in vacuous)
    print(f"\nthe {len(vacuous)} by CDATA class (`neither` = a mechanism other than "
          f"BUG-786):")
    for name, n in cdata_split.most_common():
        print(f"  {name:<12} {n:>6}")
    by_category = collections.Counter(inert_style_audit.category_of(tid) for tid in vacuous)
    print("\ntop categories:")
    for name, n in by_category.most_common(10):
        print(f"  {name:<24} {n:>6}")
    for tid in vacuous[:10]:
        print(f"  e.g. {tid}  (ref {ref_of[tid]})")

    starved = sorted(tid for tid, b in fail_verdicts.items()
                     if b in ("blank/paint", "text/paint", "blank/text"))
    fail_audited = sum(n for b, n in fail_counts.items() if b != "error")
    print(f"\nFAILs where the engine painted strictly less kind of content than the "
          f"reference (nothing, or prose against pixels): {len(starved)} of "
          f"{fail_audited} ({100.0 * len(starved) / max(fail_audited, 1):.1f} %)")
    starved_categories = collections.Counter(inert_style_audit.category_of(tid)
                                             for tid in starved)
    for name, n in starved_categories.most_common(10):
        print(f"  {name:<24} {n:>6}")
    for tid in starved[:10]:
        print(f"  e.g. {tid}")

    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump({"pass": pass_counts, "fail": fail_counts,
                       "pass_verdicts": pass_verdicts, "fail_verdicts": fail_verdicts,
                       "empty_verdict_passes": empty,
                       "prose_reference": sorted(prose_ref),
                       "negative_tests": sorted(negative),
                       "vacuous_candidates": vacuous,
                       "vacuous_bound": len(vacuous),
                       "vacuous_by_cdata_class": dict(cdata_split),
                       "vacuous_by_category": dict(by_category),
                       "starved_fails": starved,
                       "starved_fails_by_category": dict(starved_categories)},
                      fh, indent=2)
        print(f"\nwrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
