# BUG-345: navigation to a page mixing `document.designMode` + declarative Shadow DOM hangs/times out under live BiDi

**Статус:** FIXED (side effect, not reproducible) 2026-08-06
**Компонент:** shell/js (unclear which layer yet — see investigation notes)
**Найден:** P2, WPT-VENDOR-contenteditable 2026-07-25 (`run_report.py --all --root contenteditable --recursive`, vendored `contenteditable/designmode-iscontenteditable.html`)

## Симптом (исходный)

`run_report.py --all --root contenteditable --recursive` (live `lumen --bidi-port` +
wptrunner's `webdriver-bidi` executor) on `contenteditable/designmode-iscontenteditable.html`:

```
TEST_START: /contenteditable/designmode-iscontenteditable.html
INFO Got timeout in harness
TEST_END: TIMEOUT, expected OK - TestRunner hit external timeout (this may indicate a hang)
WARNING Exception in TestExecutor.run:
...
webdriver.bidi.error.UnknownErrorException: unknown error (navigate: automation command timed out)
```

`browsingContext.navigate` itself timed out — the harness never got to run any of
the page's `test()` calls.

## Проверка 2026-08-06 (P3, задача BUGS.md:359)

Симптом **не воспроизводится**. Two independent `run_report.py --all --root
contenteditable --recursive` runs on current `main` (commit `699b2762f`,
dev-release, freshly built in the `p3-work` slot — the root worktree's
`target/dev-release/lumen.exe` was stale, see gotcha below):

```
tests: 2/2 harness OK; subtests: 2/6 passed
```

No `TIMEOUT`/hang anywhere in either run's log. `designmode-iscontenteditable.html`
completes its harness cleanly; 4 of its 6 subtests still FAIL, but on an ordinary
`ReferenceError: ceInherit is not defined` — the already-tracked
[BUG-384](BUG-384-FIXED.md) (named access on `window` for id'd elements not
implemented) — not a hang. `plaintext-only.html` (the sibling file in the same
category) now passes 2/2, confirming [BUG-344](BUG-344-FIXED.md)'s fix is live.

Also isolated the two suspected ingredients (declarative Shadow DOM +
`document.designMode`) in a minimal non-WPT repro via `--mcp-live-port` +
`getElementById` (avoiding the BUG-384 mask): `navigate` (1.5s) → `wait
document_ready` (0.4s) → `eval` (instant) all completed with no hang.
`isContentEditable` reads `false` throughout (`designMode` remains an ordinary,
unimplemented property, as originally noted — it never flips `isContentEditable`
one way or the other). `ceInherit.shadowRoot.getElementById("inShadow")` returned
`null` (declarative shadow root content not queryable this way) — a real but
separate gap, not investigated further here since it doesn't cause a hang either.

## Причина исчезновения

Not pinned to a single commit. Candidates checked and ruled out:

- The default JS engine flip to V8 (`cc3d7db10`, 2026-07-14) predates this bug's
  filing date (2026-07-25) — V8 was already the live engine when the hang was
  observed, so QuickJS removal (S12b-F1..F4, closed 2026-08-04) cannot be the
  cause.
- The `browsingContext.navigate`/`DocumentReady`-staleness class of bug
  ([BUG-300](BUG-300-FIXED.md)) was fixed 2026-07-17, also before this bug's
  filing date — a different mechanism must have been responsible, since BUG-300's
  fix was already live when the hang was captured.
- `S12b-24` ("dom.rs → V8", 30 slices, closed 2026-08-04) ported the rquickjs-era
  **unit-test monolith** in `dom.rs` to run against `V8JsRuntime` instead of
  `rquickjs::Runtime` — it did not change the live DOM-shim behavior evaluated by
  `install_dom`, so it's an unlikely mechanism too.

Between 2026-07-25 and 2026-08-06 dozens of unrelated engine fixes landed (focus
API, IDL reflection, event dispatch, DEVX-15 auth, BiDi network interception,
etc. — see `git log --oneline --since=2026-07-25`); the hang most likely depended
on some now-fixed interaction one of these touched, but no single commit stood
out as the fix on inspection. Given the symptom is confirmed gone on two
independent fresh-binary runs, closing as fixed-by-side-effect rather than
spending further bisection budget on a P3 bug-fix task.

## Остаточная находка

None new. The 4 residual subtest FAILs in `designmode-iscontenteditable.html`
are [BUG-384](BUG-384-FIXED.md) (already open); no hang, no new defect.

## Гочи, встреченные в ходе проверки

**Stale root-worktree binary produced a false-positive regression report.**
`run_report.py --binary` pointed first at the root worktree's
`target/dev-release/lumen.exe` (built 01:53, before the BUG-344 merge commit at
08:19) instead of the `p3-work` slot's freshly-built binary (08:24) — this
reproduced the exact `docs/wpt-status.md`-adjacent gotcha already logged in
`CLAUDE.md` ("a stale `target/dev-release/lumen.exe` will mimic an unrelated
empty-subresource-fetch bug"), this time masking a just-fixed bug
(`plaintext-only.html` showed FAIL on the stale binary, PASS on the fresh one).
Always point `--binary` at the worktree whose commit you're actually verifying
when the change under test touched Rust code (not just for WPT-VENDOR's
vendoring-only case, where the recipe of reusing the root binary is safe).

## DoD

- [x] `run_report.py --all --root contenteditable --recursive` completes with no
      `TIMEOUT`/hang, twice in a row, on a binary built from the commit under test.
- [x] Root-cause investigation performed; no single fixing commit identified, but
      candidate mechanisms from the original filing window were checked and ruled
      out (documented above) rather than assumed.
- [x] Residual subtest failures triaged to an already-open bug (BUG-384), not
      left unexplained.
