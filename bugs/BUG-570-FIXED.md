# BUG-570: `VTTCue`/`TextTrackCue`/`TrackEvent` global constructors do not exist

**Статус:** FIXED 2026-09-14 (P3)
**Компонент:** js (`crates/js/src/dom.rs` — none of the three interfaces is
registered as a global; `crates/js/src/text_track_store.rs` implements the
underlying cue data model but only mentions the interface names in doc
comments, never installs constructors)
**Найден:** P2, WPT-VENDOR-html-semantics-embedded-content, 2026-08-04

## Симптом

`new VTTCue(...)`, `new TextTrackCue(...)` and `new TrackEvent(...)` all
throw `ReferenceError: <Name> is not defined` — the constructors are absent
from the global scope, not present-but-broken. Examples from
`html/semantics/embedded-content/media-elements/`:

```
FAIL Float precision of VTTCue attributes line, position and size, stored as floats - VTTCue is not defined
FAIL Invoke getCueAsHTML() on an empty cue - VTTCue is not defined
FAIL TextTrackCue and VTTCue are separate interfaces - TextTrackCue is not defined
FAIL TrackEvent constructor, one arg - TrackEvent is not defined
FAIL track element changing "track URL" and clearing cues, set mode, add cue, set src - VTTCue is not defined
```

A related but distinct assertion also fails for the opposite reason —
`TextTrackCue constructor should not be supported` (per spec,
`TextTrackCue` itself must NOT be directly constructible, only `VTTCue` is)
expects a `TypeError` on `new TextTrackCue(...)` and instead gets the
`ReferenceError` above, since neither name exists at all.

## Причина

WebVTT cue *data* is real and wired end-to-end — cue parsing, active-cue
resolution and rendering all work (`lumen_dom::vtt`,
`crates/js/src/text_track_store.rs`, per `CAPABILITIES.md`'s "✅ WebVTT" +
"TextTrack JS API" bullets covering `video.textTracks` /
`TextTrack.kind/label/language/mode/cues/activeCues` /
`TextTrackCue.startTime/endTime/text` / `cuechange`). What's missing is the
JS-visible *interface* layer: nothing in `dom.rs` calls the global-install
path (the same `ctx.globals().set(...)` pattern used for other DOM
interfaces) for `VTTCue`, `TextTrackCue`, or `TrackEvent`. Tests that read
existing cue objects via `video.textTracks[0].cues[0]` still work (the
objects exist, built server-side by the cue store); tests that construct a
cue/event directly from script, or that assert on `instanceof`/constructor
identity, fail outright.

## Масштаб

22 subtests across `media-elements/` (14 `VTTCue is not defined` + 6
`TextTrackCue is not defined` + 2 `TrackEvent is not defined`). Adjacent to
the already-documented `CAPABILITIES.md` gap "⬜ addTextTrack(), TextTrack.
mode-setter" — that bullet covers missing *methods* on an existing
`TextTrack` instance; this finding is the separate, unlisted gap of missing
*constructors* for the cue/event types themselves.

## Исправлено

`dom.rs` has since been split; the real fix sites are
`crates/js/src/video_bindings.rs` (`TextTrackCue`/`VTTCue`, next to the
`makeTextTrack`/`appendCues` machinery that has owned the actual cue data
since BUG-775) and `crates/js/src/shim/web_api_shim_mid.js` (`TrackEvent`,
next to the other `Event` subclasses like `HashChangeEvent`).

`TextTrackCue` is the spec's abstract base (`interface TextTrackCue :
EventTarget`) — WebIDL gives it no constructor operation, so it is installed
as a function that always throws `TypeError`, with `VTTCue.prototype`
chaining through its prototype. `VTTCue(startTime, endTime, text)` populates
the WebVTT §3.1 defaults (`id`, `pauseOnExit`, `region`, `vertical`,
`snapToLines`, `line`, `lineAlign`, `position`, `positionAlign`, `size`,
`align`) and adds `getCueAsHTML()`, which wraps the cue text in a single Text
node inside a document fragment (full WebVTT markup parsing — `<i>`/`<b>`/
timestamps — stays unimplemented, out of this bug's scope). `TrackEvent`
mirrors the other `Event` subclasses: `track` is exposed through a
getter-only property so a later assignment (the WPT constructor test does
exactly that) is silently ignored rather than mutating the event.

Both cue classes derive from a `_lumen_cue_base` that falls back to a no-op
function when `EventTarget` isn't defined — needed only so the crate's own
bare-runtime unit tests (which install `video_bindings` without the rest of
the page shim) don't fail at *install* time; in the real browser `EventTarget`
is always installed first (`v8_runtime.rs`'s `WEB_API_SHIM` eval runs before
`install_v8!(video_bindings::install_video_bindings_v8)`).

Wiring `track`/`addCue`/`removeCue` to a real `TextTrack`, and full WebVTT
cue-text markup parsing, remain the separate, already-documented
`CAPABILITIES.md` method-layer gap this bug explicitly excluded.

New tests in `crates/js/src/video_bindings.rs`'s `tests_v8::vtt_cue` module
(5 tests): constructor defaults, double-precision `line`/`position`/`size`
round-trip, `getCueAsHTML()` shape, `TextTrackCue`/`VTTCue` separateness plus
the illegal-constructor throw, and `TrackEvent`'s readonly `track`.

`cargo test -p lumen-js --features v8-backend` 3645/3645 (was 3640),
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D
warnings` clean.
