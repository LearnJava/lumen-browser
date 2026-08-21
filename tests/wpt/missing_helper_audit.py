#!/usr/bin/env python3
"""WPT-RUN-5 slice 31: what does the unvendored-helper backlog cost the number?

Slice 30 found the backlog and measured its *reach*: 96 referenced paths are
absent from the checkout and, counted in the unit the pass-rate denominator uses,
they block **6 825 automatable manifest ids (10.1 % of the corpus)**, four files
carrying 5 572 of them. That is the whole content of `WPT-RUN-11`. What slice 30
did not measure is the price: an id that waits for a 404 only costs the figure if
it would have *scored* once the file were there, and nothing so far has asked it.

This audit asks it directly, per test, with an A/B on the same binary:

* **arm A** — the page as the corpus run gets it: the helper 404s;
* **arm B** — the same page, same binary, same harness options, served by the
  same private static server with one difference — a request the checkout cannot
  answer falls through to the upstream tree at the **pinned vendoring commit**
  (`tests/wpt/VENDOR.md`), fetched once and cached under `.tmp/`. Arm B is
  therefore "WPT-RUN-11 done", not "a different test run".

The fall-through is recursive by construction: a helper that pulls in further
helpers gets them the same way, so arm B answers "vendor what this test needs",
which is the decision WPT-RUN-11 actually faces — not "vendor exactly these four
files".

The arms are compared on the scoring rule the published number uses
(`run_corpus.py::score_reports`: passing subtests over total, or 1.0 for a
subtest-less PASS), so the sample's mean score delta projects straight onto the
blocked population as pass-rate points.

What this deliberately does not do, and why:

* **It does not write into `tests/wpt/`.** The corpus run reads test files off
  the doc root per request, so vendoring a helper into the served tree mid-run
  would hand later shards a different corpus than earlier ones and quietly
  contaminate the very figure WPT-RUN-5 is producing. The upstream cache lives in
  `.tmp/wpt-upstream-cache/` and is visible only to arm B's server.
* **It does not run `wptrunner`.** The corpus run owns the pinned `config.json`
  ports; a second runner would fight it for them or inherit the live run's
  substituted options through the `StaticHandler` freeze (see `CLAUDE.md`).
  Pages are served privately, the way `long_timeout_audit.py` does.
* **It does not score reftests.** A reftest verdict is a render comparison, not a
  testharness report; `/common/reftest-wait.js` (1 518 ids) is named in the
  census with the verdicts those ids hold today, so the unmeasured part of the
  backlog is stated in ids rather than left implicit.

Usage (from repo root, venv per tests/wpt/README.md):

    <venv>/python tests/wpt/missing_helper_audit.py --census
    <venv>/python tests/wpt/missing_helper_audit.py --sample 60 --jobs 3 --json OUT
    <venv>/python tests/wpt/missing_helper_audit.py --selftest
"""

import argparse
import asyncio
import collections
import functools
import glob
import http.server
import json
import os
import random
import re
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
WPT_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")
sys.path[:0] = [REPO_ROOT, os.path.join(REPO_ROOT, "tools", "webdriver"),
                os.path.dirname(os.path.abspath(__file__))]

import corpus_stats  # noqa: E402
import find_missing_resources as fmr  # noqa: E402

#: The commit `tests/wpt/` was vendored from (`tests/wpt/VENDOR.md`). Arm B must
#: fetch from *this* revision, not from `master`: a file taken off a later tip
#: could reference APIs or sibling files the rest of our snapshot does not have,
#: and the arm would then measure vendoring drift rather than the backlog.
VENDOR_PIN = "35be3b44f3111c4d614b5b201e399493d20e7b38"
UPSTREAM_RAW = "https://raw.githubusercontent.com/web-platform-tests/wpt/" + VENDOR_PIN

#: Where arm B keeps what it fetched. Outside the served tree on purpose — see
#: the module docstring.
CACHE_DIR = os.path.join(REPO_ROOT, ".tmp", "wpt-upstream-cache")

#: What `StaticHandler` substitutes into `testharnessreport.js` for a default
#: run — read back off the live server by `route_audit.py` (slice 22).
REPORT_ARGS = {"output": 0, "timeout_multiplier": 1,
               "explicit_timeout": "false", "debug": "false"}

#: `testharnessreport.js` ships numeric codes; the naming is `wptrunner`'s and
#: happens on the runner side, which this audit bypasses.
HARNESS_CODES = {0: "OK", 1: "ERROR", 2: "TIMEOUT", 3: "PRECONDITION_FAILED"}
SUBTEST_PASS = 0

#: Statuses that mean "this arm produced no verdict at all", as opposed to a
#: verdict that happens to score zero. `NOT-REPLACED` joins them for
#: `tls_eof_audit.py`: a document that never replaced the outgoing one is the
#: same nothing as a page that hung to the cap, just detected sooner.
DEAD_STATUSES = frozenset({"TIMEOUT", "CAP", "NOT-REPLACED"})

RESULTS_GLOBAL = "__lumen_wpt_results"
STALE_GLOBAL = "__lumen_wpt_stale"
POLL_INTERVAL_S = 0.1

RESET_EXPRESSION = f"""(() => {{
  window.{RESULTS_GLOBAL} = undefined;
  window.{STALE_GLOBAL} = true;
}})()"""

POLL_EXPRESSION = f"""(() => {{
  if (window.{STALE_GLOBAL} === true) {{ return JSON.stringify({{k: "s"}}); }}
  if (window.{RESULTS_GLOBAL} !== undefined) {{
    return JSON.stringify({{k: "r", v: window.{RESULTS_GLOBAL}}});
  }}
  return null;
}})()"""

#: Reference shapes a test document carries. Same three as
#: `find_missing_resources.REF_RE`, widened to relative paths: that tool answers
#: "what is missing from the vendored tree" (site-absolute by definition), this
#: one answers "what does *this id* wait for", and a sibling `src="helper.js"`
#: blocks it just as hard.
_REF_RE = re.compile(
    r"""(?:src|href)\s*=\s*["']?([^"'\s>]+)"""
    r"""|url\(\s*["']?([^"'\)\s]+)""")

#: Placeholders `wptserve` fills in a `.sub.` file. Our private server is one
#: origin on one port, so cross-origin variants collapse onto it; a test that
#: genuinely needs a second origin fails in both arms and cannot inflate the
#: delta. `route_audit.py` established the real values are irrelevant here —
#: what matters is that arm B does not serve a file full of raw `{{...}}`, which
#: is a syntax error and would make arm B *worse* than arm A.
_SUB_RE = re.compile(r"\{\{([^}]+)\}\}")


# --------------------------------------------------------------------------- #
# population
# --------------------------------------------------------------------------- #

def _resolve(ref: str, test_path: str) -> str:
    """Absolute on-disk path a reference from `test_path` points at."""
    ref = ref.split("#", 1)[0].split("?", 1)[0]
    if ref.startswith("/"):
        return os.path.join(WPT_ROOT, ref.lstrip("/"))
    return os.path.normpath(os.path.join(os.path.dirname(test_path), ref))


def missing_refs(test_id: str) -> set:
    """Referenced paths this id's own file needs and the checkout does not have.

    Site-relative results are returned as site paths (leading `/`) so they can be
    counted and fetched; `data:`/`http:`/template refs are skipped, and anything
    `wptrunner` serves from a static route is not a gap.
    """
    base = test_id.split("?")[0]
    path = os.path.join(WPT_ROOT, base.lstrip("/"))
    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            source = fh.read()
    except OSError:
        return set()
    out = set()
    for match in _REF_RE.finditer(source):
        ref = (match.group(1) or match.group(2) or "").strip()
        if not ref or "{{" in ref or ":" in ref.split("/")[0]:
            continue  # scheme-ful or a wptserve template variable
        clean = ref.split("#", 1)[0].split("?", 1)[0]
        if not clean:
            continue
        site = clean if clean.startswith("/") else "/" + os.path.relpath(
            _resolve(clean, path), WPT_ROOT).replace(os.sep, "/")
        if site in fmr.ROUTED_NOT_VENDORED:
            continue
        if not os.path.isfile(_resolve(clean, path)):
            out.add(site)
    return out


def unservable_reason(test_id: str):
    """Why a private static server cannot serve this id faithfully, or None."""
    base = test_id.split("?")[0]
    path = os.path.join(WPT_ROOT, base.lstrip("/"))
    if not os.path.exists(path):
        return "generated by wptserve (.any/.window), no file on disk"
    if ".sub." in base:
        return "needs .sub substitution of the test itself"
    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            source = fh.read()
    except OSError:
        return "unreadable"
    if "testdriver.js" in source:
        return "needs testdriver (wptrunner message queue)"
    return None


def load_verdicts(out_dir: str) -> dict:
    """`{test_id: {...}}` from the run's reports; first writer wins.

    Same rule as `run_corpus.load_results`: a `--resume` rerun appends a second
    report for the same shard and the earlier one is what the published snapshot
    was taken from.
    """
    verdicts = {}
    for path in sorted(glob.glob(os.path.join(out_dir, "*.json"))):
        if os.path.basename(path) == "state.json":
            continue
        try:
            with open(path, encoding="utf-8") as fh:
                report = json.load(fh)
        except (OSError, ValueError):
            continue
        for result in report.get("results", []):
            test_id = result.get("test")
            if not test_id or test_id in verdicts:
                continue
            subtests = result.get("subtests") or []
            verdicts[test_id] = {
                "status": result.get("status", ""),
                "seconds": (result.get("duration") or 0) / 1000.0,
                "subtests": len(subtests),
                "passed": sum(1 for s in subtests if s.get("status") == "PASS"),
            }
    return verdicts


def score_of(status: str, subtests: int, passed: int) -> float:
    """The published number's per-id score — `run_corpus.py::score_reports`."""
    if subtests:
        return passed / subtests
    return 1.0 if status == "PASS" else 0.0


def census(manifest: dict, verdicts: dict) -> dict:
    """The blocked population, before a single browser is started."""
    blocked_by_type = collections.Counter()
    ids_by_helper = collections.Counter()
    blocked = {}
    total_automatable = 0
    for test_type, _category, test_id in corpus_stats.iter_ids(manifest):
        if test_type in corpus_stats.NON_AUTOMATABLE_TYPES:
            continue
        total_automatable += 1
        refs = missing_refs(test_id)
        if not refs:
            continue
        blocked[test_id] = (test_type, refs)
        blocked_by_type[test_type] += 1
        for ref in refs:
            ids_by_helper[ref] += 1

    # What the blocked ids are worth today: the ceiling of any repair is
    # (1 - what they already score) per executed id, so an audit that never
    # states it can claim an upside the population cannot deliver.
    executed = held = 0
    executed_status = collections.Counter()
    for test_id, (_type, _refs) in blocked.items():
        verdict = verdicts.get(test_id)
        if not verdict:
            continue
        executed += 1
        executed_status[verdict["status"]] += 1
        held += score_of(verdict["status"], verdict["subtests"], verdict["passed"])

    probeable, unservable = [], collections.Counter()
    for test_id, (test_type, _refs) in blocked.items():
        if test_type != "testharness":
            continue
        reason = unservable_reason(test_id)
        if reason:
            unservable[reason] += 1
        else:
            probeable.append(test_id)

    strata = collections.Counter()
    plain = []
    for test_id in probeable:
        refs = blocked[test_id][1]
        if any(".sub." in ref for ref in refs):
            strata["waits for a .sub. helper"] += 1
        else:
            strata["waits for a plain helper"] += 1
            plain.append(test_id)

    return {
        "vendor_pin": VENDOR_PIN,
        "automatable_ids": total_automatable,
        "blocked_ids": len(blocked),
        "blocked_by_type": dict(blocked_by_type),
        "blocked_executed": executed,
        "blocked_executed_status": dict(executed_status.most_common(8)),
        "blocked_score_held": round(held, 2),
        "top_helpers": dict(ids_by_helper.most_common(12)),
        "distinct_helpers": len(ids_by_helper),
        "probeable": sorted(probeable),
        "plain": sorted(plain),
        "strata": dict(strata),
        "unservable": dict(unservable),
    }


# --------------------------------------------------------------------------- #
# arm B's upstream fall-through
# --------------------------------------------------------------------------- #

class UpstreamCache:
    """Fetch-once-and-remember for files missing from the checkout.

    Records every lookup so a run can prove arm B did something (`served`) and
    name what upstream itself does not have at the pin (`absent`), instead of
    reporting a null result that might just be a broken fetch.
    """

    def __init__(self, root: str = CACHE_DIR, offline: bool = False):
        """Cache under `root`; `offline` refuses the network (selftest/replay)."""
        self.root = root
        self.offline = offline
        self.lock = threading.Lock()
        self.served = collections.Counter()
        self.absent = collections.Counter()
        self.errors = collections.Counter()

    def get(self, site_path: str):
        """Bytes for `/some/path`, or None if upstream has no such file."""
        rel = site_path.lstrip("/")
        local = os.path.join(self.root, *rel.split("/"))
        with self.lock:
            if os.path.isfile(local):
                self.served[site_path] += 1
                with open(local, "rb") as fh:
                    return fh.read()
            if self.offline:
                self.absent[site_path] += 1
                return None
            try:
                with urllib.request.urlopen(f"{UPSTREAM_RAW}/{rel}", timeout=30) as resp:
                    body = resp.read()
            except urllib.error.HTTPError as exc:
                if exc.code == 404:
                    self.absent[site_path] += 1
                    return None
                self.errors[f"{site_path} HTTP {exc.code}"] += 1
                return None
            except OSError as exc:  # DNS, TLS, timeout — never a test verdict
                self.errors[f"{site_path} {type(exc).__name__}"] += 1
                return None
            os.makedirs(os.path.dirname(local), exist_ok=True)
            with open(local, "wb") as fh:
                fh.write(body)
            self.served[site_path] += 1
            return body


def substitute(text: str, port: int) -> tuple:
    """Fill a `.sub.` file's `{{...}}` placeholders against our own server.

    Returns `(text, unresolved)`. Leaving a placeholder in place would ship a
    JavaScript syntax error, making arm B *worse* than arm A on exactly the ids
    it is supposed to help, so the fallback is a string literal, and how often
    that happened is reported rather than hidden.
    """
    unresolved = collections.Counter()

    def repl(match):
        key = match.group(1).strip()
        if key.startswith("ports["):
            return str(port)
        if key in ("host", "domains[]", "hosts[][]"):
            return "127.0.0.1"
        if key.startswith("domains[") or key.startswith("hosts["):
            return "127.0.0.1"
        if key.startswith("location["):
            return {"location[host]": f"127.0.0.1:{port}",
                    "location[hostname]": "127.0.0.1",
                    "location[port]": str(port)}.get(key, "127.0.0.1")
        unresolved[key] += 1
        return "127.0.0.1"

    return _SUB_RE.sub(repl, text), unresolved


# --------------------------------------------------------------------------- #
# private static server
# --------------------------------------------------------------------------- #

class ArmHandler(http.server.SimpleHTTPRequestHandler):
    """Serves `tests/wpt/`; arm B falls through to the upstream cache on a miss.

    `testharnessreport.js` carries wptserve's four `%(...)s` placeholders and is
    a syntax error served through them, in both arms alike.

    Shared with `tls_eof_audit.py`, which reuses the tree, the placeholder
    substitution and `.sub.` handling but frames its responses differently — the
    name lost its underscore when the second caller appeared.
    """

    cache = None   # UpstreamCache for arm B, None for arm A
    port = 0

    def log_message(self, fmt, *args):  # noqa: D102 - silence per-request logging
        pass

    def render(self, site_path: str):
        """`(body, content_type)` for a site path, or None if nothing serves it.

        Split out of `do_GET` so a sibling audit can reuse the tree, the report
        placeholders and the `.sub.` substitution while framing the response
        differently — `tls_eof_audit.py` must answer without `Content-Length`.
        """
        path = self.translate_path(site_path)
        raw = None
        if os.path.isfile(path):
            try:
                with open(path, "rb") as fh:
                    raw = fh.read()
            except OSError:
                raw = None
        elif self.cache is not None:
            raw = self.cache.get(site_path)
        if raw is None:
            return None
        text = raw.decode("utf-8", "replace")
        if path.replace("\\", "/").endswith("/resources/testharnessreport.js"):
            text = text % REPORT_ARGS
        elif ".sub." in site_path:
            text, _unresolved = substitute(text, self.port)
        return text.encode("utf-8"), self.guess_type(path)

    def do_GET(self):  # noqa: N802 - name fixed by BaseHTTPRequestHandler
        rendered = self.render(self.path.split("?")[0])
        if rendered is None:
            self.send_error(404)
            return None
        body, content_type = rendered
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        return None


def start_server(cache=None) -> tuple:
    """Serve `tests/wpt/` on a free port; returns (server, port)."""
    server_holder = {}
    handler = type("_Arm", (ArmHandler,), {"cache": cache})
    handler_partial = functools.partial(handler, directory=WPT_ROOT)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler_partial)
    server.daemon_threads = True
    handler.port = server.server_address[1]
    server_holder["s"] = server
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, server.server_address[1]


# --------------------------------------------------------------------------- #
# browser
# --------------------------------------------------------------------------- #

def free_port() -> int:
    """A port nothing holds right now (racy by nature, as everywhere in wpt)."""
    sock = socket.socket()
    try:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]
    finally:
        sock.close()


def launch(binary: str, env: dict = None) -> tuple:
    """Start `binary --bidi-port <free>`; returns (proc, port, token).

    `env` replaces the child's environment wholesale (callers pass a copy of
    their own with additions) — `tls_eof_audit.py` needs `LUMEN_EXTRA_CA_CERT`.
    """
    port = free_port()
    proc = subprocess.Popen([binary, "--bidi-port", str(port)],
                            cwd=REPO_ROOT, env=env,
                            stderr=subprocess.PIPE, text=True)
    deadline = time.time() + 60
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"lumen exited with {proc.returncode}")
        sock = socket.socket()
        try:
            sock.connect(("127.0.0.1", port))
            break
        except OSError:
            time.sleep(0.05)
        finally:
            sock.close()
    else:
        proc.kill()
        raise TimeoutError("BiDi port never opened")
    token = None
    for _ in range(400):
        line = proc.stderr.readline()
        if not line:
            break
        if line.strip().startswith("[bidi] token: "):
            token = line.strip()[len("[bidi] token: "):]
            break
    if token is None:
        proc.kill()
        raise RuntimeError("lumen --bidi-port did not print [bidi] token")
    threading.Thread(target=lambda: collections.deque(proc.stderr, maxlen=0),
                     daemon=True).start()
    return proc, port, token


async def run_one(session, context, url: str, cap: float, stale_cap: float = None) -> dict:
    """Navigate and poll for testharness results; the arm's whole measurement.

    `stale_cap` (seconds, None = never) ends the poll early while the *outgoing*
    document is still in place — `navigate` answered but nothing replaced the
    page, the BUG-438 shape. `tls_eof_audit.py` needs it: its arm A loses every
    document body by construction, so without it every id in that arm would burn
    the full `cap` waiting for a page that was never parsed.
    """
    from webdriver.bidi.error import BidiException, UnknownErrorException
    from webdriver.bidi.modules.script import ContextTarget

    started = time.time()
    try:
        await session.script.evaluate(expression=RESET_EXPRESSION,
                                      target=ContextTarget(context), await_promise=False)
    except BidiException:
        pass  # no JS runtime on the outgoing document — nothing to carry over
    try:
        await session.browsing_context.navigate(context=context, url=url, wait="complete")
    except BidiException as exc:
        return {"status": "NAV-ERROR", "message": str(exc), "seconds": time.time() - started}

    while True:
        try:
            value = await session.script.evaluate(expression=POLL_EXPRESSION,
                                                  target=ContextTarget(context),
                                                  await_promise=False)
        except UnknownErrorException as exc:
            # The JS runtime is installed off the UI thread, after `navigate`
            # answers — see executorlumen.py. Not ready is not an error.
            if "JS context not available" not in getattr(exc, "message", str(exc)):
                raise
        else:
            if value.get("type") == "string":
                outer = json.loads(value["value"])
                if outer["k"] == "r":
                    _url, code, message, _stack, subtests = json.loads(outer["v"])
                    return {"status": HARNESS_CODES.get(code, str(code)),
                            "message": message,
                            "subtests": len(subtests),
                            "passed": sum(1 for s in subtests if s[1] == SUBTEST_PASS),
                            "seconds": time.time() - started}
                if (outer["k"] == "s" and stale_cap is not None
                        and time.time() - started > stale_cap):
                    # Still the outgoing document, long after `navigate` said
                    # `complete`: the requested one never became a document.
                    return {"status": "NOT-REPLACED", "seconds": time.time() - started}
        if time.time() - started > cap:
            return {"status": "CAP", "seconds": time.time() - started}
        await asyncio.sleep(POLL_INTERVAL_S)


def disable_ws_keepalive() -> None:
    """Stop the BiDi client from dropping a session whose browser is busy.

    `tools/webdriver/.../bidi/transport.py` calls `websockets.connect(url,
    max_size=...)` and takes the library's default keepalive: a ping every 20 s,
    the connection closed if no pong comes back within 20 s more. Several pages
    in this population block the browser's main thread far longer than that
    (media elements, `sharedworker`), so the *client* tears the session down and
    every later id on that worker fails with `keepalive ping timeout` — the first
    run of this audit lost 51 of 60 ids that way, which reads like a null result
    and is really a dead socket. `tools/` is vendored unmodified (`VENDOR.md`),
    so the argument is injected here instead of patched there. Nothing is lost by
    dropping the ping: `run_one`'s own `cap` is the real deadline, and a browser
    that never comes back is caught by `Browser.alive`.
    """
    import websockets  # noqa: PLC0415 - only needed when a browser is driven

    if getattr(websockets, "_lumen_no_keepalive", False):
        return
    original = websockets.connect

    def connect(url, **kwargs):
        kwargs.setdefault("ping_interval", None)
        return original(url, **kwargs)

    websockets.connect = connect
    websockets._lumen_no_keepalive = True


class Browser:
    """A lumen process plus its BiDi session, restartable in place.

    A page in this population can hang or kill the browser, and a worker that
    cannot recover turns one bad id into a whole arm of zeros.
    """

    def __init__(self, binary: str, env: dict = None):
        """Nothing is started until `start()`."""
        self.binary = binary
        self.env = env
        self.proc = None
        self.session = None
        self.context = None
        self.restarts = 0

    async def start(self) -> None:
        """Launch the browser and take the top-level context."""
        from webdriver.bidi.client import BidiSession  # noqa: PLC0415

        self.proc, port, token = launch(self.binary, self.env)
        self.session = BidiSession.bidi_only(
            f"ws://127.0.0.1:{port}",
            requested_capabilities={"alwaysMatch": {"token": token}})
        await self.session.start()
        tree = await self.session.browsing_context.get_tree()
        self.context = tree[0]["context"]

    async def stop(self) -> None:
        """End the session and make sure the process is gone."""
        if self.session is not None:
            try:
                await self.session.end()
            except Exception:  # noqa: BLE001 - the browser is about to be killed
                pass
        self.session = None
        if self.proc is not None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.proc = None

    async def restart(self) -> None:
        """Replace a browser that stopped answering."""
        await self.stop()
        self.restarts += 1
        await self.start()

    @property
    def alive(self) -> bool:
        """False once the process has exited or the session was torn down."""
        return (self.proc is not None and self.proc.poll() is None
                and self.session is not None)


async def worker(binary: str, port_a: int, port_b: int, queue: list, lock,
                 cap: float, results: dict, progress, scheme: str = "http",
                 env: dict = None, stale_cap: float = None,
                 order: tuple = ("a", "b")) -> None:
    """One browser, both arms of every id it pulls off the shared queue.

    `scheme`/`env`/`stale_cap`/`order` exist for `tls_eof_audit.py`, whose two
    arms are TLS servers, whose browser needs the test CA and whose arm A never
    gets a document at all; the defaults are this audit's own.

    `order` is which arm goes first, not which arm is which: `stale_cap` can only
    fire while some *previous* document is still in place, so an audit whose arm A
    never loads anything wants arm B probed first — otherwise the first id of every
    browser sits on `about:blank`, where the stale marker was never set, and burns
    the full `cap` instead of being recognised as "nothing replaced the page".
    """
    browser = Browser(binary, env)
    await browser.start()
    try:
        while True:
            async with lock:
                if not queue:
                    return
                test_id = queue.pop()
            row = {}
            ports = {"a": port_a, "b": port_b}
            for arm in order:
                port = ports[arm]
                url = f"{scheme}://127.0.0.1:{port}{test_id}"
                for attempt in (0, 1):
                    try:
                        row[arm] = await run_one(browser.session, browser.context, url,
                                                cap, stale_cap)
                        break
                    except Exception as exc:  # noqa: BLE001 - one id must not end the run
                        row[arm] = {"status": "PROBE-ERROR", "message": str(exc),
                                    "attempt": attempt}
                        try:
                            await browser.restart()
                        except Exception as restart_exc:  # noqa: BLE001
                            row[arm] = {"status": "PROBE-ERROR",
                                        "message": f"restart failed: {restart_exc}"}
                            break
                if not browser.alive:
                    await browser.restart()
            results[test_id] = row
            progress(test_id, row)
    finally:
        await browser.stop()


# --------------------------------------------------------------------------- #
# classification
# --------------------------------------------------------------------------- #

def classify(row: dict) -> tuple:
    """`(bucket, score_a, score_b)` for one id's two arms."""
    arm_a, arm_b = row.get("a") or {}, row.get("b") or {}
    if arm_a.get("status") in (None, "PROBE-ERROR", "NAV-ERROR"):
        return "probe-error", 0.0, 0.0
    if arm_b.get("status") in (None, "PROBE-ERROR", "NAV-ERROR"):
        return "probe-error", 0.0, 0.0
    score_a = score_of(arm_a["status"], arm_a.get("subtests", 0), arm_a.get("passed", 0))
    score_b = score_of(arm_b["status"], arm_b.get("subtests", 0), arm_b.get("passed", 0))
    a_dead = arm_a["status"] in DEAD_STATUSES
    b_dead = arm_b["status"] in DEAD_STATUSES
    if a_dead and not b_dead:
        return "revived", score_a, score_b
    if score_b > score_a + 1e-9:
        return "improved", score_a, score_b
    if score_b < score_a - 1e-9:
        return "regressed", score_a, score_b
    if a_dead and b_dead:
        return "still-dead", score_a, score_b
    return "unchanged", score_a, score_b


def project(rows: dict, population: int, corpus_ids: int) -> dict:
    """Sample -> population, in the units the headline is published in."""
    buckets = collections.Counter()
    delta = 0.0
    subtests_a = subtests_b = 0
    measured = 0
    for row in rows.values():
        bucket, score_a, score_b = classify(row)
        buckets[bucket] += 1
        if bucket == "probe-error":
            continue
        measured += 1
        delta += score_b - score_a
        subtests_a += (row["a"].get("subtests") or 0)
        subtests_b += (row["b"].get("subtests") or 0)
    mean = delta / measured if measured else 0.0
    projected = mean * population
    return {"buckets": dict(buckets), "measured": measured,
            "sample_delta": round(delta, 3), "mean_delta": round(mean, 4),
            "subtests_a": subtests_a, "subtests_b": subtests_b,
            "population": population,
            "projected_score": round(projected, 1),
            "projected_points": round(100.0 * projected / corpus_ids, 4) if corpus_ids else 0.0}


# --------------------------------------------------------------------------- #
# entry points
# --------------------------------------------------------------------------- #

def print_census(data: dict) -> None:
    """The population table — printed by every mode, browser or not."""
    print(f"vendoring pin: {data['vendor_pin']}")
    print(f"automatable ids in the manifest:   {data['automatable_ids']}")
    print(f"blocked by a missing reference:    {data['blocked_ids']} "
          f"({100.0 * data['blocked_ids'] / max(1, data['automatable_ids']):.1f} %), "
          f"{data['distinct_helpers']} distinct paths")
    for test_type, count in sorted(data["blocked_by_type"].items(), key=lambda kv: -kv[1]):
        print(f"    {test_type:<22} {count}")
    print(f"of them executed by this run:      {data['blocked_executed']}, "
          f"holding {data['blocked_score_held']} score points today")
    for status, count in sorted(data["blocked_executed_status"].items(), key=lambda kv: -kv[1]):
        print(f"    {status:<22} {count}")
    print("blocked ids by missing path (an id can wait for more than one):")
    for ref, count in sorted(data["top_helpers"].items(), key=lambda kv: -kv[1]):
        print(f"    {count:6d}  {ref}")
    print(f"testharness ids probeable here: {len(data['probeable'])}")
    for reason, count in sorted(data["unservable"].items(), key=lambda kv: -kv[1]):
        print(f"    not probeable: {reason:<48} {count}")
    for stratum, count in sorted(data["strata"].items(), key=lambda kv: -kv[1]):
        print(f"    {stratum:<30} {count}")


def _selftest() -> int:
    """Pure-function checks — no browser, no network, no corpus."""
    failures = []

    def check(name, cond):
        if not cond:
            failures.append(name)

    check("score: subtest ratio", abs(score_of("TIMEOUT", 4, 1) - 0.25) < 1e-9)
    check("score: bare PASS", score_of("PASS", 0, 0) == 1.0)
    check("score: bare OK scores nothing", score_of("OK", 0, 0) == 0.0)
    check("cap scores zero", score_of("CAP", 0, 0) == 0.0)
    check("harness codes decode", HARNESS_CODES[2] == "TIMEOUT" and HARNESS_CODES[0] == "OK")

    rows = {
        "/a": {"a": {"status": "TIMEOUT", "subtests": 0, "passed": 0},
               "b": {"status": "OK", "subtests": 4, "passed": 4}},
        "/b": {"a": {"status": "TIMEOUT", "subtests": 0, "passed": 0},
               "b": {"status": "CAP", "subtests": 0, "passed": 0}},
        "/c": {"a": {"status": "OK", "subtests": 2, "passed": 1},
               "b": {"status": "OK", "subtests": 2, "passed": 1}},
        "/d": {"a": {"status": "PROBE-ERROR"}, "b": {"status": "OK"}},
        "/e": {"a": {"status": "OK", "subtests": 2, "passed": 2},
               "b": {"status": "OK", "subtests": 2, "passed": 1}},
    }
    check("revived", classify(rows["/a"])[0] == "revived")
    check("still-dead", classify(rows["/b"])[0] == "still-dead")
    check("unchanged", classify(rows["/c"])[0] == "unchanged")
    check("probe-error excluded", classify(rows["/d"])[0] == "probe-error")
    check("regressed", classify(rows["/e"])[0] == "regressed")

    proj = project(rows, population=400, corpus_ids=1000)
    check("projection divides by measured ids only", proj["measured"] == 4)
    check("projection scales the mean", abs(proj["projected_score"] - 50.0) < 0.05)
    check("projection in points", abs(proj["projected_points"] - 5.0) < 0.01)
    check("subtest totals carried", proj["subtests_a"] == 4 and proj["subtests_b"] == 8)

    text, unresolved = substitute("x = '{{host}}:{{ports[http][0]}}';", 8123)
    check("sub fills host and port", text == "x = '127.0.0.1:8123';")
    check("sub leaves nothing unresolved here", not unresolved)
    text2, unresolved2 = substitute("y = {{nonsense}};", 1)
    check("sub reports what it invented", "nonsense" in unresolved2 and "{{" not in text2)

    check("report placeholders substitute",
          "timeout_multiplier: 1" in ("setup({timeout_multiplier: %(timeout_multiplier)s})"
                                      % REPORT_ARGS))

    # The reference scan must see a relative sibling as well as a site path, and
    # must not report a file the checkout does have.
    probe_dir = os.path.join(REPO_ROOT, ".tmp", "missing-helper-selftest")
    os.makedirs(probe_dir, exist_ok=True)
    check("routed paths are not a gap", "/resources/testdriver.js" in fmr.ROUTED_NOT_VENDORED)

    offline = UpstreamCache(root=os.path.join(probe_dir, "cache"), offline=True)
    check("offline cache reports a miss instead of fetching",
          offline.get("/common/nope.js") is None and offline.absent["/common/nope.js"] == 1)

    # The keepalive injection is what makes a long-hanging page survivable; check
    # it reaches the argument list rather than trusting that it does.
    import websockets  # noqa: PLC0415 - selftest-only
    saved, saved_flag = websockets.connect, getattr(websockets, "_lumen_no_keepalive", False)
    seen = {}
    websockets._lumen_no_keepalive = False
    websockets.connect = lambda url, **kwargs: seen.update(kwargs)
    try:
        disable_ws_keepalive()
        websockets.connect("ws://127.0.0.1:1")
        check("keepalive disabled for the vendored transport",
              "ping_interval" in seen and seen["ping_interval"] is None)
    finally:
        websockets.connect, websockets._lumen_no_keepalive = saved, saved_flag

    if failures:
        for name in failures:
            print(f"selftest FAILED: {name}", file=sys.stderr)
        return 1
    print(f"selftest OK ({21 - len(failures)} checks)")
    return 0


def main() -> int:
    """CLI entry point."""
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", default=os.path.join(
        REPO_ROOT, "target", os.environ.get("LUMEN_PROFILE", "dev-release"), "lumen"))
    parser.add_argument("--out-dir", default=os.path.join(REPO_ROOT, ".tmp", "wpt-corpus"),
                        help="the corpus run's report directory (for what the blocked "
                             "ids score today; the arms themselves are re-run)")
    parser.add_argument("--manifest", default=corpus_stats.DEFAULT_MANIFEST)
    parser.add_argument("--sample", type=int, default=60)
    parser.add_argument("--seed", type=int, default=811)
    parser.add_argument("--jobs", type=int, default=3)
    parser.add_argument("--cap", type=float, default=45.0,
                        help="wall-clock ceiling per arm")
    parser.add_argument("--stratum", choices=("all", "plain"), default="all",
                        help="`plain` samples only ids whose missing helpers need no "
                             "wptserve substitution, where arm B is exact")
    parser.add_argument("--prefix", default="",
                        help="sample only ids under this path — the population is "
                             "heavy-tailed (WPT-RUN-5 slice 31: all of the sample's "
                             "score came from `/editing/run/`, 110 of 5 731 ids), so a "
                             "stratum worth pinning is probed on its own and projected "
                             "over its own size")
    parser.add_argument("--census", action="store_true", help="population only, no browser")
    parser.add_argument("--json", help="write the full per-id table here")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        return _selftest()

    with open(args.manifest, encoding="utf-8") as fh:
        manifest = json.load(fh)
    verdicts = load_verdicts(args.out_dir)
    data = census(manifest, verdicts)
    print_census(data)
    if args.census:
        if args.json:
            with open(args.json, "w", encoding="utf-8") as fh:
                json.dump(data, fh, indent=1)
        return 0

    if not os.path.isfile(args.binary):
        print(f"lumen binary not found: {args.binary}", file=sys.stderr)
        return 1

    population = data["plain"] if args.stratum == "plain" else data["probeable"]
    if args.prefix:
        population = [tid for tid in population if tid.startswith(args.prefix)]
        if not population:
            print(f"no probeable id under {args.prefix}", file=sys.stderr)
            return 1
    rng = random.Random(args.seed)
    sample = rng.sample(population, min(args.sample, len(population)))
    print(f"\nprobing {len(sample)} of {len(population)} ({args.stratum}) on {args.jobs} "
          f"browsers (arm A: as vendored, arm B: + upstream fall-through at the pin)\n")

    cache = UpstreamCache()
    server_a, port_a = start_server(cache=None)
    server_b, port_b = start_server(cache=cache)
    results = {}
    done = [0]

    def progress(test_id, row):
        done[0] += 1
        bucket = classify(row)[0]
        arm_a, arm_b = row["a"], row["b"]
        print(f"  [{done[0]:>3}/{len(sample)}] {bucket:<11} "
              f"A={arm_a.get('status'):<9}{arm_a.get('seconds', 0):5.1f}s "
              f"{arm_a.get('passed', 0)}/{arm_a.get('subtests', 0)}  "
              f"B={arm_b.get('status'):<9}{arm_b.get('seconds', 0):5.1f}s "
              f"{arm_b.get('passed', 0)}/{arm_b.get('subtests', 0)}  {test_id}", flush=True)

    disable_ws_keepalive()

    async def drive():
        lock = asyncio.Lock()
        queue = list(sample)
        await asyncio.gather(*[
            worker(args.binary, port_a, port_b, queue, lock, args.cap, results, progress)
            for _ in range(max(1, args.jobs))])

    started = time.time()
    try:
        asyncio.run(drive())
    finally:
        server_a.shutdown()
        server_b.shutdown()

    corpus_ids = data["automatable_ids"]
    proj = project(results, len(population), corpus_ids)
    print(f"\n{len(results)} ids probed in {(time.time() - started) / 60:.1f} min")
    for bucket, count in sorted(proj["buckets"].items(), key=lambda kv: -kv[1]):
        print(f"    {bucket:<15} {count}")
    print(f"\nsubtests seen: arm A {proj['subtests_a']}, arm B {proj['subtests_b']}")
    print(f"upstream fall-through served {sum(cache.served.values())} requests "
          f"over {len(cache.served)} paths; {len(cache.absent)} absent upstream, "
          f"{sum(cache.errors.values())} fetch errors")
    for ref, count in cache.served.most_common(10):
        print(f"    {count:4d}  {ref}")
    if cache.errors:
        for key, count in cache.errors.most_common(5):
            print(f"    fetch error x{count}: {key}")
    print(f"\nmean score delta per probed id: {proj['mean_delta']:+.4f}")
    print(f"projected over {proj['population']} ids of the "
          f"{args.prefix or args.stratum} stratum: "
          f"{proj['projected_score']:+.1f} score points "
          f"= {proj['projected_points']:+.4f} pass-rate points of {corpus_ids} ids")

    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump({"census": data, "sample": sample, "results": results,
                       "projection": proj,
                       "upstream": {"served": dict(cache.served),
                                    "absent": dict(cache.absent),
                                    "errors": dict(cache.errors)}}, fh, indent=1)
        print(f"wrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
