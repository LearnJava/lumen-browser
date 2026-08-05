# BUG-638: `<audio>.src = <relative URL>` permanently deadlocks the JS engine/automation channel

**Статус:** OPEN
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
