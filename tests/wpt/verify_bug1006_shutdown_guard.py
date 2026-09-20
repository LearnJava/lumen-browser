#!/usr/bin/env python3
"""BUG-1006: pins for `shutdown_guard.py` — a run that receives SIGTERM/SIGBREAK
must exit in bounded time even when a non-daemon thread never returns and a
child process is still alive.

Each case runs in a fresh child interpreter (the guard ends with `os._exit`).
The child reproduces the shape that hung the real runner: a NON-daemon thread
blocked forever (stand-in for `TestRunnerManager`/`ProcessReader`) plus a live
grandchild process (stand-in for the orphaned `lumen` that holds the pipes).

Usage: tests/wpt/.venv/Scripts/python.exe tests/wpt/verify_bug1006_shutdown_guard.py
"""

import os
import subprocess
import sys
import textwrap
import time

HERE = os.path.dirname(os.path.abspath(__file__))
PY = sys.executable

#: Generous upper bound for a 1 s grace — the point is "bounded", not "fast".
DEADLINE = 20.0


def run_child(body: str, timeout: float = DEADLINE + 10):
    """Runs `body` in a fresh interpreter with `tests/wpt` importable; returns
    (exit code, wall seconds, stdout)."""
    code = f"import sys; sys.path.insert(0, {HERE!r})\n" + textwrap.dedent(body)
    start = time.time()
    proc = subprocess.run([PY, "-c", code], capture_output=True, text=True, timeout=timeout)
    return proc.returncode, time.time() - start, proc.stdout


def check(name: str, ok: bool, detail: str = "") -> bool:
    print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  ({detail})" if detail else ""))
    return ok


#: The stuck non-daemon thread + a live grandchild that prints nothing.
STUCK = """
    import subprocess, sys, threading, time
    grandchild = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(600)"])
    print("GRANDCHILD", grandchild.pid, flush=True)
    threading.Thread(target=lambda: threading.Event().wait(), daemon=False).start()
"""


def case_watchdog_bounds_a_stuck_exit() -> bool:
    body = STUCK + """
    import shutdown_guard
    shutdown_guard.arm(grace=1.0)
    # main returns -> interpreter waits on the non-daemon thread forever,
    # unless the watchdog fires.
    """
    code, secs, out = run_child(body)
    pid = int(out.split("GRANDCHILD")[1].split()[0])
    import psutil
    alive = psutil.pid_exists(pid) and psutil.Process(pid).status() != psutil.STATUS_ZOMBIE
    ok = code == 143 and secs < DEADLINE and not alive
    if alive:
        psutil.Process(pid).kill()
    return check("armed watchdog ends a stuck interpreter and kills its child", ok,
                 f"exit={code}, {secs:.1f}s, grandchild_alive={alive}")


def case_unarmed_run_is_untouched() -> bool:
    code, secs, _ = run_child("import shutdown_guard\nprint('done')\n")
    return check("nothing armed on the normal path: clean exit 0", code == 0 and secs < DEADLINE,
                 f"exit={code}, {secs:.1f}s")


def case_signal_handler_arms_and_still_interrupts() -> bool:
    body = STUCK + """
    import signal, types
    import shutdown_guard

    sig = signal.SIGBREAK if sys.platform == "win32" else signal.SIGTERM

    def upstream_install():
        def termination_handler(_signum, _frame):
            raise KeyboardInterrupt()
        signal.signal(sig, termination_handler)

    fake = types.SimpleNamespace(handle_interrupt_signals=upstream_install)
    shutdown_guard.install(fake, grace=1.0)
    fake.handle_interrupt_signals()
    try:
        signal.getsignal(sig)(sig, None)
    except KeyboardInterrupt:
        print("INTERRUPTED", flush=True)
    # falling off the end leaves the non-daemon thread -> only the watchdog
    # armed by the handler can end this process.
    """
    code, secs, out = run_child(body)
    ok = code == 143 and "INTERRUPTED" in out and secs < DEADLINE
    return check("signal handler arms the watchdog and still raises KeyboardInterrupt", ok,
                 f"exit={code}, {secs:.1f}s, interrupted={'INTERRUPTED' in out}")


def case_install_is_idempotent() -> bool:
    body = """
    import types
    import shutdown_guard
    fake = types.SimpleNamespace(handle_interrupt_signals=lambda: None)
    shutdown_guard.install(fake)
    first = fake.handle_interrupt_signals
    shutdown_guard.install(fake)
    print("SAME" if fake.handle_interrupt_signals is first else "DOUBLE-WRAPPED")
    """
    _, _, out = run_child(body)
    return check("install() twice does not double-wrap", "SAME" in out, out.strip())


def main() -> int:
    results = [
        case_unarmed_run_is_untouched(),
        case_install_is_idempotent(),
        case_watchdog_bounds_a_stuck_exit(),
        case_signal_handler_arms_and_still_interrupts(),
    ]
    print(f"{sum(results)}/{len(results)} passed")
    return 0 if all(results) else 1


if __name__ == "__main__":
    sys.exit(main())
