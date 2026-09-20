//! Streaming PCM playback for FFmpeg-backed `<video>` audio (GAP-MEDIADECODE
//! срез 14).
//!
//! Unlike [`super::audio_player::PlatformAudioPlayer`], which decodes an
//! `<audio>`'s whole container up front via `rodio::Decoder`, this sink
//! receives already-decoded interleaved S16 PCM chunks
//! (`VideoDecodeSession::decode_audio_pcm`) from `Lumen::tick_video_ffmpegs`
//! and queues them on a `rodio::Sink` as `rodio::buffer::SamplesBuffer`
//! sources, one small chunk per render tick.

use rodio::{OutputStream, OutputStreamHandle, Sink};

/// One open output stream + sink for a single `<video>` node's audio track.
///
/// `OutputStream` is `!Send`; this is only ever constructed and used from
/// `Lumen::tick_video_ffmpegs`, which — like the rest of `Lumen`'s per-page
/// state — runs entirely on the UI thread, so it never needs to cross a
/// thread boundary.
pub(crate) struct VideoPcmAudioSink {
    /// Kept alive only so the underlying device stream is not torn down —
    /// never read after construction.
    _stream: OutputStream,
    /// Kept alive only because it is required to construct `sink` and
    /// dropping it would close the stream — never read after construction.
    _stream_handle: OutputStreamHandle,
    sink: Sink,
}

impl VideoPcmAudioSink {
    /// Open a new output stream and sink.
    ///
    /// Returns `None` if no audio device is available. That is treated as
    /// non-fatal by the caller: missing audio hardware must not block video
    /// playback, it just plays silently — the same stance the shell already
    /// takes for `<audio>` (`PlatformAudioPlayer` sets `has_error` but still
    /// leaves `readyState` progressing).
    pub(crate) fn new() -> Option<Self> {
        let (stream, stream_handle) = OutputStream::try_default().ok()?;
        let sink = Sink::try_new(&stream_handle).ok()?;
        Some(Self {
            _stream: stream,
            _stream_handle: stream_handle,
            sink,
        })
    }

    /// Queue an interleaved S16 PCM chunk for playback.
    ///
    /// No-op for an empty chunk (EOF or a rounding-to-zero sample count) and
    /// for a malformed track description (`channels == 0` — `SamplesBuffer`
    /// panics on that instead of erroring).
    pub(crate) fn push_pcm(&self, samples: Vec<i16>, sample_rate: u32, channels: u16) {
        if samples.is_empty() || channels == 0 || sample_rate == 0 {
            return;
        }
        self.sink.append(rodio::buffer::SamplesBuffer::new(channels, sample_rate, samples));
    }

    /// Mirror the node's `paused` playback state onto the sink. Idempotent —
    /// safe to call every tick regardless of whether the state actually
    /// changed since the last call.
    pub(crate) fn set_paused(&self, paused: bool) {
        if paused {
            self.sink.pause();
        } else {
            self.sink.play();
        }
    }

    /// Set the linear output volume (`0.0` silent .. `1.0` unattenuated,
    /// values above `1.0` amplify same as `rodio::Sink::set_volume`).
    /// Idempotent — safe to call every tick.
    pub(crate) fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI/dev machines without a usable audio device return `None` here
    /// (same non-fatal shape `PlatformAudioPlayer` already tolerates) — these
    /// tests skip instead of failing when that happens, since the point is
    /// exercising `push_pcm`'s guards, not asserting hardware presence.
    fn sink_or_skip() -> Option<VideoPcmAudioSink> {
        VideoPcmAudioSink::new()
    }

    #[test]
    fn push_pcm_empty_is_noop() {
        let Some(sink) = sink_or_skip() else { return };
        sink.push_pcm(Vec::new(), 44100, 2);
    }

    #[test]
    fn push_pcm_zero_channels_is_noop() {
        let Some(sink) = sink_or_skip() else { return };
        sink.push_pcm(vec![1, 2, 3, 4], 44100, 0);
    }

    #[test]
    fn push_pcm_zero_sample_rate_is_noop() {
        let Some(sink) = sink_or_skip() else { return };
        sink.push_pcm(vec![1, 2, 3, 4], 0, 2);
    }

    #[test]
    fn push_pcm_valid_chunk_does_not_panic() {
        let Some(sink) = sink_or_skip() else { return };
        sink.push_pcm(vec![0i16; 2048], 44100, 2);
    }

    #[test]
    fn set_paused_toggle_does_not_panic() {
        let Some(sink) = sink_or_skip() else { return };
        sink.set_paused(true);
        sink.set_paused(false);
    }

    #[test]
    fn set_volume_does_not_panic() {
        let Some(sink) = sink_or_skip() else { return };
        sink.set_volume(0.0);
        sink.set_volume(0.5);
        sink.set_volume(1.0);
    }
}
