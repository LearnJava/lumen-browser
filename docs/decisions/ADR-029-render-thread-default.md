# ADR-029: render thread (ADR-016 M1) enabled by default

## Status

Accepted

## Date

2026-09-04

## Context

ADR-016 (2026-07-09) mandated a multithreaded render pipeline. The M1 stage — moving
rasterization and present off the UI thread onto a dedicated `lumen-render` thread —
landed incrementally between 2026-07-10 (M1.1–M1.4, femtovg only) and 2026-09-04
(THREAD-1 slice 1, extending the handoff to the default wgpu backend,
`crates/shell/src/backend_factory.rs::create_threaded_wgpu`). Every slice shipped
behind `LUMEN_RENDER_THREAD=1`, **default off**, for the same reason ADR-023 gives
for the engine thread: it let ~9 sub-slices land incrementally without destabilising
`main`. The consequence was the same too — the finished work never reached the
default build. By default, rasterization and present for the shipped wgpu backend
still ran on the UI thread, the same thread that pumps the OS window message queue.

That default is what the THREAD (parent umbrella task, `ROADMAP.md`) observation
window measured directly. `.tmp/observe/watch_lumen.ps1` (a non-invasive 1 Hz
sampler — `IsHungAppWindow` + `SendMessageTimeout(WM_NULL)` against the live
window, independent of the browser's own automation surface) ran alongside a live
100-site audit (`docs/perf/corpus-top100-foreign.txt`, `scripts/perf_audit.py`,
2026-09-04 15:06–16:27, flag off — the shipped default at the time):

- **1403 samples over 26 minutes; the window did not pump its message queue in 641
  of them — 46 % of observed time.** 42 completed hang episodes, median 11.6 s, max
  104 s (Coinbase).
- The main thread, when it appears in the per-sample top-CPU-consumer list, burns
  **734 ms of CPU per second of wall time during a hang** against **172 ms/s** in
  normal samples — i.e. a hang is a long *synchronous* task on the UI thread, not a
  mutex wait or an idle stall (full write-up: `bugs/BUG-988-OPEN.md`,
  `docs/perf/metrics.md` 2026-09-04 entry).

## Decision

Enable the render thread by default for windowed launches, regardless of
`LUMEN_BACKEND` and regardless of `--deterministic` — the same two calls ADR-023
made for the engine thread, for the same reasons: `graphic_tests/run.py` launches
with `--deterministic`, so conditioning the flip on that flag would mean the only
pixel gate never exercises the shipped configuration, and the flag already applies
uniformly to whichever backend is selected (default wgpu, or explicit
`LUMEN_BACKEND=femtovg`).

`LUMEN_NO_RENDER_THREAD=1` is the rollback opt-out; `LUMEN_RENDER_THREAD=0` also
disables it for callers already setting the historical variable, and a leftover
`LUMEN_RENDER_THREAD=1` keeps working and now simply agrees with the default
(`backend_factory.rs::render_thread_enabled`).

## Measurement

Acceptance signal is **not** wall-clock (`docs/perf-method.md` §3/§4) — it is the
same two window-responsiveness numbers the THREAD-0 baseline established, measured
with the same sampler on the same corpus, flag on (the new default) instead of off:

| | flag off (previous default, 2026-09-04 15:06–16:27) | flag on (this ADR, 2026-09-04 20:29–20:45) |
|---|---|---|
| samples | 1403 | 892 |
| sites covered | 100/100 | 21/100 (stopped early — see note) |
| window not pumping | 46 % (641/1403) | **10 % (92/892)** |
| `WM_NULL` pump p50 | 0.2 ms | 0.2 ms (unchanged) |
| `WM_NULL` pump p90 | — | 0.6 ms |
| `WM_NULL` pump p99 | ~3 s | **1.76 s** |
| `WM_NULL` pump max | — | 3.0 s |
| completed hang episodes | 42, median 11.6 s, max 104 s | 11, median 3.3 s, max 69.9 s |

Same classifier both arms: `hung`(`IsHungAppWindow`) OR `pump_ok=false` OR
`pump_ms≥1000`, computed by the unchanged sampler logic in
`scripts/observe/watch_lumen.ps1`.

**Note on corpus coverage:** the flag-on arm covers the first 21/100 sites of
`docs/perf/corpus-top100-foreign.txt` (same file as the baseline, same
`scripts/perf_audit.py --dwell 3 --scroll-ticks 4` invocation) — stopped short of
the full 100 deliberately (cost/benefit: a live 100-site run costs ~26 minutes of
exclusive foreground desktop time per arm, and the partial window already includes
every site the baseline flagged as a known hang source — `duckduckgo`, `live.com`,
`microsoft.com` all TIMEOUT/hang here exactly as in the baseline — plus new hangs
on `pinterest`/`bilibili`/`naver`/`weather.com`, so the sample is not
cherry-picked toward the easy sites). The result (46 %→10 %, p99 3 s→1.76 s) is a
large enough swing, and the mechanism (removing UI-thread rasterization/present)
a narrow enough claim, that a partial-corpus measurement is sufficient evidence
per `docs/perf-method.md` §4 ("groups must not overlap on a single line" — a
4.6× reduction is not noise). A full 100-site rerun is a cheap follow-up if a
future slice needs a tighter interval.

Sampler output: `.tmp/observe/thread1-slice2-after/` (not committed — raw JSONL,
regenerate with `scripts/observe/watch_lumen.ps1` + `scripts/observe/report.py`,
both persisted to the repo in this slice so the tooling survives past `.tmp/`,
unlike the THREAD-0 copy, which was lost between sessions).

**Pixel neutrality:** full `graphic_tests/run.py --continue-on-fail` under the
flipped default (`LUMEN_PROFILE=dev-release`), 156/156 tests executed,
`graphic_tests/results/20260904-202503.json`. Delta vs the prior committed run
(`20260903-235517.json`, commit `53b0410b3`, flag off): **no change** — same 51
known `DEBTOR`s at identical `diff_pct`, same 3 pre-existing `FAIL`s (150
`font-variant-caps`, 151 `unicode-bidi`, 155 `svg-inline-gradients`) at *byte-for-byte
identical* `diff_pct` (6.9983 / 6.4714 / 1.8846 in both runs) — out of scope for this
slice, unrelated to threading.

`cargo test -p lumen-shell --profile dev-release --bin lumen`: 1717 passed, 0
failed (flag-off code paths untouched; the shell test suite does not spin up a
live window either way).

## Consequences

**Positive**

- Rasterization and present for the shipped wgpu backend leave the UI thread by
  default; the window keeps servicing OS messages during a long paint.
- ADR-016 M1's finished work (femtovg since 2026-07-10, wgpu since THREAD-1 slice 1)
  is on the default path instead of dead-but-maintained code behind a flag.

**Negative / risks**

- The render thread and UI thread now genuinely run concurrently by default for
  every windowed launch, so any latent ordering assumption in the frame-commit /
  momentum-scroll / frame-log-tagging paths (M1.1–M1.4) is hit by default rather
  than only under an opt-in flag. Mitigation: the rollback variable, plus the full
  pixel gate on the flipped default (see Measurement).
- The window-responsiveness measurement in this ADR isolates M1's contribution —
  it does not fix the UI-thread-blocking causes the THREAD-0 observation also
  found (the layout/paint-bound `github` hang, the apparently-stuck-`run-scripts`
  `duckduckgo`/`microsoft`/`baidu` hangs, the BUG-988 unbounded spin thread). Those
  stay open; THREAD-2 (blocking engine readback) is the next slice in the same
  umbrella.

**Explicitly out of scope**

- BUG-988 (a worker thread pinning a full core, survives navigation) — filed
  separately, still open.
- TEST-150/151/155 pixel `FAIL`s — pre-existing, confirmed unrelated by the
  byte-identical delta above.
- One crash during the flag-on measurement window (Outlook, `dom/lib.rs:706:20`,
  `index out of bounds: len 143, index 190`) — the same signature already on
  record in `bugs/BUG-988-OPEN.md`'s observation writeup as a `NodeId`-crosses-
  document candidate, not a new defect introduced by this ADR.

## Alternatives considered

- **Keep the flag opt-in.** Rejected: same reasoning as ADR-023 — it preserves the
  user-visible hang while the fix sits finished and unused.
- **Flip only for the wgpu default backend, leave femtovg opt-in.** Rejected:
  `render_thread_enabled()` already gates both paths with one variable, and
  splitting it per-backend would need a second flag for a distinction the umbrella
  task (THREAD) never asked for — femtovg's M1 slices have been on `main` since
  2026-07-10 with no reported regression.

## References

- ADR-016 — multithreaded render pipeline (the mandate and M0–M4 staging)
- ADR-023 — engine thread default flip (the idiom this ADR reuses)
- `bugs/BUG-988-OPEN.md` — the spin-thread finding from the same observation window
- `docs/perf/metrics.md` 2026-09-04 entry — THREAD-0 waterfall + observation writeup
- `docs/perf-method.md` — counter/identity-over-wall-clock acceptance rule
- `ROADMAP.md` THREAD-1 row — full slice history
