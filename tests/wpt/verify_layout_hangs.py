#!/usr/bin/env python3
"""WPT-RUN-6 slice 23: the pages that kill the browser instead of failing a test.

Slices 15-22 measured *harness* mechanisms — an API that answers wrong, a
promise that never settles — by driving a live window and reading the page's
own console. This slice takes the part of the residual that no such probe can
reach: 45 of the 311 unexplained TIMEOUT ids of the WPT-RUN-5 snapshot print
nothing after `Получено N байт`, i.e. the browser never finished laying the
page out, and 16 of them are the `hung-browser` culprits that take the rest of
their shard down with them (695 collateral TIMEOUTs, `timeout_audit.py`'s
`hung_browsers` list). A reftest has no harness output by construction, so the
source-marker stage of the audit is blind here — the evidence has to come from
running the page.

**What this script does, and why in this shape.**

* It serves the *real* WPT checkout over plain http and runs one browser per
  page in a headless dump mode (`--dump-layout`, `--dump-display-list`,
  `--screenshot`), with a timeout. A page that does not finish `--dump-layout`
  in 15 s while a normal page finishes in 0.1 s is an engine hang, measured
  without a window, without wptrunner and without the harness.
* For a page that hangs it samples the **main thread's stack**: gdb is started
  as the browser's parent (`ptrace_scope=1` on this machine allows tracing a
  descendant only, so `gdb -p` cannot work), the page is given a few seconds
  to wedge, and the inferior is stopped with `SIGUSR1`. The frame at the top
  says which loop is spinning; `thread apply all bt` plus a filter for the
  thread that carries `lumen_shell`/`lumen_layout` frames picks the main
  thread out of the rayon/V8 pool.
* For a hang it can then **reduce** the page: line-wise delta debugging
  (`--reduce`) against a server that serves the candidate body at the test's
  own URL, so `/css/support/grid.css` and every other absolute reference still
  resolves and the reduction stays a real page.
* A **reftest's reference is a page too** — and in five of the sixteen wedges
  it is the reference that hangs while the test itself lays out in 0.13 s
  (WPT-RUN-6 slice 11 noticed the shape, this slice measured it). `--refs`
  extracts `<link rel=match|mismatch>` and probes those as well.

**Measured 2026-08-22** (dev-release, Linux, commit `3ae02b208`), sweeping the
311 residual ids of `.tmp/wpt-corpus` (the WPT-RUN-5 snapshot) plus the 19
references of its reftests. Five mechanisms, four of them reproduced by a
minimal page in `REPROS` below, and one non-defect:

    grid-implicit-track-loop   CSS Grid auto-placement never terminates when an
                               item's column (row) range reaches past the last
                               explicit line: the `fits` test in the placement
                               loop can only ever be false, and the scan
                               advances forever                    <- BUG-801
                               22 residual ids, 10 of them culprits of 244
                               collateral TIMEOUTs. Causality is carried by the
                               repro pairs below: two pages differing in one
                               declaration, one hanging and one laying out in
                               0.01 s.
    svg-transform-loop         `parse_svg_transform` advances `pos` only for a
                               token shaped `alpha(`; anything else (`FAIL_ME(30)`,
                               a bare digit) re-enters the loop at the same
                               position forever                    <- BUG-803
                               1 residual id — and the single worst wedge in
                               the snapshot: 133 collateral TIMEOUTs. The same
                               function panics outright on `transform=","`
                               (index out of bounds: the `pos < len` guard
                               binds to the `&&` operand only), which no
                               corpus page happens to carry.
    nested-flex-exponential    Nested flex containers cost ~2x per level
                               (`lay_out_flex` lays every child out twice and
                               keeps neither result): depth 16 = 0.27 s,
                               18 = 1.21 s, 20 = 4.91 s. The
                               five `.xhtml` culprits reach ~50 levels because
                               XHTML is parsed as HTML and `<div class="a"/>`
                               is an *open* tag there (BUG-786), so the whole
                               document nests                      <- BUG-802
                               5 residual ids, 318 collateral TIMEOUTs.
    dom-wrapper-oom            `document.createElement` costs ~165 us and an
                               interned wrapper object that nothing frees on
                               this path: 20k elements = 3.3 s, 40k = fatal V8
                               OOM and the process dies (exit 133) <- BUG-849
                               1 residual id. The same page OOMs identically
                               inside the corpus run, so this is not a probe
                               artefact.
    unclamped-blur             `backdrop-filter: blur(100000px)` is rasterized
                               at face value: 16.6 s for one CPU screenshot,
                               linear in the radius                <- BUG-850
                               1 residual id.

Not a defect, kept here so the next reader does not re-measure it: the two
`wasm/core/*.wast.js.html` ids complete in 32 s and 10 s — they are simply
larger than the runner's timeout, not hung.

**Traps this probe hit, worth knowing before writing another one.** A plain
static server is *not* wptserve: `resources/testharnessreport.js` in this
checkout is a template with `%(output)s` placeholders that only wptserve
substitutes, so a page served this way reports
`SyntaxError: Unexpected token '%'` from the harness. That is an artefact of
the probe, never evidence about the engine — read it as "this page's harness
did not run", and check any JS-dependent finding against the corpus log
instead (BUG-852's OOM was confirmed that way). And `--dump-layout` runs the
page's scripts but hands the runtime no fetch provider, so nothing a probe
page fetches will ever arrive (CLAUDE.md, "fetch() / XMLHttpRequest do
nothing in the headless dump modes").

Usage (from repo root):

    tests/wpt/.venv/bin/python tests/wpt/verify_layout_hangs.py --repros
    tests/wpt/.venv/bin/python tests/wpt/verify_layout_hangs.py --ids-from .tmp/s23-audit.json
    tests/wpt/.venv/bin/python tests/wpt/verify_layout_hangs.py --id /css/css-grid/subgrid/line-names-012.html --stack
    tests/wpt/.venv/bin/python tests/wpt/verify_layout_hangs.py --reduce /css/css-transforms/2d-rotate-notref.html
    tests/wpt/.venv/bin/python tests/wpt/verify_layout_hangs.py --emit-table   # ids -> mechanism, for timeout_audit.py

Exit code is 0 whatever the outcome — this is a measurement, not a gate.
"""

import argparse
import functools
import http.server
import json
import os
import re
import signal
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
WPT_ROOT = HERE

#: Minimal pages, each reduced from a real residual test by `--reduce` and then
#: cut by hand until one declaration decides the outcome. `expect` is what the
#: page does today; a page that stops hanging is the fix landing, and this table
#: is the check for it (`--repros`).
REPROS = {
    # BUG-849. `grid-column: 3` needs a third column, the template has two, and
    # the placement loop's `fits` test compares against the *explicit* count —
    # so it is false at every scan position and the scan runs forever. One
    # explicit column is the accidental escape hatch (`|| n_explicit_cols == 1`),
    # which is why a single-track grid does not hang.
    "grid-line-past-explicit": (
        '<div style="display:grid; grid-template-columns: 35px 35px;">'
        '<div style="grid-column: 3"></div></div>', "hang"),
    "grid-span-past-explicit": (
        '<div style="display:grid; grid-template-columns: repeat(3, 35px);">'
        '<div style="grid-column: 1 / span 4"></div></div>', "hang"),
    # The same defect reached through subgrid: the outer template resolves to
    # two explicit columns, the inner item asks for lines 2..4.
    "grid-subgrid-nested": (
        '<div><div style="display:grid; grid-template-columns: subgrid [a][a] 30px;">'
        '<div style="display:grid; grid-column: 2 / span 2;'
        ' grid-template-columns: subgrid [][][a];"><i>x</i></div></div></div>', "hang"),
    # Controls for BUG-849: the same shapes that stay inside the explicit grid,
    # and the one-column case.
    "grid-inside-explicit": (
        '<div style="display:grid; grid-template-columns: 35px 35px;">'
        '<div style="grid-column: 1 / span 2"></div></div>', "ok"),
    "grid-single-track": (
        '<div style="display:grid; grid-template-columns: 35px;">'
        '<div style="grid-column: 3"></div></div>', "ok"),
    # BUG-850. The function-name scan takes ASCII letters only, so it stops at
    # `_`; the next byte is not `(`, the loop `continue`s without touching
    # `pos`, and nothing ever advances.
    "svg-transform-underscore": (
        '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">'
        '<rect transform="foo_bar(30)" width="10" height="10"/></svg>', "hang"),
    "svg-transform-bare-number": (
        '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">'
        '<rect transform="1" width="10" height="10"/></svg>', "hang"),
    # The neighbouring defect in the same loop: the whitespace/comma skip is
    # written `pos < len && ws || bytes[pos] == b','`, so the guard covers the
    # first operand only and a trailing comma indexes one past the end.
    "svg-transform-comma": (
        '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">'
        '<rect transform="," width="10" height="10"/></svg>', "died"),
    "svg-transform-valid": (
        '<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">'
        '<rect transform="rotate(30)" width="10" height="10"/></svg>', "ok"),
    # BUG-851 is a cost, not a loop, so the repro is a depth sweep
    # (`--nesting`) rather than one page; this entry pins the depth at which it
    # crosses a runner timeout.
    "flex-nesting-20": (
        ('<div style="height:200px;display:flex;flex-direction:column;'
         'float:left;border:1px dotted black">' * 20)
        + '<div style="width:10px;flex:0 10px;border:solid 1px purple;padding:2px">x</div>'
        + '</div>' * 20, "slow"),
}

#: Ids measured to hang/die standalone, with the mechanism each belongs to.
#: `timeout_audit.py` reads this table (`--emit-table` writes it as JSON) — a
#: regex over the source cannot decide these: whether an item reaches past the
#: last explicit line depends on the *resolved* track count, which
#: `repeat(auto-fill, ...)` makes a function of the container width.
MEASURED = {
    "grid-implicit-track-loop": [
        "/css/css-box/margin-trim/computed-margin-values/grid-block-end-column-auto-flow.html",
        "/css/css-box/margin-trim/computed-margin-values/grid-block-end.html",
        "/css/css-box/margin-trim/computed-margin-values/grid-inline-end-columns-added-to-end.html",
        "/css/css-grid/abspos/grid-positioned-items-and-autofit-tracks-007.html",
        "/css/css-grid/abspos/grid-positioned-items-gaps-001.html",
        "/css/css-grid/abspos/grid-positioned-items-gaps-rtl-001.html",
        "/css/css-grid/grid-definition/grid-auto-fill-columns-001.html",
        "/css/css-grid/grid-definition/grid-auto-fit-columns-001.html",
        "/css/css-grid/grid-definition/grid-auto-repeat-intrinsic-001.html",
        "/css/css-grid/grid-definition/grid-auto-repeat-multiple-values-002.html",
        "/css/css-grid/grid-definition/grid-auto-repeat-multiple-values-003.html",
        "/css/css-grid/grid-definition/grid-change-auto-repeat-tracks.html",
        "/css/css-grid/grid-lanes/subgrid/grid-subgridded-to-grid-lanes/column-subgrid-writing-direction-001.html",
        "/css/css-grid/grid-lanes/subgrid/grid-subgridded-to-grid-lanes/gap/column-subgrid-grid-gap-003.html",
        "/css/css-grid/grid-lanes/subgrid/grid-subgridded-to-grid-lanes/line-names/column-line-names-012.html",
        "/css/css-grid/grid-lanes/track-sizing/auto-repeat/column-auto-repeat-022.html",
        "/css/css-grid/subgrid/line-names-005.html",
        "/css/css-grid/subgrid/line-names-008.html",
        "/css/css-grid/subgrid/line-names-010.html",
        "/css/css-grid/subgrid/line-names-012.html",
        "/css/css-grid/subgrid/parent-repeat-auto-fit-001.html",
        "/css/css-grid/subgrid/writing-directions-001.html",
    ],
    "nested-flex-exponential": [
        "/css/css-flexbox/flexbox-justify-content-vert-001a.xhtml",
        "/css/css-flexbox/flexbox-justify-content-vert-001b.xhtml",
        "/css/css-flexbox/flexbox-justify-content-vert-002.xhtml",
        "/css/css-flexbox/flexbox-justify-content-vert-004.xhtml",
        "/css/css-flexbox/flexbox-justify-content-vert-005.xhtml",
    ],
    "svg-transform-loop": [
        "/css/css-transforms/2d-rotate-notref.html",
    ],
    "dom-wrapper-oom": [
        "/css/selectors/invalidation/has-complexity.html",
    ],
    # WPT-RUN-6 slice 29. Not a loop: the page finishes, in 31.2 s and 15.9 s
    # against the harness's 10 s. `WebAssembly.validate` costs 101 ms per call
    # on this corpus because `parse_code_section` materializes the declared
    # local count (BUG-898) — a 32-byte module is 6.95 s of it.
    "wasm-locals-unbounded": [
        "/wasm/core/binary.wast.js.html",
        "/wasm/core/bulk-memory/memory_copy.wast.js.html",
        "/wasm/core/memory64/memory_copy64.wast.js.html",
    ],
    "unclamped-blur": [
        "/css/filter-effects/backdrop-filter-blur-large-value.html",
    ],
}

#: Bug each mechanism became, mirrored in `timeout_audit.py`.
MECHANISM_BUG = {
    "grid-implicit-track-loop": "BUG-801",
    "svg-transform-loop": "BUG-803",
    "nested-flex-exponential": "BUG-802",
    "dom-wrapper-oom": "BUG-849",
    "unclamped-blur": "BUG-850",
}

#: `<link rel=match|mismatch href=...>` — a reftest's reference page. Probed
#: alongside the test because in five of the sixteen wedges of the snapshot it
#: is the reference that hangs.
_REF_RE = re.compile(r"""<link[^>]*\srel\s*=\s*["']?(?:match|mismatch)["']?[^>]*>""",
                     re.IGNORECASE)
_HREF_RE = re.compile(r"""href\s*=\s*["']([^"']+)["']""", re.IGNORECASE)


class _Handler(http.server.SimpleHTTPRequestHandler):
    """Static server over the WPT checkout, with one overridable path.

    The override is what makes reduction possible: the candidate body is served
    at the *test's own* URL, so its absolute references (`/css/support/grid.css`,
    `/resources/testharness.js`) resolve exactly as they do in a real run.
    """

    #: Set by `Server`; a dict so the handler class stays stateless.
    override = {"path": None, "body": b""}

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=WPT_ROOT, **kwargs)

    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.override["path"] and self.path.split("?")[0] == self.override["path"]:
            body = self.override["body"]
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        return super().do_GET()


class Server:
    """The probe's http server; `url_for` turns a manifest id into a URL."""

    def __init__(self):
        handler = type("_H", (_Handler,), {"override": {"path": None, "body": b""}})
        self._handler = handler
        self._srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=self._srv.serve_forever, daemon=True).start()

    @property
    def port(self):
        return self._srv.server_address[1]

    def url_for(self, test_id):
        return f"http://127.0.0.1:{self.port}{test_id}"

    def serve_body(self, path, text):
        self._handler.override["path"] = path
        self._handler.override["body"] = text.encode("utf-8")
        return f"http://127.0.0.1:{self.port}{path}"

    def stop(self):
        self._srv.shutdown()


def run_page(binary, url, mode="--dump-layout", limit=15.0, out=None):
    """Run one page in one dump mode. Returns `(status, seconds, stderr tail)`.

    `status` is `hang` (the timeout fired), `died` (the process aborted — a
    fatal V8 OOM leaves exit 133 this way), or `ok`.
    """
    args = ["timeout", str(limit), binary]
    if mode == "--screenshot":
        args += [mode, out or os.path.join(REPO, ".tmp", "verify-layout-hangs.png")]
    else:
        args += [mode]
    args.append(url)
    started = time.time()
    proc = subprocess.run(args, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    elapsed = round(time.time() - started, 2)
    err = proc.stderr.decode("utf-8", "replace")
    if proc.returncode == 124:
        status = "hang"
    elif proc.returncode != 0:
        status = "died"
    else:
        status = "ok"
    return status, elapsed, err[-2000:]


#: gdb has to *launch* the browser: `/proc/sys/kernel/yama/ptrace_scope` is 1 on
#: this machine, so only an ancestor may trace a process and `gdb -p <pid>` is
#: refused. Batch mode cannot interrupt a running inferior either (`interrupt`
#: returns "Selected thread is running" and the backtrace comes out empty), so
#: the inferior is stopped by a signal instead — gdb stops on `SIGUSR1` by
#: default and `nopass` keeps the browser from seeing it.
_GDB_ARGS = ["-q", "-batch",
             "-ex", "set pagination off",
             "-ex", "set confirm off",
             "-ex", "set debuginfod enabled off",
             "-ex", "handle SIGUSR1 stop print nopass",
             "-ex", "run",
             "-ex", "thread apply all bt 40"]


def sample_stack(binary, url, wait=8.0, mode="--dump-layout"):
    """Main-thread frames of a page that is currently wedged.

    Returns the frames of the thread that carries `lumen_*` frames — the rayon
    and V8 worker threads are parked in a futex wait and would otherwise be
    whichever thread gdb happens to print first.
    """
    proc = subprocess.Popen(["gdb"] + _GDB_ARGS + ["--args", binary, mode, url],
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True)
    time.sleep(wait)
    kids = subprocess.run(["pgrep", "-P", str(proc.pid)],
                          capture_output=True, text=True).stdout.split()
    for kid in kids:
        try:
            os.kill(int(kid), signal.SIGUSR1)
        except (OSError, ValueError):
            pass
    try:
        out = proc.communicate(timeout=30)[0]
    except subprocess.TimeoutExpired:
        proc.kill()
        out = proc.communicate()[0]
    best = []
    for block in re.split(r"\nThread \d+ \(", out):
        frames = [re.sub(r"^#\d+\s+(0x[0-9a-f]+ in )?", "", line).split(" (")[0].strip()
                  for line in block.splitlines() if line.startswith("#")]
        if any("lumen_" in f for f in frames) and len(frames) > len(best):
            best = frames
    return best


def ddmin(server, path, lines, binary, limit, still_bad):
    """Line-wise delta debugging: drop as many lines as keep the page bad."""
    n = 2
    while len(lines) >= 2:
        chunk = max(1, len(lines) // n)
        reduced = None
        for i in range(0, len(lines), chunk):
            candidate = lines[:i] + lines[i + chunk:]
            if not candidate:
                continue
            url = server.serve_body(path, "\n".join(candidate))
            if still_bad(*run_page(binary, url, limit=limit)[:2]):
                reduced = candidate
                break
        if reduced is None:
            if n >= len(lines):
                break
            n = min(len(lines), n * 2)
        else:
            lines = reduced
            n = max(2, n - 1)
    return lines


def source_path(test_id):
    return os.path.join(WPT_ROOT, test_id.split("?")[0].lstrip("/"))


def references_of(test_id):
    """Absolute ids of a reftest's `rel=match|mismatch` pages."""
    path = source_path(test_id)
    try:
        with open(path, "rb") as handle:
            text = handle.read().decode("utf-8", "replace")
    except OSError:
        return []
    out = []
    base = os.path.dirname(test_id.split("?")[0])
    for tag in _REF_RE.findall(text) or []:
        pass
    for match in _REF_RE.finditer(text):
        href = _HREF_RE.search(match.group(0))
        if not href:
            continue
        ref = href.group(1)
        out.append(ref if ref.startswith("/")
                   else os.path.normpath(os.path.join(base, ref)))
    return out


def sweep(binary, ids, limit, jobs, mode="--dump-layout"):
    """Run every id once; report the ones that hang, die or drag."""
    server = Server()
    try:
        def one(test_id):
            status, secs, err = run_page(binary, server.url_for(test_id),
                                         mode=mode, limit=limit)
            return {"id": test_id, "status": status, "seconds": secs,
                    "oom": "out of memory" in err}
        with ThreadPoolExecutor(jobs) as pool:
            return list(pool.map(one, ids))
    finally:
        server.stop()


def cmd_repros(args):
    """Run the minimal pages of `REPROS` and print measured vs expected."""
    server = Server()
    try:
        worst = 0
        for name, (body, expect) in REPROS.items():
            html = "<!doctype html><meta charset=utf-8>\n" + body
            url = server.serve_body("/__repro__.html", html)
            status, secs, _ = run_page(args.binary, url, limit=args.limit)
            got = status if status != "ok" else ("slow" if secs > 3 else "ok")
            flag = "  " if got == expect else "<-"
            worst += got != expect
            bug = MECHANISM_BUG.get(_mechanism_of_repro(name), "")
            print(f"{flag} {name:28} expect={expect:5} got={got:5} "
                  f"t={secs:6}s {bug}")
        print(f"\n{len(REPROS) - worst}/{len(REPROS)} as recorded")
    finally:
        server.stop()


def _mechanism_of_repro(name):
    if name.startswith("grid"):
        return "grid-implicit-track-loop"
    if name.startswith("svg"):
        return "svg-transform-loop"
    if name.startswith("flex"):
        return "nested-flex-exponential"
    return ""


def cmd_nesting(args):
    """BUG-851's cost curve: layout time against flex nesting depth."""
    server = Server()
    try:
        item = ('<div style="width:10px;flex:0 10px;border:solid 1px purple;'
                'padding:2px">x</div>')
        box = ('<div style="height:200px;display:flex;flex-direction:column;'
               'float:left;border:1px dotted black">')
        for depth in args.depths:
            html = ("<!doctype html><meta charset=utf-8>\n" + box * depth
                    + item + "</div>" * depth)
            url = server.serve_body("/__nesting__.html", html)
            status, secs, _ = run_page(args.binary, url, limit=args.limit)
            print(f"depth={depth:3} {status:5} t={secs}s")
    finally:
        server.stop()


def cmd_sweep(args):
    ids = args.ids
    if args.refs:
        extra = []
        for test_id in ids:
            extra += references_of(test_id)
        ids = ids + [i for i in dict.fromkeys(extra) if i not in ids]
    rows = sweep(args.binary, ids, args.limit, args.jobs, mode=args.mode)
    bad = [r for r in rows if r["status"] != "ok" or r["seconds"] > 3]
    for row in sorted(bad, key=lambda r: (-r["seconds"], r["id"])):
        print(f'{row["status"]:5} t={row["seconds"]:6}s '
              f'{"OOM " if row["oom"] else "    "}{row["id"]}')
    print(f"\n{len(bad)} of {len(rows)} pages hang, die or take over 3 s")
    if args.json:
        with open(args.json, "w", encoding="utf-8") as handle:
            json.dump(rows, handle, indent=1)
        print(f"wrote {args.json}")


def cmd_stack(args):
    server = Server()
    try:
        frames = sample_stack(args.binary, server.url_for(args.id), wait=args.wait)
        if not frames:
            print("no lumen frames — the page may not be hanging")
            return
        for i, frame in enumerate(frames[:25]):
            print(f"{i:3} {frame}")
    finally:
        server.stop()


def cmd_reduce(args):
    server = Server()
    try:
        path = args.reduce.split("?")[0]
        text = open(source_path(args.reduce), encoding="utf-8",
                    errors="replace").read()
        probe_path = os.path.join(os.path.dirname(path), "__probe__.html")
        url = server.serve_body(probe_path, text)
        status, secs, _ = run_page(args.binary, url, limit=args.limit)
        bad = (lambda s, t: s != "ok" or t > args.limit * 0.6)
        print(f"baseline {status} t={secs}s lines={len(text.splitlines())}")
        if not bad(status, secs):
            print("page is not bad under this limit — nothing to reduce")
            return
        out = ddmin(server, probe_path, text.splitlines(), args.binary,
                    args.limit, bad)
        print(f"--- reduced to {len(out)} lines\n" + "\n".join(out))
    finally:
        server.stop()


def cmd_emit_table(args):
    table = {test_id: mech for mech, ids in MEASURED.items() for test_id in ids}
    print(json.dumps(table, indent=1, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary",
                        default=os.path.join(REPO, "target", "dev-release",
                                             "lumen"))
    parser.add_argument("--limit", type=float, default=15.0,
                        help="seconds before a page counts as hung")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--mode", default="--dump-layout",
                        choices=["--dump-layout", "--dump-display-list",
                                 "--screenshot"])
    parser.add_argument("--id", action="append", dest="ids", default=[],
                        help="manifest id to probe (repeatable)")
    parser.add_argument("--ids-from",
                        help="timeout_audit.py --json output; sweeps its "
                             "residual_ids")
    parser.add_argument("--refs", action="store_true",
                        help="also probe every rel=match reference")
    parser.add_argument("--stack", action="store_true",
                        help="sample the main-thread stack of --id")
    parser.add_argument("--wait", type=float, default=8.0,
                        help="seconds before the stack sample is taken")
    parser.add_argument("--reduce", help="delta-debug this id down to a "
                                         "minimal bad page")
    parser.add_argument("--repros", action="store_true",
                        help="run the minimal pages of REPROS")
    parser.add_argument("--nesting", action="store_true",
                        help="flex nesting depth cost curve (BUG-851)")
    parser.add_argument("--depths", type=int, nargs="*",
                        default=[8, 12, 14, 16, 18, 20])
    parser.add_argument("--emit-table", action="store_true",
                        help="print the MEASURED table as JSON")
    parser.add_argument("--json", help="write the sweep result here")
    args = parser.parse_args()

    if args.ids_from:
        with open(args.ids_from, encoding="utf-8") as handle:
            args.ids += json.load(handle).get("residual_ids", [])

    if args.emit_table:
        cmd_emit_table(args)
    elif args.repros:
        cmd_repros(args)
    elif args.nesting:
        cmd_nesting(args)
    elif args.reduce:
        cmd_reduce(args)
    elif args.stack:
        if len(args.ids) != 1:
            parser.error("--stack needs exactly one --id")
        args.id = args.ids[0]
        cmd_stack(args)
    elif args.ids:
        cmd_sweep(args)
    else:
        parser.error("nothing to do — pass --repros, --id, --ids-from, "
                     "--reduce, --nesting or --emit-table")
    return 0


if __name__ == "__main__":
    sys.exit(main())
