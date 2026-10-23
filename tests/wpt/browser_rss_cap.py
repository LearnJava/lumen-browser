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

WPT-RUN-18 — the cap above looks at one browser, but the run dies of the sum:
WPT-RUN-16 started 3 lanes x 14 browsers on a 32 GB box, spent ~1.5 h in swap
and booked false TIMEOUTs on `editing`, `WebCryptoAPI`, `webaudio`. So there is
a second trigger, `min_free_gb`: while the machine's available physical memory
is below it, the run's biggest browser (when it holds at least
`LOW_FREE_FLOOR_GB`) is killed — its one test becomes a CRASH instead of
dragging 40 neighbours into the page file. Such kills carry `"reason":
"low-free"` in `rss-cap-kills.jsonl`; cap kills carry `"cap"`. The watchdog
also samples free memory, swap and the browsers' resident total every
`MEM_LOG_EVERY_S` into `mem-log.tsv` (`python browser_rss_cap.py --sample PID
--out FILE` does that for a run that has no watchdog, e.g. the "before" arm of
a measurement).

`psutil` is optional for the corpus run (it is present in the documented
venv, but not pinned by `requirements.txt`); without it the cap is off and the
caller is told so rather than silently running uncapped.
"""

import json
import os
import sys
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
MEM_LOG_FILE = "mem-log.tsv"
MEM_LOG_HEADER = "time\tfree_gb\tswap_used_gb\tbrowsers\tbrowser_rss_gb\tmax_browser_gb\n"

#: Low-free trigger (WPT-RUN-18): only a browser at least this big is worth
#: killing — a healthy one holds 0.6-1.0 GB and is not what is eating the box.
LOW_FREE_FLOOR_GB = 2.0

#: After a low-free kill the freed pages come back asynchronously; do not shoot
#: a second browser for the same shortage before the first has had time to count.
LOW_FREE_COOLDOWN_S = 5.0

#: How often the memory log gets a row.
MEM_LOG_EVERY_S = 10.0

_BROWSER_NAMES = frozenset({"lumen.exe", "lumen"})


def _psutil():
    try:
        import psutil  # noqa: PLC0415 — optional, see module docstring
    except ImportError:
        return None
    return psutil


class BrowserRssCap:
    """Background watchdog; `start()`/`stop()` or use as a context manager."""

    def __init__(self, cap_gb: float, out_dir: str, root_pid: int = None, log=print,
                 min_free_gb: float = 0.0, low_free_floor_gb: float = LOW_FREE_FLOOR_GB,
                 clock=time.monotonic):
        self.cap_bytes = int(cap_gb * 2**30)
        self.cap_gb = cap_gb
        self.min_free_bytes = int(min_free_gb * 2**30)
        self.min_free_gb = min_free_gb
        self.floor_bytes = int(low_free_floor_gb * 2**30)
        self.kills_path = os.path.join(out_dir, KILLS_FILE)
        self.mem_log_path = os.path.join(out_dir, MEM_LOG_FILE)
        self.root_pid = root_pid or os.getpid()
        self.log = log
        self.kills = []
        self._clock = clock
        self._last_low_kill = None
        self._last_mem_row = None
        self._scan = (0, 0, 0)  # browsers, their RSS total, the biggest one
        #: Lowest free memory / highest swap seen by any poll, bytes.
        self.min_free_seen = None
        self.max_swap_seen = 0
        self.low_free_polls = 0
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread = None
        self.psutil = _psutil()

    @property
    def active(self) -> bool:
        return self._thread is not None

    def start(self) -> bool:
        if self.cap_bytes <= 0 and self.min_free_bytes <= 0:
            return False
        if self.psutil is None:
            self.log("warning: psutil is not installed — --max-browser-gb and --min-free-gb "
                     "kills are OFF, a runaway browser can page the whole machine")
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
                self.maybe_log_memory()
            except Exception as exc:  # noqa: BLE001 — a watchdog must not die
                self.log(f"rss cap: poll failed: {exc!r}")

    def available_bytes(self):
        """Free physical memory, or None when it cannot be read."""
        try:
            return self.psutil.virtual_memory().available
        except (self.psutil.Error, OSError):
            return None

    def scan(self) -> list:
        """`[(process, rss)]` of the run's `lumen` browsers; also refreshes the
        numbers the memory log prints."""
        ps = self.psutil
        try:
            root = ps.Process(self.root_pid)
            children = root.children(recursive=True)
        except ps.Error:
            self._scan = (0, 0, 0)
            return []
        browsers = []
        for proc in children:
            try:
                if (proc.name() or "").lower() not in _BROWSER_NAMES:
                    continue
                browsers.append((proc, proc.memory_info().rss))
            except ps.Error:
                continue
        sizes = [rss for _proc, rss in browsers]
        self._scan = (len(sizes), sum(sizes), max(sizes, default=0))
        return browsers

    def check_once(self) -> list:
        """One poll; returns the kill records it produced (also used by `_selftest`)."""
        made = []
        alive = []
        for proc, rss in self.scan():
            if self.cap_bytes > 0 and rss > self.cap_bytes:
                record = self._kill(proc, rss, "cap")
                if record:
                    made.append(record)
                    continue
            alive.append((proc, rss))
        free = self.available_bytes()
        if free is not None:
            if self.min_free_seen is None or free < self.min_free_seen:
                self.min_free_seen = free
        if self.min_free_bytes > 0 and free is not None and free < self.min_free_bytes:
            self.low_free_polls += 1
            record = self._relieve(alive, free)
            if record:
                made.append(record)
        return made

    def _relieve(self, alive: list, free: int):
        """Free memory is short: kill the biggest browser that is worth it."""
        now = self._clock()
        if self._last_low_kill is not None and now - self._last_low_kill < LOW_FREE_COOLDOWN_S:
            return None
        if not alive:
            return None
        proc, rss = max(alive, key=lambda pair: pair[1])
        if rss < self.floor_bytes:
            return None
        record = self._kill(proc, rss, "low-free", free)
        if record:
            self._last_low_kill = now
        return record

    def maybe_log_memory(self, force: bool = False):
        """Append a row to `mem-log.tsv` at most once per `MEM_LOG_EVERY_S`."""
        now = self._clock()
        if not force and self._last_mem_row is not None                 and now - self._last_mem_row < MEM_LOG_EVERY_S:
            return
        self._last_mem_row = now
        free = self.available_bytes()
        try:
            swap = self.psutil.swap_memory().used
        except (self.psutil.Error, OSError):
            swap = 0
        self.max_swap_seen = max(self.max_swap_seen, swap)
        count, total, biggest = self._scan
        row = "\t".join([time.strftime("%Y-%m-%dT%H:%M:%S"),
                         "" if free is None else f"{free / 2**30:.2f}", f"{swap / 2**30:.2f}",
                         str(count), f"{total / 2**30:.2f}", f"{biggest / 2**30:.2f}"]) + "\n"
        try:
            fresh = not os.path.isfile(self.mem_log_path)
            with open(self.mem_log_path, "a", encoding="utf-8") as fh:
                if fresh:
                    fh.write(MEM_LOG_HEADER)
                fh.write(row)
        except OSError:
            pass

    def low_free_kills(self) -> int:
        with self._lock:
            return sum(1 for k in self.kills if k.get("reason") == "low-free")

    def _kill(self, proc, rss: int, reason: str = "cap", free: int = None):
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
                  "reason": reason, "rss_gb": round(rss / 2**30, 2), "cap_gb": self.cap_gb,
                  "ancestors": ancestors, "cmdline": cmdline[:200]}
        if free is not None:
            record["free_gb"] = round(free / 2**30, 2)
            record["min_free_gb"] = self.min_free_gb
        with self._lock:
            self.kills.append(record)
            try:
                with open(self.kills_path, "a", encoding="utf-8") as fh:
                    fh.write(json.dumps(record) + "\n")
            except OSError:
                pass
        if reason == "low-free":
            self.log(f"  rss cap: low free memory ({record['free_gb']} GB < {self.min_free_gb} GB) "
                     f"— killed the biggest lumen pid {proc.pid} at {record['rss_gb']} GB")
        else:
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

    GB = 2**30

    class FakePs:
        Error = FakeError
        free = 20 * GB
        swap = 0
        root = None

        @classmethod
        def Process(cls, pid):  # noqa: N802 — psutil's name
            assert pid == 100
            return cls.root

        @classmethod
        def virtual_memory(cls):
            return type("V", (), {"available": cls.free})()

        @classmethod
        def swap_memory(cls):
            return type("S", (), {"used": cls.swap})()

    def tree(*rss_gb):
        """A run: root -> shard -> one lumen per `rss_gb` entry, plus a stranger."""
        root = Proc(100, "python.exe", 0)
        smoke = Proc(200, "python.exe", 0, parents=[root])
        browsers = [Proc(301 + n, "lumen.exe", int(gb * GB), parents=[smoke, root])
                    for n, gb in enumerate(rss_gb)]
        other = Proc(399, "notlumen.exe", 30 * GB, parents=[smoke, root])
        root._children = [smoke, *browsers, other]
        FakePs.root = root
        return browsers, other

    failures = []

    def expect(condition, message):
        if not condition:
            failures.append(message)

    with tempfile.TemporaryDirectory() as tmp:
        # Per-browser cap (WPT-RUN-9).
        (fat, thin, _extra), other = tree(6, 1, 1)
        cap = BrowserRssCap(4.0, tmp, root_pid=100, log=lambda *_: None)
        cap.psutil = FakePs
        made = cap.check_once()
        expect([k["pid"] for k in made] == [301], f"killed {[k['pid'] for k in made]}, expected [301]")
        expect(fat.killed and not thin.killed and not other.killed, "wrong process killed")
        expect([k["pid"] for k in cap.kills_under(200)] == [301] and not cap.kills_under(999),
               "kills_under attribution wrong")
        expect([k["pid"] for k in load_kills(tmp)] == [301], "kill not persisted")
        expect(made[0]["reason"] == "cap", "cap kill must say so")
        off = BrowserRssCap(0, tmp, log=lambda *_: None)
        expect(not off.start(), "cap 0 with no memory floor must stay off")

    # Low-free trigger (WPT-RUN-18).
    with tempfile.TemporaryDirectory() as tmp:
        clock = [1000.0]
        cap = BrowserRssCap(0, tmp, root_pid=100, log=lambda *_: None, min_free_gb=6,
                            clock=lambda: clock[0])
        cap.psutil = FakePs
        expect(cap.cap_bytes == 0 and cap.min_free_bytes == 6 * GB, "min-free alone must arm")

        browsers, other = tree(3.0, 2.5, 0.8)
        FakePs.free = 20 * GB
        expect(not cap.check_once(), "plenty of memory: nothing to kill")

        FakePs.free = 3 * GB
        made = cap.check_once()
        expect([k["pid"] for k in made] == [301] and made[0]["reason"] == "low-free",
               f"short on memory: kill the biggest, got {made}")
        expect(made and made[0]["free_gb"] == 3.0 and made[0]["min_free_gb"] == 6,
               "low-free record must carry the free memory it reacted to")
        expect(browsers[0].killed and not browsers[1].killed and not browsers[2].killed,
               "only the biggest browser dies")
        expect(cap.low_free_kills() == 1, "low_free_kills must count it")

        # Still short right after: the cooldown spares the next one.
        browsers[0]._rss = 0
        clock[0] += 1
        expect(not cap.check_once(), "no second kill inside the cooldown")
        clock[0] += LOW_FREE_COOLDOWN_S
        made = cap.check_once()
        expect([k["pid"] for k in made] == [302], f"after the cooldown the next biggest, got {made}")

        # Only small browsers: a shortage they did not cause is not answered.
        clock[0] += LOW_FREE_COOLDOWN_S + 1
        tree(1.5, 0.9)
        expect(not cap.check_once(), "browsers under the floor are never killed for low memory")
        expect(cap.low_free_polls >= 3 and cap.min_free_seen == 3 * GB, "shortage statistics")

        # Cap and low-free in one poll: the capped one dies once, as `cap`.
        clock[0] += LOW_FREE_COOLDOWN_S + 1
        cap2 = BrowserRssCap(4.0, tmp, root_pid=100, log=lambda *_: None, min_free_gb=6,
                             clock=lambda: clock[0])
        cap2.psutil = FakePs
        tree(7.0, 3.0)
        made = cap2.check_once()
        expect([(k["pid"], k["reason"]) for k in made] == [(301, "cap"), (302, "low-free")],
               f"cap first, then the next biggest for low memory: {made}")

        # Memory log: header once, rows throttled, numbers readable.
        FakePs.free, FakePs.swap = 12 * GB, 1 * GB
        tree(1.0, 2.0)
        log_cap = BrowserRssCap(0, tmp, root_pid=100, log=lambda *_: None, min_free_gb=1,
                                clock=lambda: clock[0])
        log_cap.psutil = FakePs
        log_cap.check_once()
        log_cap.maybe_log_memory()
        log_cap.maybe_log_memory()  # inside MEM_LOG_EVERY_S: no second row
        clock[0] += MEM_LOG_EVERY_S
        log_cap.maybe_log_memory()
        with open(log_cap.mem_log_path, encoding="utf-8") as fh:
            lines = fh.read().splitlines()
        expect(len(lines) == 3 and lines[0].startswith("time\tfree_gb"), f"mem log shape: {lines}")
        fields = lines[1].split("\t")
        expect(fields[1:] == ["12.00", "1.00", "2", "3.00", "2.00"], f"mem log row: {fields}")
        expect(log_cap.max_swap_seen == 1 * GB, "swap high-water mark")
    for failure in failures:
        print("FAIL:", failure)
    print("browser_rss_cap selftest:", "FAIL" if failures else "ok")
    return 1 if failures else 0


def _sample_main(argv: list) -> int:
    """`--sample PID --out FILE [--interval S]`: write the memory log of a run
    that has no watchdog of its own (the "before" arm of a WPT-RUN-18 A/B), until
    process PID ends."""
    import argparse  # noqa: PLC0415

    parser = argparse.ArgumentParser(prog="browser_rss_cap.py")
    parser.add_argument("--sample", type=int, required=True, metavar="PID",
                        help="log free memory, swap and the lumen browsers under PID")
    parser.add_argument("--out", required=True, help="memory log file (tab-separated)")
    parser.add_argument("--interval", type=float, default=MEM_LOG_EVERY_S)
    args = parser.parse_args(argv)
    ps = _psutil()
    if ps is None:
        print("psutil is not installed", file=sys.stderr)
        return 1
    sampler = BrowserRssCap(0, os.path.dirname(os.path.abspath(args.out)) or ".",
                            root_pid=args.sample)
    sampler.mem_log_path = args.out
    while ps.pid_exists(args.sample):
        sampler.check_once()
        sampler.maybe_log_memory(force=True)
        time.sleep(args.interval)
    return 0


if __name__ == "__main__":
    raise SystemExit(_sample_main(sys.argv[1:]) if len(sys.argv) > 1 else _selftest())
