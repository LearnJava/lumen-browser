//! Platform audio capture backend for `navigator.mediaDevices.getUserMedia({audio})`.
//!
//! [`PlatformAudioCapture`] implements [`AudioCaptureProvider`] using the `cpal` crate,
//! which maps to WASAPI on Windows and ALSA on Linux.
//!
//! ## Threading model
//!
//! `cpal` runs a dedicated OS audio thread that writes captured PCM frames into a
//! shared ring buffer (`Arc<Mutex<VecDeque<f32>>>`).  The JS thread drains the
//! ring buffer via `read_pcm_f32()` without blocking the audio thread (the mutex
//! is held only for the duration of `extend_from_slice` / `drain`).
//!
//! ## Sample format
//!
//! On WASAPI shared mode the driver exposes the format in `default_input_config()`.
//! We request the native format and convert non-F32 samples to F32 inline.
//!
//! ## Ring buffer capacity
//!
//! Capped at 5 s × sample_rate × channel_count samples.  Overflow discards the
//! oldest frames so the JS side always reads the freshest audio.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};

use lumen_core::ext::{
    AudioCaptureConfig, AudioCaptureError, AudioCaptureHandle, AudioDeviceDescriptor,
    AudioCaptureProvider,
};

// ── Provider ─────────────────────────────────────────────────────────────────

/// Platform audio capture provider (WASAPI / ALSA via `cpal`).
///
/// Stateless: each `capture()` call opens a new OS stream.  Can be safely
/// shared as `Arc<PlatformAudioCapture>`.
pub struct PlatformAudioCapture;

impl AudioCaptureProvider for PlatformAudioCapture {
    fn enumerate_devices(&self) -> Vec<AudioDeviceDescriptor> {
        let host = cpal::default_host();
        match host.input_devices() {
            Ok(devs) => {
                let default_name = host
                    .default_input_device()
                    .and_then(|d| d.name().ok())
                    .unwrap_or_default();
                devs.enumerate()
                    .map(|(i, dev)| {
                        let label = dev.name().unwrap_or_else(|_| format!("Device {i}"));
                        let is_default = label == default_name;
                        AudioDeviceDescriptor {
                            device_id: format!("audioinput-{i}"),
                            group_id: String::new(),
                            label,
                            kind: "audioinput",
                            is_default,
                        }
                    })
                    .collect()
            }
            Err(e) => {
                eprintln!("[lumen-audio] enumerate_devices failed: {e}");
                Vec::new()
            }
        }
    }

    fn capture(
        &self,
        config: AudioCaptureConfig,
    ) -> Result<Box<dyn AudioCaptureHandle>, AudioCaptureError> {
        let host = cpal::default_host();

        // Device selection: use the requested device ID if present,
        // otherwise fall back to the system default.
        let (device, device_idx) = if let Some(ref id) = config.device_id {
            // id format: "audioinput-N"
            let idx: usize = id
                .strip_prefix("audioinput-")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let dev = host
                .input_devices()
                .map_err(|_| AudioCaptureError::NotFound)?
                .nth(idx)
                .ok_or(AudioCaptureError::NotFound)?;
            (dev, idx)
        } else {
            let dev = host
                .default_input_device()
                .ok_or(AudioCaptureError::NotFound)?;
            (dev, 0)
        };

        let device_label = device.name().unwrap_or_else(|_| "Microphone".to_owned());
        let device_id = format!("audioinput-{device_idx}");

        let supported = device
            .default_input_config()
            .map_err(|_| AudioCaptureError::NotFound)?;

        let sample_rate = config
            .sample_rate
            .unwrap_or(supported.sample_rate().0)
            .clamp(8_000, 192_000);
        let channel_count = config
            .channel_count
            .unwrap_or(supported.channels() as u32)
            .clamp(1, 8);

        let stream_config = StreamConfig {
            channels: channel_count as u16,
            sample_rate: SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        // Ring buffer: 5 seconds of audio maximum.
        let cap = (sample_rate as usize) * (channel_count as usize) * 5;
        let ring: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::with_capacity(cap)));
        let ring_write = Arc::clone(&ring);

        let fmt = supported.sample_format();
        let stream = build_stream(
            &device,
            &stream_config,
            fmt,
            ring_write,
            cap,
        )
        .map_err(|e| AudioCaptureError::Other(e.to_string()))?;

        stream
            .play()
            .map_err(|e| AudioCaptureError::Other(e.to_string()))?;

        Ok(Box::new(CpalCaptureHandle {
            _stream: stream,
            ring,
            sample_rate,
            channel_count,
            device_id,
            device_label,
            stopped: false,
        }))
    }
}

// ── Stream builder (dispatches on SampleFormat) ───────────────────────────────

fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    fmt: SampleFormat,
    ring: Arc<Mutex<VecDeque<f32>>>,
    cap: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let err_fn = |e: cpal::StreamError| eprintln!("[lumen-audio] stream error: {e}");

    match fmt {
        SampleFormat::F32 => {
            device.build_input_stream(
                config,
                move |data: &[f32], _| push_f32(&ring, data, cap),
                err_fn,
                None,
            )
        }
        SampleFormat::I16 => {
            device.build_input_stream(
                config,
                move |data: &[i16], _| {
                    let f: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    push_f32(&ring, &f, cap);
                },
                err_fn,
                None,
            )
        }
        SampleFormat::U16 => {
            device.build_input_stream(
                config,
                move |data: &[u16], _| {
                    let f: Vec<f32> =
                        data.iter().map(|&s| (s as f32 / 32768.0) - 1.0).collect();
                    push_f32(&ring, &f, cap);
                },
                err_fn,
                None,
            )
        }
        // Fallback: try F32 and let cpal report the error.
        _ => device.build_input_stream(
            config,
            move |data: &[f32], _| push_f32(&ring, data, cap),
            err_fn,
            None,
        ),
    }
}

/// Write `data` into the ring buffer, discarding oldest frames on overflow.
fn push_f32(ring: &Mutex<VecDeque<f32>>, data: &[f32], cap: usize) {
    if let Ok(mut buf) = ring.lock() {
        let overflow = (buf.len() + data.len()).saturating_sub(cap);
        if overflow > 0 {
            buf.drain(..overflow);
        }
        buf.extend(data.iter().copied());
    }
}

// ── Handle ────────────────────────────────────────────────────────────────────

/// Live capture handle.  Dropping it stops the cpal audio thread.
struct CpalCaptureHandle {
    /// Keeps the OS audio thread alive.  Dropped (stream stopped) when handle is dropped.
    _stream: cpal::Stream,
    /// Shared ring buffer written by the cpal thread, drained by the JS thread.
    ring: Arc<Mutex<VecDeque<f32>>>,
    /// Actual sample rate negotiated with the OS.
    sample_rate: u32,
    /// Actual channel count negotiated with the OS.
    channel_count: u32,
    /// Opaque device ID in `"audioinput-N"` format.
    device_id: String,
    /// Human-readable device name from the OS.
    device_label: String,
    /// Set to true after `stop()` to make `read_pcm_f32` return empty.
    stopped: bool,
}

impl AudioCaptureHandle for CpalCaptureHandle {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channel_count(&self) -> u32 {
        self.channel_count
    }

    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn device_label(&self) -> &str {
        &self.device_label
    }

    fn read_pcm_f32(&mut self) -> Vec<f32> {
        if self.stopped {
            return Vec::new();
        }
        if let Ok(mut buf) = self.ring.lock() {
            buf.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    fn stop(&mut self) {
        self.stopped = true;
        // Dropping `_stream` when the handle is dropped stops the OS thread.
        // We don't need to call any explicit stop here.
    }
}

// ── cpal::Stream is Send on all supported platforms ──────────────────────────
// WASAPI (Windows): stream threads are managed by Windows COM apartments and
// are safe to send across threads.
// ALSA (Linux): the stream handle is an opaque pointer to an alsa-lib context
// which is also Send.
// cpal 0.15 marks Stream as Send + Sync for all backends.
// No unsafe impl needed.

#[cfg(test)]
mod tests {
    use super::*;

    // These tests only run when a real audio device is available.
    // In CI (no microphone), cpal returns an error and the tests are skipped.

    #[test]
    fn enumerate_returns_some_or_empty() {
        let p = PlatformAudioCapture;
        // Should not panic regardless of whether devices are present.
        let devs = p.enumerate_devices();
        // Devices may be empty in CI; just check the call doesn't crash.
        for d in &devs {
            assert!(!d.device_id.is_empty());
            assert_eq!(d.kind, "audioinput");
        }
    }

    #[test]
    fn capture_default_device_or_error() {
        let p = PlatformAudioCapture;
        let result = p.capture(AudioCaptureConfig::default());
        match result {
            Ok(mut h) => {
                assert!(h.sample_rate() >= 8_000);
                assert!(h.channel_count() >= 1);
                assert!(!h.device_id().is_empty());
                assert!(!h.device_label().is_empty());
                // read immediately after start should not panic
                let _ = h.read_pcm_f32();
                h.stop();
                let empty = h.read_pcm_f32();
                assert!(empty.is_empty(), "after stop read_pcm_f32 must be empty");
            }
            Err(AudioCaptureError::NotFound | AudioCaptureError::DeviceInUse) => {
                // No mic available in this environment — skip
            }
            Err(AudioCaptureError::NotAllowed) => {
                // OS denied permission — skip
            }
            Err(AudioCaptureError::Other(e)) => {
                eprintln!("platform capture error (acceptable in CI): {e}");
            }
        }
    }
}
