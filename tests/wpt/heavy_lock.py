#!/usr/bin/env python3
"""BUG-1029 §3: a courtesy lock so a heavy `cargo build` and a WPT corpus run
don't pile their memory on top of each other on one machine.

Why this exists. The two 2026-09-07 OOM kills were dominated by *orphaned*
`lumen` processes (BUG-1029 §§1-2, fixed by `port_guard.py`'s
`reap_lumen_orphans`), but both episodes also had a live `rustc`/linker
running at the same time — a WPT run's browsers (~1 GB RSS each,
`--processes` of them) plus a build (~0.5-1 GB per job) is enough on its own
on a 7.6 GB box, orphans or not. There is no single choke point that already
serializes "build" and "WPT run" the way `port_guard.py` serializes two WPT
runs against each other (`ensure_free`'s port scan) — builds are invoked
directly by whichever session wants one, in any of the five worktree pool
slots, with no shared entry point to hook.

So this is an OS-level advisory lock on one file (`msvcrt.locking` on
Windows, `fcntl.flock` elsewhere — both released automatically if the holder
dies, unlike a hand-rolled pidfile, which is the point: nothing has to clean
up after a `kill -9`), used two ways:

* a WPT run (`run_report.py`/`run_corpus.py`) holds it for the run's
  duration;
* `scripts/cargo-heavy.sh` (or `heavy_lock.py run -- <cmd...>` directly)
  holds it for the duration of a build a session wants to keep off the WPT
  run's toes.

Deliberately a courtesy, not a hard gate: both sides wait up to `timeout`
seconds for the lock, then proceed anyway with a loud warning. A hard block
would let one side wedge the other for as long as it runs — a multi-hour
`--all` corpus run would then stall every session's build on the machine,
which is worse than the rare OOM this exists to reduce. Use `--report` to see
who holds it right now without touching it.
"""

import argparse
import contextlib
import os
import subprocess
import sys
import time

LOCK_PATH = os.path.join(os.path.dirname(__file__), ".heavy.lock")
#: Holder description lives in its own file, not in `LOCK_PATH` itself: on
#: Windows `msvcrt.locking` is a *mandatory* OS-level lock on the byte range
#: it covers, so a second process plainly opening `LOCK_PATH` to read the
#: current holder's label can hit that locked region and fail — writing the
#: label into a sibling file every other reader is free to open keeps the
#: diagnostic read independent of the lock byte.
OWNER_PATH = LOCK_PATH + ".owner"

#: How long a caller waits for the lock before giving up and proceeding
#: unlocked anyway. Long enough to ride out a normal build (a few minutes) or
#: the gap between two WPT shards; short enough that a multi-hour corpus run
#: does not wedge every session's build for its whole duration.
DEFAULT_TIMEOUT = 300.0
POLL_SECONDS = 2.0

if os.name == "nt":
    import msvcrt

    def _try_lock(fh) -> bool:
        fh.seek(0)
        try:
            msvcrt.locking(fh.fileno(), msvcrt.LK_NBLCK, 1)
            return True
        except OSError:
            return False

    def _unlock(fh) -> None:
        fh.seek(0)
        with contextlib.suppress(OSError):
            msvcrt.locking(fh.fileno(), msvcrt.LK_UNLCK, 1)
else:
    import fcntl

    def _try_lock(fh) -> bool:
        try:
            fcntl.flock(fh.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except OSError:
            return False

    def _unlock(fh) -> None:
        with contextlib.suppress(OSError):
            fcntl.flock(fh.fileno(), fcntl.LOCK_UN)


def _write_owner(owner: str) -> None:
    """Record who holds the lock, for `_read_holder`/`--report`.

    Written next to `OWNER_PATH` then `os.replace`d into place so a
    concurrent reader never sees a half-written label (the write is not
    protected by the OS lock — see `OWNER_PATH`'s comment).
    """
    tmp_path = OWNER_PATH + f".tmp{os.getpid()}"
    with open(tmp_path, "w", encoding="utf-8") as fh:
        fh.write(f"{owner} pid={os.getpid()} since={time.ctime()}")
    os.replace(tmp_path, OWNER_PATH)


def _read_holder() -> str:
    """Best-effort description of who last wrote the lock file.

    Racy by nature (the holder can change between this read and the caller
    acting on it) — diagnostic text for a log line, not something to branch
    on.
    """
    try:
        with open(OWNER_PATH, encoding="utf-8", errors="replace") as fh:
            return fh.read().strip() or "<unknown>"
    except OSError:
        return "<unknown>"


@contextlib.contextmanager
def heavy_lock(owner: str, timeout: float = DEFAULT_TIMEOUT,
               poll: float = POLL_SECONDS, log=print):
    """Hold the machine-wide heavy-work lock for the duration of the `with`
    block, waiting up to `timeout` seconds if someone else has it.

    Yields `True` if the lock was actually acquired, `False` if `timeout`
    ran out and the caller is proceeding unlocked — callers that only want
    the courtesy, not a hard dependency, can ignore the yielded value; ones
    that want to know why the OOM risk was not reduced this run can log it.
    Never raises for contention; a lock file that cannot be created/opened at
    all — read-only filesystem, permissions — is not this feature's problem
    to solve, so that still raises `OSError`.
    """
    os.makedirs(os.path.dirname(LOCK_PATH), exist_ok=True)
    fh = open(LOCK_PATH, "a+b")  # noqa: SIM115 - lifetime is this context manager
    try:
        acquired = _try_lock(fh)
        if not acquired:
            holder = _read_holder()
            log(f"heavy lock: held by {holder!r} — waiting up to {timeout:.0f}s "
                f"({owner})")
            deadline = time.time() + timeout
            while time.time() < deadline:
                time.sleep(poll)
                if _try_lock(fh):
                    acquired = True
                    break
            if not acquired:
                log(f"heavy lock: still held by {_read_holder()!r} after "
                    f"{timeout:.0f}s — proceeding unlocked ({owner}); this is "
                    f"the exact combination BUG-1029 flagged as an OOM risk")
        if acquired:
            _write_owner(owner)
        try:
            yield acquired
        finally:
            if acquired:
                _unlock(fh)
    finally:
        fh.close()


def _run(argv, owner: str, timeout: float) -> int:
    with heavy_lock(owner, timeout=timeout):
        return subprocess.run(argv, check=False).returncode


def _cmd_report(_args) -> int:
    acquired = None
    if os.path.exists(LOCK_PATH):
        fh = open(LOCK_PATH, "a+b")
        try:
            acquired = _try_lock(fh)
            if acquired:
                _unlock(fh)
        finally:
            fh.close()
    if acquired is None:
        print("heavy lock: never used yet (no lock file)")
    elif acquired:
        print("heavy lock: free")
    else:
        print(f"heavy lock: held by {_read_holder()!r}")
    return 0


def _cmd_run(args) -> int:
    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        print("nothing to run — usage: heavy_lock.py run [--owner ...] "
              "[--timeout N] -- <command...>", file=sys.stderr)
        return 2
    owner = args.owner or " ".join(command)
    return _run(command, owner, args.timeout)


def _selftest() -> int:
    """Prove acquire/contend/timeout-fallback/release, in-process.

    Runs the lock against itself with two independent file handles rather
    than two subprocesses — same `_try_lock`/`_unlock` code path a second
    process would exercise (it operates on the OS handle, not anything
    process-local), simpler to assert against.
    """
    failures = []

    def check(label, condition, detail=""):
        print(f"  {'ok  ' if condition else 'FAIL'} {label}{(' — ' + str(detail)) if detail else ''}")
        if not condition:
            failures.append(label)

    for stale in (LOCK_PATH, OWNER_PATH):
        with contextlib.suppress(OSError):
            os.remove(stale)

    os.makedirs(os.path.dirname(LOCK_PATH), exist_ok=True)
    holder_fh = open(LOCK_PATH, "a+b")
    try:
        check("first handle acquires the free lock", _try_lock(holder_fh))
        _write_owner("selftest-holder")

        contender_fh = open(LOCK_PATH, "a+b")
        try:
            check("second handle sees it busy", _try_lock(contender_fh) is False)
        finally:
            contender_fh.close()

        check("holder is readable from a third handle",
              "selftest-holder" in _read_holder(), _read_holder())

        logged = []
        with heavy_lock("selftest-waiter", timeout=0.3, poll=0.1,
                        log=logged.append) as acquired:
            check("heavy_lock times out and proceeds unlocked while busy",
                  acquired is False)
        check("timeout path logged the BUG-1029 warning",
              any("BUG-1029" in line for line in logged), logged)

        _unlock(holder_fh)
    finally:
        holder_fh.close()

    with heavy_lock("selftest-clean") as acquired:
        check("heavy_lock acquires a free lock", acquired is True)
        check("owner file names the clean acquirer",
              "selftest-clean" in _read_holder(), _read_holder())

    fresh_fh = open(LOCK_PATH, "a+b")
    try:
        check("lock is free again after the context manager exits",
              _try_lock(fresh_fh))
        _unlock(fresh_fh)
    finally:
        fresh_fh.close()

    for path in (LOCK_PATH, OWNER_PATH):
        with contextlib.suppress(OSError):
            os.remove(path)

    print(f"selftest: {'PASS' if not failures else 'FAIL (' + ', '.join(failures) + ')'}")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="subcommand", required=True)

    report_parser = sub.add_parser("report", help="print who currently holds the lock and exit")
    report_parser.set_defaults(func=_cmd_report)

    selftest_parser = sub.add_parser("selftest", help="verify acquire/contend/timeout/release")
    selftest_parser.set_defaults(func=lambda _args: _selftest())

    run_parser = sub.add_parser(
        "run", help="hold the lock for the duration of a command",
        epilog="usage: heavy_lock.py run [--owner LABEL] [--timeout N] -- <command...>")
    run_parser.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT,
                            help=f"seconds to wait for a busy lock before proceeding "
                                 f"unlocked (default: {DEFAULT_TIMEOUT:.0f})")
    run_parser.add_argument("--owner", default=None,
                            help="label recorded for --report while held "
                                 "(default: the wrapped command)")
    run_parser.add_argument("command", nargs=argparse.REMAINDER,
                            help="`-- <command...>`: propagates its exit code")
    run_parser.set_defaults(func=_cmd_run)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
