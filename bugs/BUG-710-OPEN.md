# BUG-710: WebCodecs buffer types drop their payload — `VideoFrame`/`AudioData` never store the source data, `copyTo()`/`allocationSize()` are no-ops or missing entirely, `VideoColorSpace` doesn't exist

**Статус:** OPEN
**Компонент:** js (`crates/js/src/web_codecs.rs` — `webcodecs_shim`)
**Найден:** WPT-VENDOR-webcodecs (`ROADMAP.md`)

## Симптом

`WPT-VENDOR-webcodecs` run (`run_report.py --all --root webcodecs
--recursive --processes=4`, 13:11, 126 selected ids): **18/126 harness OK,
27/214 subtests passed**. Of the 108 non-OK harness results, 103 are
TIMEOUT on the standard TLS `UnknownIssuer` gap (55 `.https.` files) — not
a new finding. Of the 18 tests that did run, the dominant subtest-failure
cluster (79+ of the 214 unexpected results) is not the acknowledged "Phase
0, no codec backend" gap (`web_codecs.rs`'s own doc comment, `CAPABILITIES.md`
"⬜ WebHID/USB/Bluetooth/Serial/MIDI/WebXR/WebCodecs (NotSupported stubs)") —
it's a narrower defect that needs no codec at all: the buffer-holding
classes never retain or expose their payload.

`crates/js/src/web_codecs.rs`'s `webcodecs_shim`:

* `VideoFrame`'s constructor (`web_codecs.rs:227-234`) takes a `data`
  source (canvas/`ImageBitmap`/`OffscreenCanvas`/another `VideoFrame`) as
  its first argument and **never reads it** — only `init.format`/
  `init.codedWidth`/`init.codedHeight`/`init.timestamp`/`init.duration`
  from the second argument are used, all defaulting to `0`/`'I420'` when
  omitted (which they usually are for a canvas/frame source, since the
  spec derives those from the source itself). The class has **no**
  `allocationSize()`, `copyTo()`, `displayWidth`, `displayHeight`,
  `visibleRect`, `colorSpace`, `rotation`, `flip`, or `metadata()` at
  all — every spec member except the four listed above is simply absent.
* `AudioData`'s constructor (`web_codecs.rs:249-257`) has the identical
  bug: `init.data` (the actual sample buffer) is never read or stored.
  `copyTo()` (`web_codecs.rs:271-273`) is a hardcoded no-op — since there
  is no stored buffer, it could not copy anything even if implemented.
* `EncodedVideoChunk`/`EncodedAudioChunk` (`web_codecs.rs:197-225`) are
  the *one* case that *does* store the payload (`this._data = init.data`),
  yet `copyTo()` is still a hardcoded no-op comment `// Phase 0: no-op`
  that ignores both `this._data` and the `destination` argument — despite
  the data being sitting right there in `this._data`, unlike `VideoFrame`/
  `AudioData` this needs no codec work, just `destination.set(this._data)`
  plus a length check.
* `globalThis.VideoColorSpace` is never installed — the class referenced
  by `VideoFrame.prototype.colorSpace` in the spec doesn't exist at all
  (`typeof VideoColorSpace === 'undefined'`).

None of this requires FFmpeg/libav1 or any codec backend — it's plain
buffer storage, slicing, and geometry, the same category of "achievable
without a real DSP/codec pipeline" as the `OscillatorNode.type`/
`BiquadFilterNode.type` validation that already exists elsewhere in the
Phase-0 stubs (see the equivalent finding class in
[BUG-708](BUG-708-OPEN.md) for `webaudio`).

## Как воспроизвести

```js
// EncodedVideoChunk: data IS stored, copyTo() still throws it away
var chunk = new EncodedVideoChunk({type: 'key', timestamp: 0, data: new Uint8Array([10, 20, 30, 40])});
chunk.byteLength;                       // 4 (correct — reads this._data.byteLength)
var dest = new Uint8Array(4);
chunk.copyTo(dest);
dest[0];                                // 0, expected: 10 (this._data is right there and ignored)
chunk.copyTo(new Uint8Array(2));        // should throw (destination too small) — doesn't throw at all

// VideoFrame: source argument is discarded outright
var canvas = new OffscreenCanvas(64, 32);
var frame = new VideoFrame(canvas, {timestamp: 0});
frame.codedWidth;                       // 0, expected: 64 (never read from the canvas source)
typeof frame.allocationSize;            // "undefined", expected: "function"
typeof frame.copyTo;                    // "undefined", expected: "function"
typeof frame.displayWidth;              // "undefined", expected: "number"

typeof VideoColorSpace;                 // "undefined", expected: "function"
```

## Масштаб в WPT

Representative subtest messages from the 18 harness-OK tests:
`videoFrame-copyTo.any.html` (0/18) — 5x `TypeError: frame.allocationSize
is not a function`, 2x `TypeError: frame.copyTo is not a function`;
`videoFrame-copyTo-rgb.any.html` (0/66) — same pair, dominant across the
whole file; `videoFrame-odd-size.any.html`, `videoFrame-orientation.any.html`,
`videoFrame-construction.any.html` — `Cannot read properties of undefined
(reading 'width'/'x')` from code that expects `displayWidth`/`visibleRect`
to exist; `chunk-serialization.any.html`/`encoded-video-chunk.any.html` —
`assert_equals: copyDest[0] expected 10 but got 0` and `destination is not
large enough … did not throw`, both direct hits on `EncodedVideoChunk
.copyTo()`'s no-op; `audio-data-copyTo.any.html` (0/4) — every subtest
fails the same way (`AudioData.copyTo` no-op, no stored buffer);
`video-frame-serialization.any.html` — `VideoColorSpace is not defined`.

Not part of this finding (separate, already-documented gaps): the 103
TLS-gap TIMEOUTs; `ctx.drawImage`/`ctx.fillText is not a function` on an
`OffscreenCanvas` 2D context (already covered by the "16-member shim
against the element context's 59" note in `CAPABILITIES.md`, no `drawImage`/
`fillText`/`save`/`restore` there at all); `configure()`/`encode()`/
`decode()` reporting `NotSupportedError` (the acknowledged Phase-0 "no
codec backend" design, working as documented).

## Дальше

Store the source payload on construction (`this._data` for `AudioData`
mirroring the pattern `EncodedVideoChunk`/`EncodedAudioChunk` already use;
for `VideoFrame`, read the source canvas/bitmap/frame's already-decoded
pixels — Lumen's own canvas rasterization already has this data available,
no codec needed) and implement `copyTo()`/`allocationSize()` as real
buffer/geometry operations against it, including the destination-too-small
`RangeError`/`TypeError` checks the tests assert. Add `VideoColorSpace` as
a plain data-holding constructor. `displayWidth`/`displayHeight`/
`visibleRect`/`rotation`/`flip`/`metadata()` on `VideoFrame` can derive
from the same stored geometry.
