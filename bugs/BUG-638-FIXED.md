# BUG-638: `<audio>.src = <relative URL>` permanently deadlocks the JS engine/automation channel

**Статус:** FIXED 2026-09-23 (P6, повторная проба подтвердила BUG-799)
**Компонент:** js (`crates/js/src/audio_element.rs` — `AUDIO_ELEMENT_SHIM` src setter → `__lumen_audio_load`), shell (`crates/shell/src/platform/audio_player.rs::AudioPlaybackProviderImpl::load` / `fetch_audio_bytes`)
**Найден:** P2, WPT-VENDOR-mimesniff, 2026-08-05, проба `--mcp-live-port`

## Симптом

`tests/wpt/mimesniff/media/media-sniff.window.js` creates 42 `<audio>`
elements (7 media vectors × 6 `Content-Type` variants) with a
document-relative `src` (`"resources/" + vector + "?pipe=..."`) and waits for
either `loadedmetadata` or `error`. None of the 42 `async_test`s ever
resolves — `run_report.py --root mimesniff` reports the whole file as a
harness-level `TIMEOUT` (`TestRunner hit external timeout`) after the full
24 s wptrunner budget, with **zero** console output (no `fetch error`, no
`error`/`loadedmetadata` event trace) — unlike every other relative-URL bug
in this family (BUG-346/347/359/362/370), which reject/error promptly and
log `invalid url: missing scheme`.

Confirmed live via `--mcp-live-port` + `eval` (retry-free, since the hang
happens on the very first affected call):

```js
window.__el = document.createElement('audio');   // eval → "object", instant
window.__el.src = 'resources/mp3-raw.mp3';        // eval → NEVER RETURNS
```

The second `eval` call itself times out at the MCP/BiDi layer
(`-32603 Eval error: automation command timed out`), not just the page-level
promise/event. **Every subsequent `eval` on the same process — including the
trivial `window.__ready` that worked seconds earlier — times out identically
from that point on**, reproduced independently across two fresh process
launches (ports 18899 and 18900). The `lumen.exe` process itself keeps
running (visible in `tasklist`, not crashed) — this is a true deadlock of
the JS thread / automation command channel, not a slow response or an OS-level
crash.

## Reproduction

1. `lumen.exe --mcp-live-port <N> <any page>`
2. Over the MCP channel: `eval("document.createElement('audio')")` — returns fine.
3. `eval("el.src = 'resources/x.mp3'")` (any relative, unresolvable URL) — hangs forever.
4. Any further `eval` call on the same connection/process also hangs forever.

## Hypothesis (not confirmed — no debugger attached)

`Object.defineProperty(el, 'src', {set: ...})` → `startLoad(url)` →
`__lumen_audio_load(_handle, url)` (native, `crates/js/src/audio_element.rs:96-102`)
→ `AudioPlaybackProviderImpl::load` (`crates/shell/src/platform/audio_player.rs:299-335`),
which spawns a background thread running `fetch_audio_bytes(&url)`
(`audio_player.rs:405-416`). That function does
`lumen_core::url::Url::parse(url)` **without a base** — for a relative string
like `"resources/mp3-raw.mp3"` this is the same "missing scheme" failure mode
as BUG-347, and normally should return `Err` quickly, flip `has_error`, and
let the JS-side `setInterval` poll pick it up and fire `'error'` within one
`POLL_MS` (50 ms) tick. That does not happen — the *native call itself*
never returns to JS, which points at something blocking before `load()`
returns (not at the async fetch/decode path, which runs on its own thread and
can't block the caller). `alloc_handle` (`audio_player.rs:278`) also spawns a
per-handle OS thread that calls `rodio::OutputStream::try_default()`
(`audio_player.rs:138`) synchronously on that thread — worth checking first
whether audio-device acquisition itself is hanging and somehow taking a lock
that `load()`/the V8 native-call trampoline waits on, since alloc alone (via
plain `document.createElement('audio')`) did **not** reproduce the hang in
this session's probe — only the combination of alloc-already-done +
`.src =` (i.e. `load()`) did.

## Масштаб

Confirmed only via the one repro above (relative `<audio src>`); not yet
checked whether `<video>` shares the same `load()` path (`audio_element.rs`
doc comment implies `<video>` reuses `HTMLMediaElement` plumbing) or whether
an *absolute* unresolvable URL (e.g. a dead host) reproduces the same hang —
if so this is a general media-fetch deadlock, not specific to relative-URL
resolution, and the BUG-346/347-family framing above may be a red herring.
Both should be checked before attempting a fix.

## Триаж 2026-09-23: вероятно устарел — закрыть после одной пробы

Самоблокировка на `__lumen_audio_load` снята BUG-799 (`8a3db3b4bc`, 2026-08-25), относительный
`src` резолвится от базы документа (GAP-CSPENF срез 51, `crates/js/src/audio_element.rs`).
Задача P6: одна живая проба `<audio>.src = "rel.mp3"`; если движок не виснет — перенести в
`BUGS-FIXED.md` как исправленный BUG-799, иначе описать, что осталось.

## Закрыт (P6, 2026-09-23)

Живое окно/MCP в этой сессии недоступно (см. соседние срезы BUG-683/BUG-791
того же дня — фоновый запуск без видимого окна не мирует JS-хэндл,
`feedback_background_launched_window_breaks_mcp_js_context`), поэтому проба
сделана headless-эквивалентом того же пути: `document.createElement('audio')`
→ `appendChild` → `el.src = 'resources/mp3-raw.mp3'` (тот же относительный
URL, что в оригинальном репро) в `<script>` статической страницы,
`lumen.exe --dump-layout <page>`. `--dump-layout` выполняет инлайновые
скрипты страницы синхронно (подтверждено: `<audio>` появляется в дереве
только если скрипт отработал), так что зависание в самом нативном вызове
`__lumen_audio_load` (не в последующей навигации/дренаже событий, которые
headless действительно пропускает) должно было проявиться тем же образом —
зависшим процессом, не завершившимся дампом.

Результат: `--dump-layout` завершается за 0.24 с (было — вечное зависание
процесса), в выводе `Audio … src="resources/mp3-raw.mp3"` — атрибут
присвоен, дальнейший вывод дампа получен целиком, деградации нет. Код
подтверждает причину: `startLoad()` (`audio_element.rs:269-321`) теперь
резолвит `url` через `_url_resolve`/`_lumen_document_base_url` **до**
`__lumen_audio_load`, а не передаёт сырую относительную строку — ровно тот
фикс, что описан в [BUG-799](BUG-799-FIXED.md).

Не перепроверено в этом срезе (не требовалось триажем, но осталось открытым
по «Масштабу» исходной заявки): `<video>` с тем же относительным `src` и
абсолютный недостижимый URL — если кто-то заново увидит зависание на одном
из этих путей, это отдельный баг, не рецидив этого.
