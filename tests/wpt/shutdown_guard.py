"""BUG-1006: bounded shutdown for `run_smoke.run()` / `run_report.py`.

Upstream `wptrunner` answers SIGTERM/SIGBREAK with `KeyboardInterrupt` in the
main thread (`wptrunner.handle_interrupt_signals`), then gives the
`TestRunnerManager` threads 10 s (`ManagerGroup.wait(timeout=10)`) and
re-raises. Those threads are NON-daemon, so the interpreter does not exit
until each of them returns from its own cleanup — and that cleanup can block
indefinitely: `mozprocess` starts `lumen` in a NEW process group
(`os.setpgid(0, 0)`), so a group-wide SIGTERM (`timeout`, a CI cancel) kills
python and the runner subprocess but leaves the browser alive as an orphan
that still holds the stdout/stderr pipes; the `ProcessReader` threads then sit
in `read()` (`anon_pipe_read`) for good and `threading._shutdown` waits on them
(`futex_wait` in the main thread). A plain `timeout N` (no `-k`) never gets the
run to exit, and the run leaves no verdict line behind.

`arm()` starts a daemon watchdog: after `grace` seconds it kills every
descendant process (the orphan browser included) and hard-exits with
`EXIT_CODE`. A daemon thread is exactly right here — `threading._shutdown`
skips joining daemons, so the watchdog is still alive while the interpreter
waits on the stuck managers. Nothing is armed on the normal path, so a run that
finishes on its own is untouched.
"""

import os
import signal
import sys
import threading
import time

import psutil

#: 128 + SIGTERM, what a shell reports for a process killed by SIGTERM.
EXIT_CODE = 143

#: Seconds granted to the normal cooperative shutdown (`ManagerGroup.wait`'s own
#: 10 s plus browser/runner teardown) before the watchdog steps in.
DEFAULT_GRACE = 30.0

_armed = threading.Event()


def kill_descendants() -> int:
    """SIGKILL every descendant of this process; returns how many were signalled.

    Children first-to-last is fine: killing a parent does not spare its
    children (they are enumerated up front, before any is killed)."""
    try:
        children = psutil.Process().children(recursive=True)
    except psutil.Error:
        return 0
    killed = 0
    for child in children:
        try:
            child.kill()
            killed += 1
        except psutil.Error:
            pass
    return killed


def _watchdog(grace: float, exit_code: int) -> None:
    time.sleep(grace)
    kill_descendants()
    # `os._exit`, not `sys.exit`: the point is to skip `threading._shutdown`,
    # which is the thing that is stuck.
    os._exit(exit_code)


def arm(grace: float = DEFAULT_GRACE, exit_code: int = EXIT_CODE) -> bool:
    """Start the watchdog once; returns False if it was already armed."""
    if _armed.is_set():
        return False
    _armed.set()
    threading.Thread(target=_watchdog, args=(grace, exit_code), name="bug1006-shutdown-watchdog",
                     daemon=True).start()
    return True


def install(wptrunner_module, grace: float = DEFAULT_GRACE) -> None:
    """Wrap `wptrunner.handle_interrupt_signals` so the SIGTERM/SIGBREAK it
    installs also arms the watchdog, at signal time. `run_test_iteration`
    resolves `handle_interrupt_signals` as a module global on every call, so
    replacing the attribute is enough — no vendored file is edited."""
    original = wptrunner_module.handle_interrupt_signals
    if getattr(original, "_bug1006_wrapped", False):
        return

    def wrapped():
        original()
        # `original` bound its own handler; grab it and chain ours in front so
        # it still raises `KeyboardInterrupt` exactly as before.
        sig = signal.SIGBREAK if sys.platform == "win32" else signal.SIGTERM
        upstream = signal.getsignal(sig)

        def handler(signum, frame):
            arm(grace)
            if callable(upstream):
                upstream(signum, frame)

        signal.signal(sig, handler)

    wrapped._bug1006_wrapped = True
    wptrunner_module.handle_interrupt_signals = wrapped
