#!/usr/bin/env python3
"""Minimal V8 `--prof` tick-processor substitute (THREAD-3 срез 4, BUG-1034).

There is no vendored V8 tick-processor in this repo (it lives in V8's own
`tools/`, written against a specific V8 checkout, and pulling it in is a much
bigger dependency than one grep-sized script). This is a *from-scratch*,
much smaller reimplementation of just the "self-time by code range" half of
that tool: it bins the sampled program counters from `tick` lines into the
address ranges declared by `code-creation` lines and prints the hottest N.

Usage:
    python scripts/v8_prof_top.py <isolate-...-v8.log> [--top N]

Log format (informal, from V8 source — no public schema doc):
    code-creation,<type>,<vm_state>,<time_us>,<addr>,<size>,<name>[,<sfi>,<tier>]
    tick,<pc>,<time_us>,<is_external_callback>,<tos>,<vm_state>[,<stack_pc>...]

Known limitation (found by this script, not fixed by it): every `code-creation`
name in this codebase's logs is `" :<line>:<col>"` — blank function name AND
no script id — because `crates/js/src/v8_runtime.rs` always calls
`v8::Script::compile(tc, src, None)` with `origin = None`. Without an origin,
V8 has no resource name to log, and `<line>:<col>` alone does not disambiguate
between the ~10 scripts a real page loads concurrently (BUG-900 tracks the
same root cause for muted cross-origin error reporting — the fix belongs
there, not here). Treat the `name` column as "shape of the function", not
"which file it came from".
"""

from __future__ import annotations

import argparse
import bisect
from collections import Counter


def parse_log(path: str) -> tuple[list[tuple[int, int, str]], list[list[int]]]:
    """Returns (sorted code ranges as (start, end, label), tick stacks)."""
    ranges: list[tuple[int, int, str]] = []
    ticks: list[list[int]] = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            fields = line.rstrip("\n").split(",")
            if fields[0] == "code-creation" and len(fields) >= 7:
                try:
                    addr = int(fields[4], 16)
                    size = int(fields[5])
                except ValueError:
                    continue
                kind = fields[1]
                name = fields[6].strip() or "<anonymous>"
                ranges.append((addr, addr + size, f"{kind} {name}"))
            elif fields[0] == "tick" and len(fields) >= 6:
                try:
                    pc = int(fields[1], 16)
                    vm_state = int(fields[5])
                    stack = [int(a, 16) for a in fields[6:]]
                except ValueError:
                    continue
                # vm_state 0 == JS (see V8's StateTag) — the only state whose
                # samples land inside the code ranges we indexed above; GC/IDLE/
                # COMPILER/OTHER/EXTERNAL samples never match one and would only
                # inflate the "unresolved" bucket for no diagnostic value.
                if vm_state == 0:
                    ticks.append([pc, *stack])
    ranges.sort()
    return ranges, ticks


def find_range(starts: list[int], ranges: list[tuple[int, int, str]], pc: int) -> str | None:
    i = bisect.bisect_right(starts, pc) - 1
    if i < 0:
        return None
    start, end, label = ranges[i]
    return label if start <= pc < end else None


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("logfile")
    ap.add_argument("--top", type=int, default=20)
    args = ap.parse_args()

    ranges, ticks = parse_log(args.logfile)
    starts = [r[0] for r in ranges]

    self_time = Counter()
    unresolved = 0
    for stack in ticks:
        label = find_range(starts, ranges, stack[0])
        if label is None:
            unresolved += 1
            continue
        self_time[label] += 1

    total = len(ticks)
    print(f"{total} JS-state samples, {unresolved} unresolved ({unresolved / total:.1%})" if total else "0 JS-state samples")
    print(f"top {args.top} by self-time (leaf PC only, not inclusive):")
    for label, count in self_time.most_common(args.top):
        print(f"  {count:5d}  {count / total:5.1%}  {label}")


if __name__ == "__main__":
    main()
