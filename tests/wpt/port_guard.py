#!/usr/bin/env python3
"""WPT-RUN-5 slice 18 (`docs/tasks/p2-wpt-runner-throughput.md`): make sure a
corpus run owns the `wptserve` ports it is about to use.

Why this exists. `tests/wpt/config.json` pins every server port (18300/18301
http, 18443/18444 https, 18888/18889 ws/wss, 19000 h2) — they are *fixed*, so
two runs on one machine cannot coexist, and a server left behind by a dead run
keeps them for as long as it lives. `wptserve` runs each of those servers as a
`multiprocessing` child; killing the run above it (Ctrl-C, a session teardown,
an OOM kill) does not reach them, and they get reparented to init and stay.

What that costs, measured on the 2026-08-20 corpus run:

* on Linux the squatter *answers*, so the run keeps going and looks healthy.
  Of its 342 finished shards, **328 carry `Address already in use` and the
  other 14 never reached server startup** ("Unable to find any tests") — that
  is every shard that got as far as needing a server, so not one of them was
  served by a server this run started. The holders (7 pids, one per configured
  port) began at 16:10:27, 4.5 minutes before the run itself, i.e. they are the
  leftovers of the run that died right before it;
* what that does to the verdicts is *not* nothing, but it is subtler than a
  wrong answer. Probed on the live run: the squatter serves the tree as it is
  on disk right now (a file created seconds before the probe came back 200), so
  the test files are the run's own; the `/resources/testharnessreport.js` it
  hands out is Lumen's, not `wptrunner`'s default — the difference from the
  file in `tests/wpt/resources/` is exactly the four `%(...)s` placeholders
  `TestEnvironment.get_routes` substitutes. But it substitutes them **when the
  server starts**, which is why that is the hazard: `output`, `debug`,
  `timeout_multiplier` and `explicit_timeout` reaching the page are the *dead*
  run's options, not the live one's, and the same goes for every other static
  route `browsers/lumen.py::env_options` configures. This run is trustworthy
  only because the run that died had identical options — nothing in the report
  says so, and nothing checked;
* on Windows the squatter does not answer, and the shard dies ~11 s in with an
  empty report — the 158 shards / 15 753 manifest ids that slice 16 found
  missing from the published Windows figure.

So the fix slice 16 landed (notice the empty report, retry once, retry again on
`--resume`) cannot work on its own: the retry hits the same immortal squatter.
The ports have to be *taken back* first, which is what this module does.

Policy — deliberately narrow, because the neighbouring processes belong to
other developers' sessions (five worktree pool slots, `docs/git-workflow.md`):

* nothing is killed unless it holds one of the configured ports;
* a port holder whose ancestry still contains a live `run_corpus.py` /
  `run_smoke.py` / `wptrunner` is another *running* corpus run — never killed,
  and the caller is told to stop rather than fight it for the port;
* except when that live runner is the caller itself (`own_pid`): the previous
  shard's server has not finished exiting yet, and a run has to be able to take
  its own ports back. Without this case the guard would abort runs on their own
  leftovers — the rule above matches "a corpus run is alive above it", and ours
  always is;
* a port holder that is an orphaned Python `multiprocessing` server (no live
  runner above it) is a leak by definition and is killed;
* anything else (some unrelated program on 18300) is reported, never killed.

Usage:

    <venv>/python tests/wpt/port_guard.py --report     # who holds the ports
    <venv>/python tests/wpt/port_guard.py --reclaim    # take them back
    <venv>/python tests/wpt/port_guard.py --selftest   # prove the above
"""

import argparse
import json
import os
import socket
import subprocess
import sys
import time

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TESTS_ROOT = os.path.join(REPO_ROOT, "tests", "wpt")
CONFIG_PATH = os.path.join(TESTS_ROOT, "config.json")

#: Command-line fragments that mean "a corpus run is alive above this process".
#: Matched against the whole ancestry, not just the parent: a `wptserve` child
#: sits under `run_smoke.py`, which sits under `run_corpus.py`.
RUNNER_MARKERS = ("run_corpus.py", "run_smoke.py", "run_report.py", "run_suite.py",
                  "wptrunner")

#: Command-line fragments that identify a `wptserve` server child. `wptserve`
#: starts each server through `multiprocessing`, so the child's command line is
#: the interpreter plus `multiprocessing.spawn`/`forkserver` bootstrap — the
#: server module's name never appears in it.
SERVER_MARKERS = ("multiprocessing", "wptserve", "serve.py")

#: How long to wait for a port to come back on its own before deciding it is
#: held. A server that has just been asked to stop needs a moment; a leak does
#: not care how long we wait.
DEFAULT_SETTLE_SECONDS = 5.0


class PortsBusy(RuntimeError):
    """Raised when the configured ports cannot be made available."""


def configured_ports(config_path: str = CONFIG_PATH) -> list:
    """Every TCP port `wptserve` will try to bind, from `config.json`.

    Read from the same file `environment.py` overrides its defaults with
    (`serve_path(test_paths)/config.json`), so this cannot drift from what the
    run actually binds. `null` entries mean "not configured" and are skipped —
    that is how `http-local`/`webtransport-h3` are switched off.
    """
    with open(config_path, encoding="utf-8") as fh:
        config = json.load(fh)
    ports = set()
    for values in (config.get("ports") or {}).values():
        for value in values or []:
            if isinstance(value, int):
                ports.add(value)
    return sorted(ports)


def listening(ports, host: str = "127.0.0.1") -> list:
    """The subset of `ports` something is already accepting connections on.

    A connect probe rather than a bind probe on purpose: `SO_REUSEADDR` lets a
    bind succeed against a socket in `TIME_WAIT`, which is exactly the state we
    do *not* want to report as busy.
    """
    busy = []
    for port in ports:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(0.4)
            if sock.connect_ex((host, port)) == 0:
                busy.append(port)
    return busy


def _process_table() -> dict:
    """`{pid: (ppid, command_line)}` for every process, as one snapshot."""
    table = {}
    if os.name == "nt":
        out = subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             "Get-CimInstance Win32_Process | "
             "ForEach-Object { \"$($_.ProcessId)`t$($_.ParentProcessId)`t$($_.CommandLine)\" }"],
            capture_output=True, text=True, check=False).stdout
        separator = "\t"
    else:
        out = subprocess.run(["ps", "-eo", "pid=,ppid=,args="],
                             capture_output=True, text=True, check=False).stdout
        separator = None
    for line in out.splitlines():
        parts = line.split(separator, 2) if separator else line.split(None, 2)
        if len(parts) < 2:
            continue
        try:
            pid, ppid = int(parts[0]), int(parts[1])
        except ValueError:
            continue
        table[pid] = (ppid, parts[2] if len(parts) > 2 else "")
    return table


def _port_holders(ports) -> dict:
    """`{port: pid}` for the listening sockets on `ports`.

    Three platform tools, one shape. Only called when a port is actually busy,
    so the normal path of a healthy run spawns no subprocess at all.
    """
    holders = {}
    wanted = {str(port) for port in ports}
    if os.name == "nt":
        out = subprocess.run(["netstat", "-ano", "-p", "TCP"],
                             capture_output=True, text=True, check=False).stdout
        for line in out.splitlines():
            fields = line.split()
            if len(fields) < 5 or fields[3] != "LISTENING":
                continue
            port = fields[1].rsplit(":", 1)[-1]
            if port in wanted:
                try:
                    holders[int(port)] = int(fields[4])
                except ValueError:
                    pass
        return holders

    tools = [["ss", "-ltnpH"], ["lsof", "-nP", "-iTCP", "-sTCP:LISTEN"]]
    for argv in tools:
        try:
            out = subprocess.run(argv, capture_output=True, text=True, check=False).stdout
        except OSError:
            continue
        if not out.strip():
            continue
        for line in out.splitlines():
            for port in ports:
                if f":{port}" not in line:
                    continue
                pid = None
                if "pid=" in line:                        # ss
                    pid = line.split("pid=", 1)[1].split(",", 1)[0]
                else:                                     # lsof
                    fields = line.split()
                    if len(fields) > 1 and f":{port}" in line:
                        pid = fields[1]
                try:
                    holders.setdefault(port, int(pid))
                except (TypeError, ValueError):
                    pass
        if holders:
            break
    return holders


def _ancestry(pid: int, table: dict) -> list:
    """`pid` and every ancestor of it, as `(pid, command_line)`, root last."""
    chain = []
    seen = set()
    while pid and pid not in seen and pid in table:
        seen.add(pid)
        ppid, cmd = table[pid]
        chain.append((pid, cmd))
        pid = ppid
    return chain


def classify(pid: int, table: dict, own_pid: int = None) -> str:
    """What kind of thing is holding a WPT port.

    * `stale` — a server of *our own* run: the previous shard's `wptserve` has
      not finished exiting yet. Reclaimable, and it has to be, because the
      ancestry test below would otherwise call it `live` (our `run_corpus.py`
      really is alive above it) and make a run abort on its own leftovers.
      Only ever true between shards — nothing else calls the guard.
    * `live` — a corpus run is still alive above it. Someone else is testing on
      this machine; taking its port would corrupt their results and ours.
    * `orphan` — a `wptserve` server child with no runner above it. A leak.
    * `foreign` — anything else on the port.
    * `gone` — it exited between the socket scan and the process scan.
    """
    chain = _ancestry(pid, table)
    if not chain:
        return "gone"
    if own_pid is not None and any(cpid == own_pid for cpid, _cmd in chain):
        return "stale"
    if any(marker in cmd for _pid, cmd in chain for marker in RUNNER_MARKERS):
        return "live"
    own_cmd = chain[0][1]
    if any(marker in own_cmd for marker in SERVER_MARKERS):
        return "orphan"
    return "foreign"


#: Holder kinds a run may take a port back from: a leak, or its own previous
#: shard still exiting. Everything else belongs to somebody.
RECLAIMABLE = ("orphan", "stale")


def survey(ports=None, own_pid: int = None) -> list:
    """`[(port, pid, kind, command_line)]` for every busy configured port."""
    ports = configured_ports() if ports is None else ports
    busy = listening(ports)
    if not busy:
        return []
    table = _process_table()
    holders = _port_holders(busy)
    rows = []
    for port in busy:
        pid = holders.get(port)
        if pid is None:
            rows.append((port, None, "foreign", "<owner not identified>"))
            continue
        rows.append((port, pid, classify(pid, table, own_pid), table.get(pid, (0, ""))[1]))
    return rows


def reclaim(ports=None, settle: float = DEFAULT_SETTLE_SECONDS, dry_run: bool = False,
            own_pid: int = None, log=print) -> dict:
    """Make the configured ports available, or explain why that is impossible.

    Returns `{"killed": [pid, ...], "busy": [(port, pid, kind, cmd), ...]}`;
    `busy` empty means the caller owns the ports. Raises nothing — refusing to
    kill is a result, not an error, and the caller decides how loud that is.
    """
    ports = configured_ports() if ports is None else ports
    rows = survey(ports, own_pid)
    if not rows:
        return {"killed": [], "busy": []}

    killed = []
    for port, pid, kind, cmd in rows:
        if kind not in RECLAIMABLE or pid is None:
            continue
        log(f"port {port}: {kind} wptserve pid {pid} — {cmd[:110]}")
        if dry_run:
            continue
        try:
            if os.name == "nt":
                subprocess.run(["taskkill", "/F", "/PID", str(pid)],
                               capture_output=True, check=False)
            else:
                os.kill(pid, 9)
            killed.append(pid)
        except (OSError, ProcessLookupError) as exc:
            log(f"port {port}: could not kill pid {pid}: {exc}")

    if killed and not dry_run:
        deadline = time.time() + settle
        while time.time() < deadline and listening(ports):
            time.sleep(0.2)
    return {"killed": killed, "busy": survey(ports, own_pid)}


def ensure_free(ports=None, settle: float = DEFAULT_SETTLE_SECONDS, reclaim_orphans: bool = True,
                own_pid: int = None, log=print) -> list:
    """Guarantee the run owns its ports, or raise `PortsBusy` naming the holder.

    Called before the first shard and before every shard after it. The failure
    it prevents is not a crash: it is a run that keeps going and quietly scores
    zero (Windows) or answers out of a server it does not control (Linux).
    """
    ports = configured_ports() if ports is None else ports
    busy = listening(ports)
    if not busy:
        return []
    # A shard that has just exited can leave its sockets closing; that clears
    # on its own and is not worth a report.
    deadline = time.time() + settle
    while time.time() < deadline and busy:
        time.sleep(0.25)
        busy = listening(ports)
    if not busy:
        return []

    if reclaim_orphans:
        result = reclaim(ports, settle=settle, own_pid=own_pid, log=log)
        if not result["busy"]:
            if result["killed"]:
                log(f"port guard: reclaimed {len(result['killed'])} stranded wptserve "
                    f"process(es) squatting on {', '.join(str(p) for p in busy)}")
            return result["killed"]
        rows = result["busy"]
    else:
        rows = survey(ports, own_pid)

    detail = "; ".join(f"{port} held by pid {pid} ({kind})" for port, pid, kind, _cmd in rows)
    live = [row for row in rows if row[2] == "live"]
    hint = ("another corpus run is using this machine — wait for it or stop it"
            if live else
            "stop whatever is on these ports; a corpus run cannot share them")
    raise PortsBusy(f"WPT ports unavailable: {detail}. {hint}.")


def _free_port() -> int:
    """An ephemeral port nothing is listening on at this instant."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def _selftest() -> int:
    """Prove detect → classify → reclaim on a port nothing else uses.

    Uses a synthetic squatter rather than a real `wptserve`: the thing under
    test is the ownership logic, and a real server would need the whole corpus
    environment to start.
    """
    port = _free_port()

    # The squatter has to *accept*, not just listen: `listening()` probes by
    # connecting, and a backlog that is never drained starts refusing after the
    # first couple of probes — which would make the port look free again.
    squatter_src = (
        "import socket,threading,time\n"
        "import multiprocessing\n"           # the marker `classify` looks for
        "s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)\n"
        f"s.bind(('127.0.0.1',{port}));s.listen(16)\n"
        "def serve():\n"
        "    while True:\n"
        "        conn,_=s.accept();conn.close()\n"
        "threading.Thread(target=serve,daemon=True).start()\n"
        "time.sleep(600)\n"
    )
    failures = []

    def check(label, condition, detail=""):
        print(f"  {'ok  ' if condition else 'FAIL'} {label}{(' — ' + detail) if detail else ''}")
        if not condition:
            failures.append(label)

    print(f"selftest on port {port}")
    check("free port reports no holder", listening([port]) == [])

    squatter = subprocess.Popen([sys.executable, "-c", squatter_src],
                                start_new_session=(os.name != "nt"))
    try:
        deadline = time.time() + 10
        while time.time() < deadline and not listening([port]):
            time.sleep(0.1)
        check("bound port is detected", listening([port]) == [port])
        rows = survey([port])
        check("holder is identified", bool(rows) and rows[0][1] == squatter.pid,
              f"{rows}")
        check("holder classified as orphan", bool(rows) and rows[0][2] == "orphan",
              rows[0][2] if rows else "no rows")
        # A live runner above the same process must be refused, not killed.
        table = dict(_process_table())
        table[squatter.pid] = (os.getpid(), table.get(squatter.pid, (0, ""))[1])
        table[os.getpid()] = (1, "python tests/wpt/run_corpus.py --all")
        check("holder under a live runner is spared",
              classify(squatter.pid, table) == "live")
        # ...unless that live runner is us. A run must be able to take back the
        # ports of its own previous shard, which is still exiting under it.
        check("holder under this very run is reclaimable as stale",
              classify(squatter.pid, table, own_pid=os.getpid()) == "stale")
        result = reclaim([port])
        check("orphan is killed", squatter.pid in result["killed"])
        check("port is free afterwards", result["busy"] == [], f"{result['busy']}")
        # A *fresh* ephemeral port, not the one just vacated: this machine runs
        # corpus runs of its own, whose browsers listen on ephemeral ports, and
        # one of them can take the vacated number between the kill and here.
        # That is a flake in the test, not a finding about the guard.
        clean = _free_port()
        check("ensure_free passes on a clean port", ensure_free([clean]) == [],
              f"port {clean}")
    finally:
        if squatter.poll() is None:
            squatter.kill()
        squatter.wait()

    print(f"selftest: {'PASS' if not failures else 'FAIL (' + ', '.join(failures) + ')'}")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--report", action="store_true",
                        help="print who holds the configured WPT ports and exit")
    parser.add_argument("--reclaim", action="store_true",
                        help="kill orphaned wptserve processes holding those ports")
    parser.add_argument("--dry-run", action="store_true",
                        help="with --reclaim: name what would be killed, kill nothing")
    parser.add_argument("--selftest", action="store_true",
                        help="verify detection/classification/reclaim on a spare port")
    args = parser.parse_args()

    if args.selftest:
        return _selftest()

    ports = configured_ports()
    rows = survey(ports)
    print(f"configured ports: {', '.join(str(port) for port in ports)}")
    if not rows:
        print("all free")
        return 0
    for port, pid, kind, cmd in rows:
        print(f"  {port}: {kind} pid={pid} {cmd[:120]}")
    if not args.reclaim:
        return 1
    result = reclaim(ports, dry_run=args.dry_run)
    print(f"killed: {result['killed'] or 'nothing'}")
    if result["busy"]:
        print(f"still busy: {[(row[0], row[2]) for row in result['busy']]}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
