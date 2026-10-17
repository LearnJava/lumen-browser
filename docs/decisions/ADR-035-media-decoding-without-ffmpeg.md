# ADR-035: Media decoding without FFmpeg — rav1d + OS decoders + Rust audio, on every platform

## Status

Accepted (user, 2026-10-12). **Supersedes the decision of [ADR-030](ADR-030-media-codec-strategy-ffmpeg.md)**
(«FFmpeg целиком»). The `lumen-media-ffmpeg` crate stays in the tree behind its opt-in `ffmpeg-video`
feature as a developer/testing backend only — it is never part of a release build.

## Date

2026-10-12

## Context

ADR-030 (2026-09-19) chose the full FFmpeg suite; the hand-rolled FFI crate landed and the shell
playback path (`Lumen::tick_video_ffmpegs` in `crates/shell/src/page_load.rs`: `frame_at(secs)` by
the media clock, `decode_audio_pcm` into `platform/video_audio_sink.rs`) works — but only behind an
opt-in feature, so the default build still plays nothing but animated GIF. Before turning it on, the
site-compatibility study ([`docs/research/site-compat/areas/04-media-drm-codecs-rtc.md`](../research/site-compat/areas/04-media-drm-codecs-rtc.md) §3)
surfaced three problems with shipping FFmpeg:

- **Patents.** Bundled software decoders for H.264, AAC and HEVC need royalty licences (Chrome pays;
  Chromium's default build ships without them; Firefox takes them from the OS; Ladybird removed them,
  PR #12164).
- **Licence.** Lumen is MPL-2.0; a full FFmpeg build carries GPL components, which would put the whole
  distribution under GPL obligations.
- **Size.** A full shared FFmpeg is on the order of a hundred megabytes of DLLs (estimate) against the
  «lightweight» positioning.

The user also required that the answer work on **every platform** Lumen targets (Windows, macOS,
Linux; CI already builds all three), not just Windows, and asked for the simplest path built from free,
ready-made components. Of the options presented on 2026-10-12 (full FFmpeg; LGPL FFmpeg with free
codecs + OS decoders; GStreamer as a ready media engine; free codecs only; no FFmpeg — rav1d + OS
decoders), the user chose the last.

Facts in the tree that make this cheap: the `VideoDecoder` / `VideoDecodeSession` traits in
`lumen_core::ext` are FFmpeg-independent; `rav1d` (pure-Rust dav1d port, BSD-2) is already a dependency
of `lumen-image` for AVIF with a working `dav1d_open`/`dav1d_send_data` wrapper
(`crates/engine/image/src/avif/mod.rs`); MP3, FLAC, Vorbis and WAV are already decoded in pure Rust
(`rodio` + `symphonia`, `claxon`, `lewton`, `hound`), and the `ogg` crate is present.

## Decision

1. **One decoder registry behind the existing traits.** A native `VideoDecoder` implementation
   («lumen-native») combines a Rust demuxer with per-codec backends. For each codec the registry picks,
   in order: OS hardware decoder → OS software decoder → bundled free decoder. `canPlayType`,
   `MediaSource.isTypeSupported` and `mediaCapabilities.decodingInfo` answer from what this registry can
   actually open **on this machine** ([ADR-034](ADR-034-site-compatibility-strategy.md) §2: feature
   detection never lies). The shell playback path is renamed from «ffmpeg» to decoder-agnostic and fed
   by the registry in the default build.
2. **Bundled, on every platform (free codecs only):**
   - AV1 — `rav1d` (shared with AVIF; the wrapper moves to a place both image and video use);
   - MP3, FLAC, Vorbis, WAV — the existing pure-Rust decoders;
   - containers — MP4/ISOBMFF via `mp4parse` (Mozilla, MPL-2.0, used in Firefox), WebM/Matroska via a
     Rust demuxer chosen at implementation time (ADR-027 justification in that commit), Ogg via `ogg`.
3. **Patent-encumbered codecs (H.264, HEVC, AAC) come only from the operating system**, never bundled:
   - **Windows** — Media Foundation, hardware via D3D11VA. Windows N/KN without the Media Feature Pack
     is detected and the user gets a precise hint.
   - **macOS** — VideoToolbox (H.264/HEVC) and AudioToolbox (AAC); always present.
   - **Linux** — VA-API (hardware) first; software fallback only through codecs the distribution
     already ships, loaded at runtime and never bundled (the mechanism — GStreamer or the system
     libavcodec — is decided in `COMPAT-MEDIA-OSDEC-LINUX`: **GStreamer through `dlopen`**, see
     `subsystems/media-native.md`; `decodebin` picks the VA-API decoder by rank). Missing codecs → a distribution-specific
     hint (Ubuntu/Debian, Fedora + RPM Fusion, openSUSE + Packman, Flatpak `ffmpeg-full` extension).
   - **AAC in pure Rust** (`symphonia`) as a fallback only after the owner's legal check
     (`COMPAT-BIZ-CODECS-LEGAL`).
4. **Later, when a measured need appears:** VP8/VP9 via `libvpx` (BSD-3) and Opus via `libopus`
   (BSD-3). Until then sites are told honestly that VP9/Opus are unsupported — YouTube then negotiates
   AV1 or H.264 + AAC. WebRTC (UX-WEBRTC) needs Opus + VP8 and H.264 Constrained Baseline; it pulls these
   in on its own schedule.
5. **Nothing FFmpeg-derived ships with Lumen.** `lumen-media-ffmpeg` remains an opt-in developer
   backend (`ffmpeg-video`), useful for A/B-checking the native path; it is excluded from release
   packaging and from the default feature set.
6. **MSE** (adaptive streaming: YouTube, VK Видео, Rutube) is built once, on top of this registry, for
   all platforms (`COMPAT-MSE`). DRM stays separate (ADR-034 §9).

### Addendum 2026-10-15 (UX-VIDEO-AV1): `asm` у rav1d

Замер 1080p/30 кадр/с, 10 с, ≈4,5 Мбит/с на машине разработчика (i5-10210U, 8 потоков): без `asm` декодер работает на ≈0,9× реального времени — не хватает; с `asm` (nasm) ≈3,3×, на одном потоке 2,25× ([docs/perf/av1-decode.md](../perf/av1-decode.md)). Решение: общий крейт `lumen-av1` держит `asm` за фичей (`lumen-shell --features av1-asm`), в default она выключена, чтобы `cargo build` не требовал nasm; **релизные сборки обязаны её включать**. Аппаратный AV1 через ОС остаётся быстрым путём там, где он есть.

### Addendum 2026-10-15 (UX-VIDEO-NATIVE): реализация п.1

Крейт `lumen-media-native` (`NativeVideoDecoder`, реестр бэкендов кодеков, `NativeSession` с общим курсором демуксера) подключён к `<video>` в штатной сборке: `lumen_js::set_video_decoder` ставит декодер до создания JS-контекстов, `canPlayType` отвечает по реестру, путь воспроизведения шелла нейтрален (`tick_video_sessions`, `pending_container_loads`). FFmpeg остался только за `--features ffmpeg-video` как dev-бэкенд на месте того же слота. Реализованы бэкенды AV1 (`lumen-av1`) и Vorbis (`lewton`); H.264/HEVC/AAC — задачи `COMPAT-MEDIA-OSDEC-*`, VP9/Opus — `UX-VIDEO-VP9`/`UX-VIDEO-OPUS`. Подробности — [subsystems/media-native.md](../../subsystems/media-native.md).

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| FFmpeg целиком (ADR-030) | Patent exposure for H.264/AAC/HEVC on every platform, GPL obligations on an MPL-2.0 product, ~100 MB of DLLs |
| LGPL FFmpeg with free codecs + OS decoders for patented ones | Legally sound, but still ships a large C dependency for codecs we can cover with `rav1d` and the Rust audio decoders already in the tree |
| GStreamer as the media engine (Servo, WebKitGTK) | Most ready-made (demux, decode, A/V sync, OS plugins), but a large framework whose runtime must be bundled on Windows/macOS, plugin-set dependent behaviour, harder debugging; MSE on top of it is still our work. Remains an option for the Linux software fallback only |
| Free codecs only, no H.264 | MP4/H.264 is most of web video (X, Instagram, news sites, HLS players, YouTube's 360p fallback) — «video does not work» for most users |
| Stay with GIF only | Level 0 failure: web video does not play at all |

## Consequences

- **Positive:** no patent or GPL exposure; small footprint; mostly Rust; hardware decoding saves battery;
  all licences compatible with MPL-2.0; the existing traits and shell playback path are reused, so the
  work is a new decoder implementation plus registry, not a new pipeline.
- **Negative / trade-offs:** several small components and glue instead of one block; three per-OS
  decoder backends (Windows, macOS, Linux) that must be tested on their own platforms (CI runners plus
  a real machine per OS); on Linux the result depends on the distribution's codecs and the GPU driver;
  VP9/Opus content is not played until `libvpx`/`libopus` land.
- **Revisit when:** H.264 patents expire in the remaining jurisdictions (per the study: EU, Japan and
  China in 2025, most of the US wave in 2027, the last one in Brazil in 2030 — unverified) — a bundled
  software H.264 fallback for Windows N and codec-less Linux becomes possible; OS-decoder coverage on
  Linux proves insufficient on the compat corpus (then GStreamer or system libavcodec as a documented
  fallback); a measured need for VP9 or Opus.
