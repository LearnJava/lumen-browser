# BUG-636 — MediaSession API skips WebIDL validation/freezing across the board; `chapterInfo` unimplemented

**Статус:** OPEN
**Компонент:** js (`crates/js/src/media_session.rs`)
**Найден:** 2026-08-05, P2, WPT-VENDOR-mediasession

## Симптом

`tests/wpt/mediasession/mediametadata.html`, `positionstate.html`,
`setactionhandler.html` — все три harness OK, но большинство подтестов FAIL.
The pattern repeats across every method of the shim: the JS shim (`crates/js/
src/media_session.rs`) stores whatever the caller passes without validating
against the WebIDL dictionary shape or freezing per spec.

```
FAIL Test that MediaMetadata is constructed using a dictionary
  - assert_throws_js: new MediaMetadata('foobar') did not throw TypeError

FAIL Test the different values allowed in MediaMetadata init dictionary
  - metadata.chapterInfo is undefined (chapterInfo entirely unimplemented)

FAIL Test that MediaMetadata.artwork can't be modified
  - metadata.artwork.push(...) doesn't throw; metadata.artwork[0].src = 'bar' mutates in place

FAIL Test that MediaMetadata.artwork will not expose unknown properties
  - assert_false('some_other_value' in metadata.artwork[0]) — got true (raw object stored, not re-shaped to MediaImage dict)

FAIL Test that MediaMetadata.artwork is Frozen
  - Object.isFrozen(metadata.artwork) === false

FAIL Test that MediaMetadata.artwork returns parsed urls
  - metadata.artwork[1].src === '../foo' (raw string), expected new URL('../foo', document.URL).href

FAIL Test MediaImage default values / Test that MediaImage.src is required
  - no validation that `src` member exists; missing src reads back as undefined instead of throwing/defaulting

FAIL Test setPositionState throws a TypeError if duration is negative
  (same for: position negative, duration < position, playbackRate === 0, duration unspecified)
  - setPositionState() accepts any numeric value silently, never throws

FAIL Test that setActionHandler() throws exception for unsupported actions
  - setActionHandler("invalid", null) is a silent no-op instead of throwing TypeError
    (WebIDL enum-argument binding failure)
```

`tests/wpt/mediasession/mediametadata.html` additionally TIMEOUTs on
"Test that the base URL of MediaImage is the base URL of entry setting
object" — depends on the same missing URL-resolution behaviour, load never
settles because the assertion inside a same-origin iframe never observes the
expected resolved URL.

## Причина (найдено чтением кода)

`crates/js/src/media_session.rs`, `MEDIA_SESSION_SHIM`:

```js
function MediaMetadata(init) {
  this.title   = (init && init.title)   || '';
  this.artist  = (init && init.artist)  || '';
  this.album   = (init && init.album)   || '';
  this.artwork = (init && Array.isArray(init.artwork)) ? init.artwork.slice() : [];
}
```

- No check that `init` (when passed) is coercible to a dictionary — a
  primitive like `'foobar'`/`42` is silently treated as "no init" instead of
  throwing `TypeError` per WebIDL dictionary conversion.
- `chapterInfo` (Media Session spec §6.1, `MediaMetadataInit.chapterInfo`) is
  not read from `init` and not exposed on the instance at all — `metadata.
  chapterInfo` is `undefined`, not even an empty frozen array.
- `artwork` is stored via `.slice()` (shallow array copy) — the *elements*
  are the caller's own objects, not re-built `MediaImage` dictionaries, so:
  unknown properties leak through, `src` is never resolved against the
  document's base URL (`new URL(src, document.baseURI).href` per spec), and
  neither the array nor its elements are frozen (`Object.freeze`).
- `setPositionState` (line ~109) type-checks each member individually with a
  silent NaN/1/0 fallback instead of validating and throwing:

```js
setPositionState: function(state) {
  ...
  _positionState = {
    duration:     typeof state.duration     === 'number' ? state.duration     : NaN,
    playbackRate: typeof state.playbackRate === 'number' ? state.playbackRate : 1,
    position:     typeof state.position     === 'number' ? state.position     : 0
  };
  ...
}
```
  Spec (Media Session §5.4, `setPositionState()` steps 3-6) requires a
  `TypeError` when `duration < 0`, `position < 0`, `position > duration`, or
  `playbackRate === 0`.
- `setActionHandler` silently returns on an action name outside
  `VALID_ACTIONS` instead of throwing — WebIDL enum-argument binding failure
  means an invalid enum value should reject the call with `TypeError`, not
  no-op:

```js
setActionHandler: function(action, callback) {
  if (!VALID_ACTIONS[action]) return;   // spec: should throw TypeError
  ...
}
```

None of this is the doc-commented "Phase 0: not forwarded to OS media-control
surface" limitation at the top of the file — that note is about shell/OS
integration, not about JS-visible WebIDL conformance. `CAPABILITIES.md` lists
MediaSession as unconditionally ✅ — same drift class as BUG-368
(`innerHTML`): the API is reachable and mostly functional for the common
case (title/artist/album/playbackState/action callbacks all work, verified
by `crates/js/src/media_session.rs`'s own unit tests), but every input
validation and read-only-freezing edge is unenforced, plus one whole spec
member (`chapterInfo`) is missing.

## Как повторить

```bash
MSYS2_ARG_CONV_EXCL='*' tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_report.py --binary 'D:/RustProjects/lumen-browser/target/dev-release/lumen.exe' \
  --all --root mediasession --recursive --out .tmp/wpt-mediasession.html
# mediametadata.html: Subtests passed 5/20 (+ 1 TIMEOUT)
# positionstate.html, setactionhandler.html: 1 FAIL each on validation
```

Live check (no WPT needed):

```js
new MediaMetadata('foobar');                              // should throw TypeError, doesn't
new MediaMetadata({}).chapterInfo;                         // should be [], is undefined
Object.isFrozen(new MediaMetadata({artwork:[{src:'x'}]}).artwork); // should be true, is false
navigator.mediaSession.setPositionState({duration:-1});    // should throw TypeError, doesn't
navigator.mediaSession.setActionHandler('bogus', null);    // should throw TypeError, doesn't
```

## Не путать с

`idlharness.window.html` TIMEOUT in the same run — that's the pre-existing,
already-documented vendoring gap (`/resources/idlharness.js` +
`/resources/WebIDLParser.js` not vendored, same class as every other
category using `idlharness.js`), not an engine defect.
