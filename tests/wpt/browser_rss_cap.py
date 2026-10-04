#!/usr/bin/env python3
"""WPT-RUN-9 (`docs/tasks/p2-wpt-runner-throughput.md` §WPT-RUN-9, memory cap):
kill a corpus run's own `lumen` browser once its resident set passes a cap.

Why. The parallel-lane mode (`run_corpus.py --parallel-shards`) is limited by
memory, not CPU, and the memory goes to a handful of tests, not to the run:
healthy browsers hold 0.6-1.0 GB, while `/acid/acid3/numbered-tests.html`
(BUG-1267) and the `webstorage` quota tests (`storage_*_setitem_quotaexceedederr`,
`storage_*_quota_independent_from_*` — they fill `localStorage` until the
engine says no, and it never does) take one browser to 15-25 GB within a minute.
Every one of them ends in TIMEOUT with zero subtests, i.e. scores 0 whether it
is allowed to eat the machine or not. Meanwhile the other lanes' browsers page,
which stretches exactly the wall-clock timeouts their verdicts depend on.

What it does. A daemon thread polls the descendants of the calling process
once per `POLL_S`; a `lumen` whose RSS exceeds the cap is killed (with its own
descendants, if any). `wptrunner` sees a browser that died mid-test, records
the test as CRASH and restarts the browser — the same path a real crash takes.
Nothing outside the calling process's tree is ever looked at, so a neighbouring
session's browser or corpus run cannot be touched.

Every kill is appended to `<out_dir>/rss-cap-kills.jsonl` with the pid, the
RSS seen and the pid chain above it, so a CRASH in a report can always be told
apart from an engine crash (`kills_under` attributes kills to a shard).

`psutil` is optional for the corpus run (it is present in the documented
venv, but not pinned by `requirements.txt`); without it the cap is off and the
caller is told so rather than silently running uncapped.
"""

import json
import os
import threading
import time

#: Poll interval. The fastest growth measured (acid3) is ~0.4 GB/s, so a 1 s
#: poll overshoots a cap by well under a gigabyte.
POLL_S = 1.0

#: Default cap, GB. Healthy browsers peak at 1.0 GB over the 2026-10-04
#: measurement sets (2 253 ids, `run9-ctl`), the runaways at 15-25 GB; 4 GB is
#: 4x the healthy peak and still leaves room for three lanes on a 32 GB box.
DEFAULT_CAP_GB = 4.0

KILLS_FILE = "rss-cap-kills.jsonl"

_BROWSER_NAMES = frozenset({"lumen.exe", "lumen"})


def _psutil():
    try:
        import psutil  # noqa: PLC0415 — optional, see module docstring
    except ImportError:
        return None
    return psutil


class BrowserRssCap:
    """Background watchdog; `start()`/`stop()` or use as a context manager."""

    def __init__(self, cap_gb: float, out_dir: str, root_pid: int = None, log=print):
        self.cap_bytes = int(cap_gb * 2**30)
        self.cap_gb = cap_gb
        self.kills_path = os.path.join(out_dir, KILLS_FILE)
        self.root_pid = root_pid or os.getpid()
        self.log = log
        self.kills = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread = None
        self.psutil = _psutil()

    @property
    def active(self) -> bool:
        return self._thread is not None

    def start(self) -> bool:
        if self.cap_bytes <= 0:
            return False
        if self.psutil is None:
            self.log("warning: psutil is not installed — --max-browser-gb is OFF, "
                     "a runaway browser can page the whole machine")
            return False
        self._thread = threading.Thread(target=self._run, name="rss-cap", daemon=True)
        self._thread.start()
        return True

    def stop(self):
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=5)

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *exc):
        self.stop()

    def kills_under(self, pid: int) -> list:
        """Kills whose victim had `pid` among its ancestors (a shard's subprocess)."""
        with self._lock:
            return [k for k in self.kills if pid in k["ancestors"]]

    def _run(self):
        while not self._stop.wait(POLL_S):
            try:
                self.check_once()
            except Exception as exc:  # noqa: BLE001 — a watchdog must not die
                self.log(f"rss cap: poll failed: {exc!r}")

    def check_once(self) -> list:
        """One poll; returns the kill records it produced (also used by `_selftest`)."""
        ps = self.psutil
        try:
            root = ps.Process(self.root_pid)
            children = root.children(recursive=True)
        except ps.Error:
            return []
        made = []
        for proc in children:
            try:
                if (proc.name() or "").lower() not in _BROWSER_NAMES:
                    continue
                rss = proc.memory_info().rss
            except ps.Error:
                continue
            if rss <= self.cap_bytes:
                continue
            record = self._kill(proc, rss)
            if record:
                made.append(record)
        return made

    def _kill(self, proc, rss: int):
        ps = self.psutil
        ancestors = []
        try:
            for parent in proc.parents():
                ancestors.append(parent.pid)
                if parent.pid == self.root_pid:
                    break
            cmdline = " ".join(proc.cmdline())
            victims = proc.children(recursive=True) + [proc]
        except ps.Error:
            return None
        for victim in victims:
            try:
                victim.kill()
            except ps.Error:
                pass
        record = {"time": time.strftime("%Y-%m-%dT%H:%M:%S"), "pid": proc.pid,
                  "rss_gb": round(rss / 2**30, 2), "cap_gb": self.cap_gb,
                  "ancestors": ancestors, "cmdline": cmdline[:200]}
        with self._lock:
            self.kills.append(record)
            try:
                with open(self.kills_path, "a", encoding="utf-8") as fh:
                    fh.write(json.dumps(record) + "\n")
            except OSError:
                pass
        self.log(f"  rss cap: killed lumen pid {proc.pid} at {record['rss_gb']} GB "
                 f"(cap {self.cap_gb} GB)")
        return record


def load_kills(out_dir: str) -> list:
    path = os.path.join(out_dir, KILLS_FILE)
    if not os.path.isfile(path):
        return []
    with open(path, encoding="utf-8") as fh:
        return [json.loads(line) for line in fh if line.strip()]


def _selftest() -> int:
    """Check the decision logic against fake processes — which process is
    killed, which is spared, how kills are attributed and persisted. A real
    runaway is exercised by the A/B in the task doc, not here."""
    import tempfile  # noqa: PLC0415

    class FakeError(Exception):
        pass

    class Proc:
        def __init__(self, pid, name, rss, parents=(), children=()):
            self.pid, self._name, self._rss = pid, name, rss
            self._parents, self._children = list(parents), list(children)
            self.killed = False

        def name(self):
            return self._name

        def memory_info(self):
            return type("M", (), {"rss": self._rss})()

        def parents(self):
            return self._parents

        def children(self, recursive=False):
            return self._children

        def cmdline(self):
            return [self._name, "--bidi-port", "1"]

        def kill(self):
            self.killed = True

    root = Proc(100, "python.exe", 0)
    smoke = Proc(200, "python.exe", 0, parents=[root])
    fat = Proc(301, "lumen.exe", 6 * 2**30, parents=[smoke, root])
    thin = Proc(302, "lumen.exe", 1 * 2**30, parents=[smoke, root])
    other = Proc(303, "notlumen.exe", 9 * 2**30, parents=[smoke, root])
    root._children = [smoke, fat, thin, other]

    class FakePs:
        Error = FakeError

        @staticmethod
        def Process(pid):
            assert pid == 100
            return root

    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        cap = BrowserRssCap(4.0, tmp, root_pid=100, log=lambda *_: None)
        cap.psutil = FakePs
        made = cap.check_once()
        if [k["pid"] for k in made] != [301]:
            failures.append(f"killed {[k['pid'] for k in made]}, expected [301]")
        if not fat.killed or thin.killed or other.killed:
            failures.append("wrong process killed")
        if [k["pid"] for k in cap.kills_under(200)] != [301] or cap.kills_under(999):
            failures.append("kills_under attribution wrong")
        if [k["pid"] for k in load_kills(tmp)] != [301]:
            failures.append("kill not persisted")
        off = BrowserRssCap(0, tmp, log=lambda *_: None)
        if off.start():
            failures.append("cap 0 must stay off")
    for failure in failures:
        print("FAIL:", failure)
    print("browser_rss_cap selftest:", "FAIL" if failures else "ok")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(_selftest())
