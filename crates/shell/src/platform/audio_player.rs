//! Platform audio playback backend for HTMLAudioElement Phase 1.
//!
//! Each audio element gets a unique `handle` (u64).  Loading and playback happen
//! on a dedicated per-handle OS thread so that `rodio::OutputStream` (which is
//! `!Send`) stays on the thread that created it.  The owning thread receives
//! commands via an `mpsc::Sender<AudioCmd>` and reports state back through
//! `Arc<Mutex<AudioElementState>>`.
//!
//! # Audio formats
//!
//! Via `rodio` feature flags selected in `Cargo.toml`:
//! - WAV  (wav)
//! - MP3  (mp3)
//! - FLAC (flac)
//! - OGG/Vorbis (vorbis)
//!
//! # Thread model
//!
//! ```text
//! JS thread ─── __lumen_audio_load(handle, url) ──► network thread (fetch)
//!                                                         │
//!                                                    bytes ready
//!                                                         │
//!                                                    AudioCmd::Load(bytes) ──► audio thread
//!                                                                                   │
//!                                                                             rodio Sink
//! JS thread ─── __lumen_audio_play(handle) ──► AudioCmd::Play ────────────► Sink::play()
//! JS thread ─── __lumen_audio_pause(handle) ──► AudioCmd::Pause ──────────► Sink::pause()
//! ```

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use lumen_core::ext::AudioPlaybackProvider;

// ── State per audio element ───────────────────────────────────────────────────

/// Shared state accessed by both the audio thread and the querying JS thread.
#[derive(Default)]
struct AudioElementState {
    /// W3C readyState: 0=HAVE_NOTHING … 4=HAVE_ENOUGH_DATA.
    ready_state: u32,
    /// Duration in seconds; NaN until metadata is decoded.
    duration: f64,
    /// Whether the Sink is currently paused (or not started).
    paused: bool,
    /// Whether playback has ended (reached the end of the source).
    ended: bool,
    /// Whether a load or decode error occurred.
    has_error: bool,
}

impl AudioElementState {
    fn new() -> Self {
        Self {
            ready_state: 0,
            duration: f64::NAN,
            paused: true,
            ended: false,
            has_error: false,
        }
    }
}

// ── Audio thread commands ─────────────────────────────────────────────────────

enum AudioCmd {
    /// Load audio bytes decoded from the given URL (fetch happened outside audio thread).
    Load(Vec<u8>),
    Play,
    Pause,
    Stop,
    /// Seek to `secs` seconds.
    Seek(f64),
    SetVolume(f32),
    SetPlaybackRate(f32),
    Free,
}

// ── Per-handle entry ──────────────────────────────────────────────────────────

struct HandleEntry {
    state: Arc<Mutex<AudioElementState>>,
    tx: std::sync::mpsc::SyncSender<AudioCmd>,
}

// ── PlatformAudioPlayer ───────────────────────────────────────────────────────

/// Shell-side implementation of `AudioPlaybackProvider` using `rodio`.
///
/// Installed by the shell before JS starts:
/// ```ignore
/// lumen_js::set_audio_playback_provider(Arc::new(PlatformAudioPlayer::new()));
/// ```
pub struct PlatformAudioPlayer {
    next_id: AtomicU64,
    handles: Mutex<HashMap<u64, HandleEntry>>,
}

impl PlatformAudioPlayer {
    /// Create a new player (no OS resources allocated until the first handle).
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            handles: Mutex::new(HashMap::new()),
        }
    }

    fn with_handle<R>(&self, handle: u64, f: impl FnOnce(&HandleEntry) -> R) -> Option<R> {
        self.handles.lock().unwrap().get(&handle).map(f)
    }

    fn send_cmd(&self, handle: u64, cmd: AudioCmd) {
        if let Some(entry) = self.handles.lock().unwrap().get(&handle) {
            let _ = entry.tx.try_send(cmd);
        }
    }
}

impl Default for PlatformAudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Audio thread body ─────────────────────────────────────────────────────────

fn audio_thread(
    rx: std::sync::mpsc::Receiver<AudioCmd>,
    state: Arc<Mutex<AudioElementState>>,
) {
    use rodio::{Decoder, OutputStream, Sink, Source};

    // `OutputStream` is `!Send`; create it on this thread.
    let (_stream, stream_handle) = match OutputStream::try_default() {
        Ok(pair) => pair,
        Err(_) => {
            state.lock().unwrap().has_error = true;
            return;
        }
    };

    let sink = match Sink::try_new(&stream_handle) {
        Ok(s) => s,
        Err(_) => {
            state.lock().unwrap().has_error = true;
            return;
        }
    };
    sink.pause(); // start paused

    let mut loaded_bytes: Option<Arc<Vec<u8>>> = None;
    let mut volume: f32 = 1.0;
    let mut rate: f32 = 1.0;

    for cmd in rx {
        match cmd {
            AudioCmd::Load(bytes) => {
                let cursor = Cursor::new(bytes.clone());
                match Decoder::new(cursor) {
                    Ok(source) => {
                        // Probe duration before appending to sink.
                        let dur = {
                            let c2 = Cursor::new(bytes.clone());
                            rodio::Decoder::new(c2)
                                .ok()
                                .and_then(|d| d.total_duration())
                                .map(|d| d.as_secs_f64())
                                .unwrap_or(f64::NAN)
                        };
                        {
                            let mut st = state.lock().unwrap();
                            st.duration = dur;
                            st.ready_state = 4;
                            st.ended = false;
                            st.has_error = false;
                            st.paused = true;
                        }
                        loaded_bytes = Some(Arc::new(bytes));
                        sink.stop();
                        sink.set_volume(volume);
                        sink.set_speed(rate);
                        sink.append(source);
                        sink.pause();
                    }
                    Err(_) => {
                        state.lock().unwrap().has_error = true;
                    }
                }
            }

            AudioCmd::Play => {
                if sink.is_paused() || sink.empty() {
                    if sink.empty() {
                        // Re-queue bytes if sink was exhausted (e.g. after ended + play again).
                        if let Some(b) = &loaded_bytes {
                            let c = Cursor::new(b.as_ref().clone());
                            if let Ok(source) = Decoder::new(c) {
                                sink.set_volume(volume);
                                sink.set_speed(rate);
                                sink.append(source);
                            }
                        }
                    }
                    sink.play();
                    let mut st = state.lock().unwrap();
                    st.paused = false;
                    st.ended = false;
                }
            }

            AudioCmd::Pause => {
                sink.pause();
                state.lock().unwrap().paused = true;
            }

            AudioCmd::Stop => {
                sink.stop();
                let mut st = state.lock().unwrap();
                st.paused = true;
                st.ended = false;
            }

            AudioCmd::Seek(secs) => {
                // rodio 0.19 does not expose sample-accurate seek for all decoders.
                // Best-effort: stop → re-queue → skip to approximate position.
                if let Some(b) = &loaded_bytes {
                    let c = Cursor::new(b.as_ref().clone());
                    if let Ok(source) = Decoder::new(c) {
                        let was_playing = !sink.is_paused();
                        sink.stop();
                        let skipped = source.skip_duration(std::time::Duration::from_secs_f64(
                            secs.max(0.0),
                        ));
                        sink.set_volume(volume);
                        sink.set_speed(rate);
                        sink.append(skipped);
                        if was_playing {
                            sink.play();
                        } else {
                            sink.pause();
                        }
                    }
                }
            }

            AudioCmd::SetVolume(v) => {
                volume = v;
                sink.set_volume(v);
            }

            AudioCmd::SetPlaybackRate(r) => {
                rate = r;
                sink.set_speed(r);
            }

            AudioCmd::Free => {
                sink.stop();
                break;
            }
        }

        // Update `ended` flag: sink is empty and not paused → playback finished.
        if sink.empty() && !sink.is_paused() {
            let mut st = state.lock().unwrap();
            st.ended = true;
            st.paused = true;
        }
    }
}

// ── AudioPlaybackProvider implementation ─────────────────────────────────────

impl AudioPlaybackProvider for PlatformAudioPlayer {
    fn alloc_handle(&self) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let state = Arc::new(Mutex::new(AudioElementState::new()));
        let (tx, rx) = std::sync::mpsc::sync_channel::<AudioCmd>(16);
        let state_clone = Arc::clone(&state);
        thread::Builder::new()
            .name(format!("lumen-audio-{id}"))
            .spawn(move || audio_thread(rx, state_clone))
            .expect("spawn audio thread");
        self.handles
            .lock()
            .unwrap()
            .insert(id, HandleEntry { state, tx });
        id
    }

    fn free_handle(&self, handle: u64) {
        self.send_cmd(handle, AudioCmd::Free);
        self.handles.lock().unwrap().remove(&handle);
    }

    fn load(&self, handle: u64, url: &str) {
        let url = url.to_owned();
        // Reset ready_state to loading (1) to indicate load started.
        if let Some(entry) = self.handles.lock().unwrap().get(&handle) {
            entry.state.lock().unwrap().ready_state = 1;
        }
        // Fetch bytes on a background thread, then forward to audio thread.
        if let Some(tx) = self
            .handles
            .lock()
            .unwrap()
            .get(&handle)
            .map(|e| e.tx.clone())
        {
            let state = self
                .handles
                .lock()
                .unwrap()
                .get(&handle)
                .map(|e| Arc::clone(&e.state));
            thread::Builder::new()
                .name(format!("lumen-audio-fetch-{handle}"))
                .spawn(move || {
                    match fetch_audio_bytes(&url) {
                        Ok(bytes) => {
                            let _ = tx.send(AudioCmd::Load(bytes));
                        }
                        Err(_) => {
                            if let Some(st) = state {
                                st.lock().unwrap().has_error = true;
                            }
                        }
                    }
                })
                .expect("spawn audio fetch thread");
        }
    }

    fn play(&self, handle: u64) {
        self.send_cmd(handle, AudioCmd::Play);
    }

    fn pause(&self, handle: u64) {
        self.send_cmd(handle, AudioCmd::Pause);
    }

    fn stop(&self, handle: u64) {
        self.send_cmd(handle, AudioCmd::Stop);
        if let Some(entry) = self.handles.lock().unwrap().get(&handle) {
            let mut st = entry.state.lock().unwrap();
            st.paused = true;
            st.ended = false;
        }
    }

    fn seek(&self, handle: u64, time_secs: f64) {
        self.send_cmd(handle, AudioCmd::Seek(time_secs));
    }

    fn set_volume(&self, handle: u64, volume: f64) {
        self.send_cmd(handle, AudioCmd::SetVolume(volume as f32));
    }

    fn set_playback_rate(&self, handle: u64, rate: f64) {
        self.send_cmd(handle, AudioCmd::SetPlaybackRate(rate as f32));
    }

    fn current_time(&self, _handle: u64) -> f64 {
        // rodio 0.19 `Sink::get_pos()` returns `Duration`.
        // We'd need to call it on the audio thread; for Phase 1 we return 0.0
        // as a conservative approximation (timeupdate polling is JS-driven anyway).
        // TODO(PH3-11-phase2): route get_pos() through shared state.
        0.0
    }

    fn duration(&self, handle: u64) -> f64 {
        self.with_handle(handle, |e| e.state.lock().unwrap().duration)
            .unwrap_or(f64::NAN)
    }

    fn is_paused(&self, handle: u64) -> bool {
        self.with_handle(handle, |e| e.state.lock().unwrap().paused)
            .unwrap_or(true)
    }

    fn is_ended(&self, handle: u64) -> bool {
        self.with_handle(handle, |e| e.state.lock().unwrap().ended)
            .unwrap_or(false)
    }

    fn ready_state(&self, handle: u64) -> u32 {
        self.with_handle(handle, |e| e.state.lock().unwrap().ready_state)
            .unwrap_or(0)
    }

    fn has_error(&self, handle: u64) -> bool {
        self.with_handle(handle, |e| e.state.lock().unwrap().has_error)
            .unwrap_or(false)
    }
}

// ── HTTP fetch helper ─────────────────────────────────────────────────────────

/// Synchronously fetch audio bytes from `url` using Lumen's network stack.
///
/// Supports `http://`, `https://`, and `data:` URLs.
fn fetch_audio_bytes(url: &str) -> Result<Vec<u8>, String> {
    if url.starts_with("data:") {
        return fetch_data_url(url);
    }

    // HTTP(S): parse URL then use lumen-network HttpClient.
    let parsed = lumen_core::url::Url::parse(url).map_err(|e| e.to_string())?;
    let client = lumen_network::HttpClient::new();
    client
        .fetch_subresource(&parsed, lumen_network::RequestDestination::Media)
        .map_err(|e| e.to_string())
}

/// Decode a `data:` URL into raw bytes (strips MIME type, handles base64).
fn fetch_data_url(url: &str) -> Result<Vec<u8>, String> {
    // data:[<mime>][;base64],<data>
    let rest = url.strip_prefix("data:").ok_or("not a data: URL")?;
    let comma = rest.find(',').ok_or("data: URL missing comma")?;
    let meta = &rest[..comma];
    let data = &rest[comma + 1..];
    if meta.ends_with(";base64") {
        use std::io::Read;
        // Use percent-decoded base64 data.
        let cleaned: String = data.chars().filter(|c| !c.is_whitespace()).collect();
        let mut decoder = base64_decode(cleaned.as_bytes());
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).map_err(|e| e.to_string())?;
        Ok(out)
    } else {
        Ok(data.as_bytes().to_vec())
    }
}

/// Minimal base64 decoder (avoids adding a new dependency).
fn base64_decode(input: &[u8]) -> impl std::io::Read + '_ {
    Base64Reader { src: input, pos: 0 }
}

struct Base64Reader<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> std::io::Read for Base64Reader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Simple one-shot decode into buf.
        let table: [i8; 256] = {
            let mut t = [-1i8; 256];
            for (i, &c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
                .iter()
                .enumerate()
            {
                t[c as usize] = i as i8;
            }
            t
        };

        let src = &self.src[self.pos..];
        let mut out = 0usize;
        let mut bits = 0u32;
        let mut nbits = 0u32;

        for &byte in src {
            if byte == b'=' {
                break;
            }
            let v = table[byte as usize];
            if v < 0 {
                continue;
            }
            bits = (bits << 6) | (v as u32);
            nbits += 6;
            if nbits >= 8 {
                nbits -= 8;
                if out >= buf.len() {
                    break;
                }
                buf[out] = ((bits >> nbits) & 0xFF) as u8;
                out += 1;
            }
        }
        self.pos = self.src.len(); // mark consumed
        Ok(out)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::AudioPlaybackProvider;

    #[test]
    fn alloc_handle_unique() {
        let p = PlatformAudioPlayer::new();
        let h1 = p.alloc_handle();
        let h2 = p.alloc_handle();
        assert_ne!(h1, h2);
        p.free_handle(h1);
        p.free_handle(h2);
    }

    #[test]
    fn initial_ready_state_zero() {
        let p = PlatformAudioPlayer::new();
        let h = p.alloc_handle();
        assert_eq!(p.ready_state(h), 0);
        p.free_handle(h);
    }

    #[test]
    fn initial_paused_true() {
        let p = PlatformAudioPlayer::new();
        let h = p.alloc_handle();
        assert!(p.is_paused(h));
        p.free_handle(h);
    }

    #[test]
    fn initial_ended_false() {
        let p = PlatformAudioPlayer::new();
        let h = p.alloc_handle();
        assert!(!p.is_ended(h));
        p.free_handle(h);
    }

    #[test]
    fn initial_duration_nan() {
        let p = PlatformAudioPlayer::new();
        let h = p.alloc_handle();
        assert!(p.duration(h).is_nan());
        p.free_handle(h);
    }

    #[test]
    fn initial_no_error() {
        let p = PlatformAudioPlayer::new();
        let h = p.alloc_handle();
        assert!(!p.has_error(h));
        p.free_handle(h);
    }

    #[test]
    fn free_handle_removes_entry() {
        let p = PlatformAudioPlayer::new();
        let h = p.alloc_handle();
        p.free_handle(h);
        // After free, queries return defaults (not panic).
        assert!(p.is_paused(h));
        assert!(!p.has_error(h));
    }

    #[test]
    fn can_play_type_mp3() {
        let p = PlatformAudioPlayer::new();
        assert_eq!(p.can_play_type("audio/mpeg"), "probably");
    }

    #[test]
    fn can_play_type_unknown() {
        let p = PlatformAudioPlayer::new();
        assert_eq!(p.can_play_type("video/x-custom"), "");
    }

    #[test]
    fn data_url_decode_text() {
        let url = "data:text/plain;base64,SGVsbG8="; // "Hello"
        let bytes = fetch_data_url(url).unwrap();
        assert_eq!(&bytes, b"Hello");
    }

    #[test]
    fn base64_reader_decodes_correctly() {
        use std::io::Read;
        let mut r = base64_decode(b"SGVsbG8="); // Hello
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(&out, b"Hello");
    }
}
