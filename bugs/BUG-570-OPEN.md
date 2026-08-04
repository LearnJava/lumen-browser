# BUG-570: `VTTCue`/`TextTrackCue`/`TrackEvent` global constructors do not exist

**Статус:** OPEN
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
