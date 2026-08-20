#!/usr/bin/env python3
"""WPT-RUN-5: how many reftest PASSes are scored on a page whose author CSS the
engine never applied — whatever the mechanism.

Slice 10 measured one mechanism: [BUG-786](../../bugs/BUG-786-OPEN.md) drops a
`<style><![CDATA[ … ]]></style>` block whole, so a pair that loses its styling on
both sides renders identically bare and scores PASS. The obvious follow-up
question is whether CDATA is the *only* such mechanism, and that one cannot be
answered by reading sources — a stylesheet can also die on an unsupported
at-rule, a selector the parser rejects, a `@charset`, a failed subresource.

So this audit asks the engine instead, and does it without knowing the
mechanism: render the test twice — once as served, once with **every** author
declaration removed (`<style>` blocks, `<link rel=stylesheet>`, `style="…"`) — and compare
both the display list and the layout dump. Identical output on both means no
author declaration changed a single pixel or a single box: whatever the verdict
says, the engine's CSS was inert for that test. On a reftest PASS that is a
vacuous PASS.

Read the number as an upper bound on inflation, the same way slice 10's is.
A test whose stylesheet is only *partly* applied counts as `effective` here, so
the bound is not tight from below either; and one family of true positives is
not a defect at all — a test whose assertion is "nothing is painted and nothing
moves" (`::selection` colours, a reftest that needs a user gesture) renders the
same stripped for a legitimate reason. Comparing layout as well as paint is
what keeps the far larger "declaration applied, paints nothing" family
(`background: fixed` on an empty div, the whole CSS2.1 `ref-nothing-below`
cluster) out of the count: geometry moves there even when paint does not.

Two guards against reading noise as signal:

* tests with no author stylesheet at all are counted separately
  (`no_author_style`) — stripping nothing changes nothing, and that is not a bug;
* a control sample of reftest FAILs is classified the same way, because
  "inert CSS" is only interesting if it is *not* the engine's normal state.

Pages are served from a local static server, not `wptserve`: the audit runs
alongside a live corpus run, which holds the wptserve ports. Consequence —
`.sub.` substitution and server pipes do not happen here, so ids that depend on
them are skipped by the `error` bucket rather than silently mis-scored.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/inert_style_audit.py [--out-dir .tmp/wpt-corpus]
        [--limit N] [--control N] [--jobs N] [--json OUT]
"""

import argparse
import collections
import concurrent.futures
import functools
import http.server
import json
import os
import random
import re
import socket
import subprocess
import sys
import threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import cdata_audit  # noqa: E402  (path set above)
import corpus_stats  # noqa: E402
import run_corpus  # noqa: E402

REPO_ROOT = cdata_audit.REPO_ROOT
WPT_ROOT = cdata_audit.WPT_ROOT

#: Query flag that makes the server hand back the same document with every
#: author stylesheet removed. A query parameter rather than a second document
#: root: relative and root-absolute subresource paths must resolve exactly as
#: they do for the unmodified page, or the diff would measure the move instead.
STRIP_FLAG = "lumen_strip=1"

_STYLE_BLOCK_RE = re.compile(r"<style\b[^>]*>.*?</style\s*>", re.S | re.I)
_LINK_RE = re.compile(r"<link\b[^>]*>", re.I)
_REL_STYLESHEET_RE = re.compile(r"rel\s*=\s*[\"']?[^\"'>]*stylesheet", re.I)
#: `style="…"` counts as author CSS too: it goes through the same declaration
#: parser, and a reftest styled only that way would otherwise drop out of the
#: audit as "nothing to strip" (8 of the first 10 such ids did).
_STYLE_ATTR_RE = re.compile(r"""\sstyle\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)""", re.I)


def strip_author_css(source: str) -> str:
    """Remove `<style>`, `<link rel=stylesheet>` and `style="…"` from a document."""
    source = _STYLE_BLOCK_RE.sub("", source)
    source = _LINK_RE.sub(
        lambda m: "" if _REL_STYLESHEET_RE.search(m.group(0)) else m.group(0), source)
    return _STYLE_ATTR_RE.sub("", source)


def has_author_css(source: str) -> bool:
    """True if the document carries any author CSS to begin with."""
    return (bool(_STYLE_BLOCK_RE.search(source))
            or bool(_STYLE_ATTR_RE.search(source))
            or any(_REL_STYLESHEET_RE.search(m.group(0))
                   for m in _LINK_RE.finditer(source)))


#: Query flag for the mechanism probe (`--explain`): serve the same document with
#: one extra, deliberately observable declaration injected into every author
#: `<style>` block — at its start (`lumen_probe=start`) or at its end
#: (`lumen_probe=end`). Stripping answers "did any author declaration matter";
#: injecting answers the next question down, "did the block reach the CSS parser
#: at all", and the start/end split localises the loss inside the block.
PROBE_FLAG = "lumen_probe"

#: The injected rule. `margin-left` on the root moves every box in the layout
#: dump, so one comparison covers any page; a paint-only marker would miss a
#: document that paints nothing. It is written last-wins-safe: an author rule
#: setting the root's `margin-left` would mask it, which shows up as a page whose
#: start and end probes are both invisible and is reported, not silently counted.
PROBE_RULE = "html{margin-left:37px}"

_STYLE_PAIR_RE = re.compile(r"(<style\b[^>]*>)(.*?)(</style\s*>)", re.S | re.I)


def inject_probe(source: str, where: str) -> str:
    """Add `PROBE_RULE` at the start/end of every author `<style>` block.

    Inside the `<![CDATA[ … ]]>` wrapper when there is one — the point of the
    probe is to test the block as the engine sees it, and a marker placed outside
    the wrapper would answer a different question.
    """
    def replace(match):
        head, body, tail = match.group(1), match.group(2), match.group(3)
        if where == "start":
            body = (body.replace("<![CDATA[", "<![CDATA[\n" + PROBE_RULE + "\n", 1)
                    if "<![CDATA[" in body else "\n" + PROBE_RULE + "\n" + body)
        else:
            body = (body.replace("]]>", "\n" + PROBE_RULE + "\n]]>", 1)
                    if "]]>" in body else body + "\n" + PROBE_RULE + "\n")
        return head + body + tail
    return _STYLE_PAIR_RE.sub(replace, source)


#: Query flag for the repair probe: serve the document with the `<![CDATA[` and
#: `]]>` markers stripped out of every author `<style>` block, i.e. the page as it
#: would reach the engine once [BUG-786](../../bugs/BUG-786-OPEN.md) is fixed.
#: This is the one probe that can price the bug instead of counting its reach:
#: rendering both sides of a reftest pair twice — as served and repaired — says
#: which verdicts the fix would MOVE, in both directions (a FAIL that only the
#: lost rule caused, and a PASS that only the loss made possible).
REPAIR_FLAG = "lumen_repair=1"

_CDATA_OPEN_RE = re.compile(r"<!\[CDATA\[")
_CDATA_CLOSE_RE = re.compile(r"\]\]>")


def repair_cdata(source: str) -> str:
    """Drop CDATA markers inside author `<style>` blocks, leaving the CSS alone.

    Only inside `<style>`: the markers are meaningful XML everywhere else in an
    XHTML document, and a document-wide replace would change what is being
    measured (script content, text nodes) instead of the stylesheet.
    """
    def replace(match):
        head, body, tail = match.group(1), match.group(2), match.group(3)
        return head + _CDATA_CLOSE_RE.sub("", _CDATA_OPEN_RE.sub("", body)) + tail
    return _STYLE_PAIR_RE.sub(replace, source)


class _StripHandler(http.server.SimpleHTTPRequestHandler):
    """Static file server that also serves a stylesheet-free copy on request."""

    def log_message(self, fmt, *args):  # noqa: D102 - silence per-request logging
        pass

    def do_GET(self):  # noqa: N802 - name fixed by BaseHTTPRequestHandler
        probe = None
        for where in ("start", "end"):
            if f"{PROBE_FLAG}={where}" in self.path:
                probe = where
        repair = REPAIR_FLAG in self.path
        if STRIP_FLAG not in self.path and probe is None and not repair:
            return super().do_GET()
        path = self.translate_path(self.path)
        try:
            with open(path, "rb") as fh:
                raw = fh.read()
        except OSError:
            self.send_error(404)
            return None
        text = raw.decode("utf-8", "replace")
        if probe:
            text = inject_probe(text, probe)
        elif repair:
            text = repair_cdata(text)
        else:
            text = strip_author_css(text)
        body = text.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", self.guess_type(path))
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        return None


def start_server() -> tuple:
    """Serve `tests/wpt/` on a free port; returns (server, port)."""
    handler = functools.partial(_StripHandler, directory=WPT_ROOT)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    port = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, port


#: Both dumps are compared, not just the display list. A declaration can be
#: applied correctly and still paint nothing (`background: fixed` on an empty
#: div, the whole "test passes if there is nothing below" family of CSS2.1),
#: and on paint alone those are indistinguishable from a stylesheet the engine
#: never parsed. Geometry separates them: `width: 1in` moves the layout dump
#: whether or not anything is painted.
DUMP_MODES = ("--dump-display-list", "--dump-layout")


def dump(binary: str, mode: str, url: str, timeout: int):
    """`mode` output for `url`, or None if the dump failed or timed out."""
    try:
        proc = subprocess.run([binary, mode, url],
                              cwd=REPO_ROOT, timeout=timeout,
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    except (subprocess.TimeoutExpired, OSError):
        return None
    if proc.returncode != 0:
        return None
    return proc.stdout


def source_of(test_id: str):
    """Text of the test document, or None if it is not a plain file on disk."""
    base = test_id.split("?")[0]
    try:
        with open(os.path.join(WPT_ROOT, base.lstrip("/")), encoding="utf-8",
                  errors="replace") as fh:
            return fh.read()
    except OSError:
        return None


#: CSS2.1 marks a test whose stylesheet is *supposed* to have no effect with
#: `<meta name="flags" content="invalid">` — the assertion is "this rule must
#: not apply" (`#1digit { color: red }` and a reference that says "no red").
#: Such a test renders identically stripped for a correct engine, so it lands
#: in the `inert` bucket as a false positive. Catching them by flag is partial,
#: not complete: the same "legitimately no effect" shape also comes from a
#: media query that does not match and a false `@supports` condition, neither
#: of which carries a flag. Hence `inert` stays an upper bound and this only
#: names the part of it that can be recognised mechanically.
_FLAGS_RE = re.compile(
    r"""<meta[^>]*name\s*=\s*["']?flags["']?[^>]*content\s*=\s*["']([^"']*)["']""",
    re.I)


def is_negative_test(test_id: str) -> bool:
    """True if the test declares `flags=invalid` — inert output is its point."""
    source = source_of(test_id)
    return bool(source) and "invalid" in (
        _FLAGS_RE.search(source).group(1).lower() if _FLAGS_RE.search(source) else "")


def classify_test(test_id: str, binary: str, port: int, timeout: int) -> str:
    """`no_author_css` / `inert` / `effective` / `error` for one test id."""
    source = source_of(test_id)
    if source is None:
        return "error"
    if not has_author_css(source):
        return "no_author_css"
    base = f"http://127.0.0.1:{port}{test_id}"
    sep = "&" if "?" in test_id else "?"
    for mode in DUMP_MODES:
        plain = dump(binary, mode, base, timeout)
        stripped = dump(binary, mode, base + sep + STRIP_FLAG, timeout)
        if plain is None or stripped is None:
            return "error"
        if plain != stripped:
            return "effective"
    return "inert"


#: Layout alone is enough for the probe (unlike `classify_test`, which needs both
#: dumps): `PROBE_RULE` moves geometry by construction, so a second comparison
#: would only double the cost.
PROBE_MODE = "--dump-layout"


def probe_test(test_id: str, binary: str, port: int, timeout: int) -> str:
    """Where an `inert` test lost its CSS: `sheet_live` / `first_rule_eaten` / …

    Reads as: `sheet_live` — the block reaches the CSS parser and the author's own
    declarations are the inert part (an unsupported property, a media query that
    does not match, a declaration overridden by itself); `first_rule_eaten` — the
    block is parsed but its opening rule is not, the signature of the
    `<![CDATA[` wrapper swallowing one rule as a bad prelude
    ([BUG-786](../../bugs/BUG-786-OPEN.md)); `sheet_lost` — nothing in the block
    took effect, i.e. a whole-stylesheet loss; `no_style_block` — the test's
    author CSS is a `<link>` or a `style=` attribute, which this probe does not
    reach and cannot speak about.
    """
    source = source_of(test_id)
    if source is None:
        return "error"
    if not _STYLE_PAIR_RE.search(source):
        return "no_style_block"
    base = f"http://127.0.0.1:{port}{test_id}"
    sep = "&" if "?" in test_id else "?"
    plain = dump(binary, PROBE_MODE, base, timeout)
    if plain is None:
        return "error"
    seen = {}
    for where in ("start", "end"):
        out = dump(binary, PROBE_MODE, f"{base}{sep}{PROBE_FLAG}={where}", timeout)
        if out is None:
            return "error"
        seen[where] = out != plain
    if seen["start"] and seen["end"]:
        return "sheet_live"
    if seen["end"]:
        return "first_rule_eaten"
    if seen["start"]:
        return "tail_eaten"
    return "sheet_lost"


def explain(ids: list, binary: str, port: int, timeout: int, jobs: int) -> dict:
    """`probe_test` over `ids`, in parallel; returns `{test_id: mechanism}`."""
    out = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = {pool.submit(probe_test, tid, binary, port, timeout): tid
                   for tid in ids}
        for future in concurrent.futures.as_completed(futures):
            out[futures[future]] = future.result()
    return out


def reftest_verdicts(out_dir: str, manifest: dict) -> dict:
    """`{test_id: status}` for every executed reftest, PASS or FAIL."""
    types = {}
    for test_type, _category, test_id in corpus_stats.iter_ids(manifest):
        if test_type == "reftest":
            types[test_id] = test_type
    results, _recovered, _empty = run_corpus.load_results(out_dir)
    return {tid: r.get("status") for tid, r in results.items()
            if tid in types and r.get("status") in ("PASS", "FAIL")}


def category_of(test_id: str) -> str:
    """Top-level WPT category of an id (`/css/CSS2/x.xht` -> `css`)."""
    parts = test_id.lstrip("/").split("/")
    return parts[0] if parts else "?"


def audit(ids: list, binary: str, port: int, timeout: int, jobs: int) -> dict:
    """Classify every id, in parallel; returns `{test_id: bucket}`."""
    verdicts = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = {pool.submit(classify_test, tid, binary, port, timeout): tid
                   for tid in ids}
        done = 0
        for future in concurrent.futures.as_completed(futures):
            verdicts[futures[future]] = future.result()
            done += 1
            if done % 200 == 0:
                print(f"  … {done}/{len(ids)}", file=sys.stderr, flush=True)
    return verdicts


def summarise(name: str, buckets: dict) -> dict:
    """Print and return the bucket histogram of one group."""
    counts = collections.Counter(buckets.values())
    total = sum(counts.values())
    print(f"\n{name}: {total} reftests")
    for bucket in ("inert", "effective", "no_author_css", "error"):
        n = counts.get(bucket, 0)
        share = f"{100.0 * n / total:.1f} %" if total else "-"
        print(f"  {bucket:<16} {n:>6}  {share}")
    return dict(counts)


def main_explain(args) -> int:
    """`--explain`: split the `inert` ids of an earlier run by mechanism."""
    with open(args.explain, encoding="utf-8") as fh:
        previous = json.load(fh)
    inert = sorted(tid for tid, bucket in previous.get("buckets", {}).items()
                   if bucket == "inert")
    print(f"{len(inert)} inert PASSes from {args.explain}")
    server, port = start_server()
    try:
        mechanisms = explain(inert, args.binary, port, args.timeout, args.jobs)
    finally:
        server.shutdown()
    counts = collections.Counter(mechanisms.values())
    print()
    for name, n in counts.most_common():
        print(f"  {name:<18} {n:>4}  {100.0 * n / max(len(inert), 1):.1f} %")
    print()
    for tid in inert:
        flag = "negative" if is_negative_test(tid) else ""
        print(f"  {mechanisms[tid]:<18} {cdata_audit.classify(tid):<11} {tid} {flag}")
    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump({"source": args.explain, "mechanisms": mechanisms,
                       "counts": dict(counts)}, fh, indent=2)
        print(f"\nwrote {args.json}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", default=cdata_audit.DEFAULT_MANIFEST)
    parser.add_argument("--out-dir", default=os.path.join(REPO_ROOT, ".tmp", "wpt-corpus"),
                        help="run directory with per-shard wptreport.json files")
    parser.add_argument("--binary", default=os.path.join("target", "dev-release", "lumen"))
    parser.add_argument("--limit", type=int, default=0,
                        help="audit the first N PASSes in id order (0 = all of them); "
                             "deterministic, but alphabetically biased — use --sample "
                             "for a number meant to stand for the whole set")
    parser.add_argument("--sample", type=int, default=0,
                        help="audit a seeded random sample of N PASSes instead of all "
                             "of them. The full set is ~2.1 s/id, i.e. hours, and the "
                             "audit shares a machine with the corpus run whose verdicts "
                             "it reads — a sample is what keeps the measurement from "
                             "perturbing what it measures")
    parser.add_argument("--only-cdata", action="store_true",
                        help="audit only the ids cdata_audit.py flags as CDATA-class — "
                             "the self-consistent denominator for a BUG-786 share")
    parser.add_argument("--control", type=int, default=300,
                        help="size of the reftest-FAIL control sample (0 = skip)")
    parser.add_argument("--jobs", type=int, default=2,
                        help="concurrent dumps; keep low while a corpus run is in flight")
    parser.add_argument("--timeout", type=int, default=30, help="seconds per dump")
    parser.add_argument("--seed", type=int, default=786, help="control-sample seed")
    parser.add_argument("--explain", default=None,
                        help="skip the classification and probe the mechanism behind the "
                             "`inert` ids of an earlier --json run instead: which of them "
                             "lost the stylesheet and which merely have declarations this "
                             "engine ignores")
    parser.add_argument("--json", default=None, help="write the numbers here as JSON")
    args = parser.parse_args()

    if args.explain:
        return main_explain(args)

    with open(args.manifest, encoding="utf-8") as fh:
        manifest = json.load(fh)

    verdicts = reftest_verdicts(args.out_dir, manifest)
    passes = sorted(tid for tid, st in verdicts.items() if st == "PASS")
    fails = sorted(tid for tid, st in verdicts.items() if st == "FAIL")
    if args.only_cdata:
        passes = [tid for tid in passes if cdata_audit.classify(tid) != "plain"]
        fails = [tid for tid in fails if cdata_audit.classify(tid) != "plain"]
    if args.limit:
        passes = passes[:args.limit]
    rng = random.Random(args.seed)
    if args.sample and args.sample < len(passes):
        sampled = rng.sample(passes, args.sample)
        passes = sorted(sampled)
    control = rng.sample(fails, min(args.control, len(fails))) if args.control else []
    total_passes = sum(1 for st in verdicts.values() if st == "PASS")
    print(f"executed reftests in {args.out_dir}: {len(verdicts)} "
          f"({len(passes)} of {total_passes} PASS audited, {len(control)} FAIL as control)")

    server, port = start_server()
    try:
        pass_buckets = audit(passes, args.binary, port, args.timeout, args.jobs)
        control_buckets = audit(control, args.binary, port, args.timeout, args.jobs)
    finally:
        server.shutdown()

    pass_counts = summarise("PASS", pass_buckets)
    control_counts = summarise("FAIL (control)", control_buckets)

    inert = [tid for tid, b in pass_buckets.items() if b == "inert"]
    negative = [tid for tid in inert if is_negative_test(tid)]
    print(f"\nof the {len(inert)} inert PASSes, {len(negative)} declare "
          f"`flags=invalid` — inert output is what they assert, so they are not "
          f"vacuous (the same is true of an unmatched media query or a false "
          f"@supports, which carry no flag: `inert` is an upper bound)")
    cdata_split = collections.Counter(cdata_audit.classify(tid) for tid in inert)
    print(f"\n{len(inert)} vacuous PASSes by CDATA class "
          f"(`plain` = a mechanism other than BUG-786):")
    for name, n in cdata_split.most_common():
        print(f"  {name:<12} {n:>6}")
    other = sorted(tid for tid in inert if cdata_audit.classify(tid) == "plain")
    by_category = collections.Counter(category_of(tid) for tid in other)
    print(f"\ntop categories of the {len(other)} non-CDATA vacuous PASSes:")
    for name, n in by_category.most_common(10):
        print(f"  {name:<24} {n:>6}")
    for tid in other[:15]:
        print(f"  e.g. {tid}")

    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump({"audited_pass_ids": len(pass_buckets),
                       "negative_tests": sorted(negative),
                       "buckets": pass_buckets, "control_buckets": control_buckets,
                       "pass": pass_counts, "control": control_counts,
                       "cdata_split": dict(cdata_split),
                       "non_cdata_examples": other[:200],
                       "non_cdata_by_category": dict(by_category)}, fh, indent=2)
        print(f"\nwrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
