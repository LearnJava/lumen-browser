# ADR-023: engine thread (ADR-016 M2) enabled by default

## Status

Accepted

## Date

2026-07-28

## Context

ADR-016 (2026-07-09) mandated a multithreaded render pipeline and laid out stages
M0–M4.1. All of them landed on `main` between 2026-07-09 and 2026-07-13, and the
ADR-016 umbrella task was closed on 2026-07-15 after a code cross-check (ROADMAP
row `P3-mt`). The engine thread itself — the M2 stage that moves layout, cascade
and the JS handle off the UI thread — shipped behind `LUMEN_ENGINE_THREAD=1`,
**default off**.

The flag stayed off for a deliberate reason: every M2 slice was accepted on the
invariant *"flag off is byte-identical to the previous synchronous behaviour"*,
which let ~21 sub-slices land incrementally without destabilising `main`. The
consequence is that the finished, accepted work was never actually reaching users:
by default every `relayout()` still ran on the UI thread — the thread that also
pumps the OS window message queue.

That default is what makes real sites appear to hang on load. Measured on
`https://lenta.ru` (9 `@font-face` files) during the BUG-274 investigation
(`bugs/BUG-274-OPEN.md`, срез 2026-07-28): each arriving web font fires
`LoadEvent::FontLoaded` → `relayout_chrome()`, and with the flag off those
relayouts serialize on the UI thread — **9 synchronous full relayouts of a
~1800-node display list, ~300–700 ms each, before the first frame**. Windows marks
a window "Not Responding" when it stops servicing its message queue, which is
exactly what a multi-second synchronous relayout does.

Measurement (dev-release, default wgpu backend, `LUMEN_FRAME_LOG=1`, same binary,
`LUMEN_NO_ENGINE_THREAD=1` for the off arm):

| | synchronous relayouts before first frame |
|---|---|
| flag off (previous default) | 9, 9 |
| flag on (this ADR) | 1, 3 |

Wall-clock first-frame numbers moved the same direction (~4.6/1.9 s off vs
~3.3/1.6 s on) but are **not** the acceptance signal here: they swing by seconds
between runs on a live network. The relayout **counter** is the stable identity
signal, per `docs/perf-method.md` ("gate by counter/identity, not wall-clock").

## Decision

Spawn the engine thread by default. `LUMEN_NO_ENGINE_THREAD=1` is the rollback
opt-out; `LUMEN_ENGINE_THREAD=0` also disables it for callers already setting the
historical variable, and a leftover `LUMEN_ENGINE_THREAD=1` keeps working and now
simply agrees with the default.

This is the same flag-strategy idiom as ADR-018 (V8 cutover, `--features quickjs`
rollback) and ADR-021/CC-14 (engine chrome, `LUMEN_LEGACY_CHROME=1` rollback):
flip the default once the feature is complete and gated, keep a one-variable
escape hatch, delete the escape hatch later.

The decision is deliberately **not** conditioned on `--deterministic`.
`graphic_tests/run.py` launches the browser with `--deterministic --viewport
1024x720`, so forcing the thread off in deterministic mode would mean the pixel
gate never exercises the configuration that ships — false confidence by
construction.

## Consequences

**Positive**

- The startup relayout storm on font-heavy real sites leaves the UI thread; the
  window keeps servicing OS messages during load.
- ADR-016 M2's finished work is actually on the default path instead of being
  dead-but-maintained code behind a flag.

**Negative / risks**

- The UI thread and engine thread now genuinely run concurrently by default, so
  any latent ordering assumption in the routed paths (`route_task_js`,
  `route_query_js`, `poll_engine_commit`) is now hit by default rather than only
  under an opt-in flag. Mitigation: the rollback variable, plus the full
  `graphic_tests` pixel gate run on the flipped default.
- Timing-sensitive automation (BiDi/MCP `wait{document_ready}`) now observes async
  relayout commits by default. No change in the wait contract itself.

**Explicitly out of scope**

- This does **not** fix scrolling. Scroll stalls on real sites were measured in
  the same session and are dominated by frames that repaint a newly exposed band
  (`band blit+expose`, 1–3 s against ~7 ms for an ordinary scroll frame), which
  perform no layout at all — an interleaved A/B showed the flag makes no
  difference there. Filed separately as BUG-405.
- This does not touch `LUMEN_RENDER_THREAD` (ADR-016 M1, the *render* thread),
  which remains opt-in. The brief's risk note "wgpu backend (BUG-274) stays off
  the threaded default until fixed" refers to that flag, not this one.

## Alternatives considered

- **Keep the flag opt-in.** Rejected: it preserves the user-visible hang while
  the fix sits finished and unused, and an unexercised default path rots.
- **Enable only for non-deterministic launches.** Rejected: it would exclude the
  graphic-test suite — the only pixel gate — from ever testing the shipped
  configuration.
- **Fix the `@font-face` relayout storm directly** (batch/debounce the per-font
  relayouts). Not rejected on merit — it is complementary and still worth doing,
  since with the thread on the work is merely moved off the UI thread rather than
  eliminated. Kept out of this ADR to keep the flip reviewable on its own.

## References

- ADR-016 — multithreaded render pipeline (the mandate and M0–M4 staging)
- `docs/tasks/ph3-render-multithreading.md` — per-slice history, M2 acceptance
- `bugs/BUG-274-OPEN.md` — cold-start investigation that produced the measurement
- `bugs/BUG-405-OPEN.md` — scroll expose-band stalls, explicitly not fixed here
- `docs/perf-method.md` — counter-over-wall-clock acceptance rule
