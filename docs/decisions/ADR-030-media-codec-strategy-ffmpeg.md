# ADR-030: Media codec strategy — FFmpeg via `ffmpeg-next`/`ffmpeg-sys-next`

## Status

Accepted (decision) — implementation blocked, see Consequences

## Date

2026-09-19

## Context

GAP-MEDIADECODE (`ROADMAP.md`, `BUGS.md` — no filed bug, size `L`): Lumen decodes
exactly one media format, animated GIF (`crates/js/src/video_bindings.rs`
module doc). `<video>`/`<audio>` never play `video/mp4`, `video/webm` or
`video/ogg` — `canPlayType` returns `""` for all three, and resource selection
ends in "dedicated media source failure steps" without the network layer ever
seeing a request. The 2026-09-17 revision confirmed there is no mature
pure-Rust decoder for H.264/VP8/VP9/AV1 in the ecosystem, and that writing one
is not a one-session task — this is squarely ADR-027's "vendored" side: a
codec bitstream is a spec/committee decision, not a Lumen product decision.
The practical vendored option is FFmpeg (libavformat/libavcodec/libswscale),
via Rust bindings.

The user made the strategy call explicitly on 2026-09-19: **FFmpeg целиком**
(the full FFmpeg suite via bindings), accepting the LGPL/GPL licensing
consequence, over the two alternatives offered (container + codec subset via
narrower crates like `dav1d`/`libvpx`; or an out-of-process decoder isolated
by IPC).

## Decision

Adopt **`ffmpeg-next`** (safe Rust wrapper) + **`ffmpeg-sys-next`** (raw FFI,
`bindgen`-generated) as the vendored media-decode dependency, linked against a
prebuilt FFmpeg **shared-dev** distribution (headers + import libs + DLLs,
via `FFMPEG_DIR`) rather than FFmpeg's own `build`/`static` Cargo feature —
the latter compiles FFmpeg from source (autoconf/make, needs a
MSYS2/yasm/nasm toolchain this project doesn't otherwise require) and is the
same class of "hours, tens of GB, separate toolchain decision" cost ADR
GAP-DOCALLDDA's `V8_FROM_SOURCE` revision measured and rejected for the same
reason. A shared-dev distribution (e.g. the Windows builds published under
`GyanD/codexffmpeg`, matched to `ffmpeg-sys-next`'s targeted FFmpeg version)
avoids that cost entirely — no FFmpeg compilation, only linking against
`.lib`/`.dll` FFmpeg already ships prebuilt.

Trait-anchor: a `VideoDecoder`/`MediaDecoder`-shaped trait in
`lumen_core::ext`, mirroring `ImageDecoder` — the FFmpeg-backed implementation
is the only implementation initially, but the trait keeps a future swap
(narrower codec crates, or an out-of-process decoder) a drop-in change, same
principle as every other provisional entry in `docs/plan/tech-stack.md` §5.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Container + codec subset (`dav1d`/`libvpx`/`symphonia`, narrower surface) | User's explicit call was "FFmpeg целиком", not a subset; also does not remove the FFI/bindgen toolchain problem described below — `libvpx`-sys and `dav1d`-sys are the same class of C-binding crate |
| Out-of-process decoder (isolated by IPC) | User's explicit call; also defers the same underlying "how do we call a native decoder from Rust" problem rather than resolving it, plus adds an IPC boundary and a process-lifecycle surface `lumen-ipc` doesn't have prior art for at video-frame rates |
| FFmpeg via `build`/`static` Cargo feature (compile FFmpeg from source) | Same shape of cost as `V8_FROM_SOURCE` (GAP-DOCALLDDA, 2026-09-17 revision): a separate heavyweight build toolchain (MSYS2, yasm/nasm, autoconf/make) this project does not otherwise carry, hours of build time. Shared-dev linking avoids it — FFmpeg is already compiled by the distributor |
| A different high-level FFmpeg crate (e.g. `ac-ffmpeg`, direct hand-rolled FFI) | `ffmpeg-next`/`ffmpeg-sys-next` is the most maintained, most widely used FFmpeg binding in the Rust ecosystem (basis for this ADR's default choice); the hand-rolled-FFI direction is exactly what the Consequences section below flags as the likely next step once the `bindgen` blocker is better understood, not a starting point |

## Consequences

- **Positive:** the decision itself is made and recorded — the next session
  does not need to re-litigate "which library" or "build from source vs.
  link prebuilt", only continue past the specific blocker below.
- **Negative — license surface:** FFmpeg's `full_build` distributions carry
  GPL-licensed components (as opposed to a narrower LGPL-only build); the
  user accepted this explicitly. `cargo-deny`'s license audit
  (`docs/plan/tech-stack.md` §Devtools) will need an explicit allow-list entry
  once the dependency actually lands in `Cargo.toml` — not done yet, see below.
- **Negative — un-landed, live toolchain blocker (measured 2026-09-19):**
  `ffmpeg-sys-next` uses `bindgen` to generate FFI struct layouts from FFmpeg's
  C headers at build time. On this project's mandated `x86_64-pc-windows-msvc`
  target, `bindgen`'s generated Rust structs disagree with `sizeof()` as
  measured by the very same `clang` parse for a **set** of FFmpeg structs
  (`AVFormatContext`, `AVFilterContext`, `AVFilterLink`, `AVFilterGraph`,
  `AVBPrint`, `AVOption`, `AVCodecParser`, …) *and* the C standard library's
  `struct tm` — `ffmpeg-sys-next`'s own generated compile-time layout
  assertions fail with `attempt to compute 1usize - Nusize, which would
  overflow`, meaning bindgen's Rust struct came out **smaller** than the C
  struct clang itself measured for the identical parse. Verified this is not
  an FFmpeg-version or header-availability problem (reproduced identically
  against both FFmpeg 9.0.1 and FFmpeg 7.1 shared-dev builds — matching
  `ffmpeg-sys-next 7.1.3`'s targeted major version made no difference), not a
  missing-header problem (`libavcodec/avfft.h`, removed from FFmpeg's own
  distribution since it was deprecated and dropped — a real but separate
  finding, worked around locally by vendoring the header text from the
  `n7.1` FFmpeg tag purely for `bindgen` to parse), and not a missing
  MSVC-environment problem (reproduced identically with `INCLUDE`/`LIB` unset
  and with them populated via `vcvars64.bat` from the installed VS 2026 Build
  Tools). A direct `clang -target x86_64-pc-windows-msvc` parse of a minimal
  `<time.h>` translation unit correctly measures `sizeof(struct tm) == 36`
  (confirmed via `-Xclang -ast-dump`, including the `MaxFieldAlignmentAttr`
  MSVC ABI pragma FFmpeg's headers apply) — so `bindgen`'s own AST→Rust-struct
  translation step, not clang's parse, is where the size is lost. This is
  narrower than "no working path exists": it is a specific, reproducible
  `bindgen`-on-MSVC translation defect, likely related to how `bindgen`
  handles the anonymous unions/bitfields or the `MaxFieldAlignmentAttr` pragma
  present in several of the failing structs.
- **Future — most promising next step:** none of the structs that fail are
  ones the video-decode path needs field-level access to — `AVFormatContext`,
  `AVCodecContext`, `AVFrame`, `AVFilterGraph` etc. are used through
  `ffmpeg-next`'s and FFmpeg's own accessor functions in real code, not by
  reading struct fields directly from Rust. A **hand-written, minimal FFI
  layer** (a handful of `extern "C"` function declarations against the
  already-linked FFmpeg DLLs — `avformat_open_input`, `avcodec_send_packet`,
  `sws_scale`, etc. — with the handful of structs Rust code actually touches
  declared as `#[repr(C)] struct Opaque { _private: [u8; 0] }` behind
  pointers) sidesteps `bindgen`'s struct-layout translation entirely and does
  not carry this defect, at the cost of writing (and maintaining) that FFI
  surface ourselves instead of consuming it whole from `ffmpeg-sys-next`.
  Cheaper alternative worth trying first: pin an older `ffmpeg-sys-next`
  release (pre-7.x) against a matching older FFmpeg shared-dev build, in case
  the defect is a regression introduced by a specific `bindgen`/FFmpeg-header
  version combination rather than inherent to the crate. Neither has been
  attempted yet — this ADR's job is recording the decision and the precise
  blocker, not resolving it.
- **Not done:** no dependency was added to any `Cargo.toml` in this revision —
  `ffmpeg-sys-next` does not compile on this project's target as measured
  above, and landing a dependency that breaks `cargo build --workspace` is
  worse than landing none. `docs/plan/tech-stack.md` §5's provisional
  accelerators table carries this entry with status "blocked", not "adopted".
