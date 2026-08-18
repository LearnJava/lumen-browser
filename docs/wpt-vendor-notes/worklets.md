# WPT vendor notes — `worklets`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-worklets`, `docs/wpt-status.md`), scope ⬜ (real, if Phase-0/partial:
`CSS.paintWorklet` and `audioContext.audioWorklet` exist, `CSS.animationWorklet`/
`CSS.layoutWorklet` do not — see finding below). Same pinned commit, `git
sparse-checkout add worklets` at the same commit hash, `LICENSE-WPT.md` copied
from the sibling `workers`, 58 files (`META.yml`, `README.md`,
`idlharness.https.any.js`, 20 root `.https.html` files — `{animation,audio,
layout,paint}-worklet-{credentials,csp,import,referrer,
service-worker-interception}.https.html` — plus `resources/` helpers:
`worklet-test-utils.js` and the five `*-tests.js` shared assertion modules).
Zero variant fan-out, zero testdriver, fully self-contained (`src=` only hits
`resources/testharness.js`/`testharnessreport.js`/its own `resources/*`) — every
predictor said cheap. All 21 selected ids are `.https.`.

`run_report.py --all --root worklets --recursive` (~12 min 19 s): **0/21
harness OK, 0/0 subtests** — every id TIMEOUT on the pre-existing WPT-RUN-2 TLS
gap (`network error: TLS handshake: invalid peer certificate: UnknownIssuer`,
same class as `webtransport`/`webusb`/`webxr`/`window-management`), a uniform
wall with zero category-specific signal.

Because the run itself gives nothing (same lesson as `eyedropper`/`fenced-frame`:
a category masked end-to-end by the TLS gap is not automatically finding-free),
probed the shared `get_worklet(type)` helper's object graph directly — grep of
`crates/js/src/*.rs` plus a live `--mcp-live-port` probe (plain `http://`, since
the WPT `.https.` pages never reach JS at all):

```json
{"css_defined":"object","paintWorklet":"object","paintWorklet_addModule":"function",
 "animationWorklet":"undefined","layoutWorklet":"undefined",
 "audioContext_worklet":"object","window_Worklet":"undefined","registerPaint":"function"}
```

`CSS.animationWorklet`/`CSS.layoutWorklet`/the generic `window.Worklet` base
interface are entirely absent — `get_worklet('animation')`/`get_worklet('layout')`
return `undefined`, so any real test/page calling `.addModule()` on them throws
`TypeError` synchronously, never reaching network activity at all. `CSS.paintWorklet`
(`crates/js/src/paint_worklet.rs:91-99`) and `audioContext.audioWorklet`
(`crates/js/src/web_audio.rs:485-487`) do exist, but both `addModule()`
implementations are pure no-ops — always resolve without ever fetching or
executing the given module URL, so `registerPaint()` (real and independently
tested — `paint_worklet.rs`'s own unit tests — but never actually reached via
`addModule`) never runs for a real page. Filed [BUG-779](../../bugs/BUG-779-OPEN.md).

No `CAPABILITIES.md` entry exists for Houdini/worklets at all (checked before
filing) — this is a straightforward "not yet built" gap, not a claims-vs-reality
regression like `innerHTML`/BUG-368.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-18 (`tests/wpt/worklets/`, 58 файлов, 21 id, все
`.https.`, без variant/testdriver). `run_report.py --all --root worklets
--recursive` (~12:19): **0/21 harness OK, 0/0 сабтестов** — все 21 TIMEOUT на
уже задокументированном TLS-гэпе `UnknownIssuer` (тот же класс, что
`webtransport`/`webusb`/`webxr`/`window-management`), сигнала по категории
прогон не дал. Проба API напрямую (греп + живой `--mcp-live-port`) нашла
[BUG-779](../../bugs/BUG-779-OPEN.md): `CSS.animationWorklet`/`CSS.layoutWorklet`/
базовый `window.Worklet` отсутствуют целиком; `CSS.paintWorklet`/
`audioContext.audioWorklet` существуют, но их `addModule()` — чистый no-op,
никогда не фетчит и не исполняет модуль (`registerPaint()` реален и работает
при прямом вызове, но недостижим через штатный `addModule()`-путь). Скоуп ⬜
(частичная Phase-0 реализация есть, не «архитектурно вне» как медиа/аппаратные
категории). Новый баг заведён один — BUG-779, покрывает весь класс дефекта.
