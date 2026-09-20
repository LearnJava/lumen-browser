//! Shared state for `<video>` elements backed by animated GIF files.
//!
//! The shell creates a [`VideoGifStore`], installs it globally with
//! [`set_video_gif_store`], and shares the same `Arc` with the JS native
//! bindings.  JS bindings read/write the playback state; the shell's render
//! tick advances frames and uploads them to the GPU.
//!
//! # Design
//!
//! Two kinds of shared state:
//!
//! 1. **Pending loads** (`pending_loads`): URLs queued by JS `__lumen_video_load`.
//!    The shell drains this on each tick, fetches + decodes the GIF, and inserts
//!    a [`VideoPlaybackState`] entry.  No `lumen_image` dependency needed here.
//!
//! 2. **Playback state** (`playback`): per-node timing info (paused, position,
//!    play epoch).  The shell also reads this to compute frame indices; JS reads
//!    it to expose `currentTime`/`duration`/`paused` to the page.
//!
//! # Threading
//!
//! QuickJS and the shell render loop both run on the same OS thread, so the
//! inner `Mutex`es never block.  They are present purely for `Sync`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

// ── Playback state ────────────────────────────────────────────────────────────

/// Per-`<video>` playback timing, stored by the shell after a GIF is decoded.
///
/// Does not contain the GIF pixel data — those live in the shell's own
/// `video_gif_entries: HashMap<u32, VideoGifShellEntry>`.  Only timing/control
/// values are stored here so JS native bindings can read them without pulling
/// in `lumen_image`.
pub struct VideoPlaybackState {
    /// `true` while playback is suspended (initial state / after `pause()`).
    pub paused: bool,
    /// Playback position (milliseconds) as of `play_epoch_ms`.
    pub position_ms: u64,
    /// Real-clock ms when the last `play()` call was made.
    /// `None` when paused or before the first `play()`.
    pub play_epoch_ms: Option<u64>,
    /// Total duration of one animation cycle in ms (sum of frame delays).
    /// Filled by the shell after decoding; 0 until ready.
    pub cycle_ms: u64,
    /// Number of loop iterations (0 = infinite per shell convention).
    pub loop_count: u32,
    /// Intrinsic width in pixels; 0 until decoded.
    pub width: u32,
    /// Intrinsic height in pixels; 0 until decoded.
    pub height: u32,
}

impl VideoPlaybackState {
    /// Playback position in ms at a given real-clock instant.
    pub fn current_ms(&self, real_now_ms: u64) -> u64 {
        if let Some(epoch) = self.play_epoch_ms {
            self.position_ms + real_now_ms.saturating_sub(epoch)
        } else {
            self.position_ms
        }
    }

    /// Whether playback has naturally ended (finite loop count exhausted).
    pub fn is_ended(&self, real_now_ms: u64) -> bool {
        if self.cycle_ms == 0 || self.loop_count == 0 {
            return false;
        }
        let total = self.cycle_ms.saturating_mul(u64::from(self.loop_count));
        self.current_ms(real_now_ms) >= total
    }

    /// Duration in seconds exposed to JS as `video.duration`.
    pub fn duration_secs(&self) -> f64 {
        if self.loop_count == 0 {
            return f64::INFINITY;
        }
        let ms = self.cycle_ms.saturating_mul(u64::from(self.loop_count));
        ms as f64 / 1000.0
    }

    /// Snapshot `position_ms` to the current playback position and clear epoch.
    pub fn freeze(&mut self, real_now_ms: u64) {
        self.position_ms = self.current_ms(real_now_ms);
        self.play_epoch_ms = None;
    }
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// Shared state for all `<video>`-element GIF animations, keyed by DOM node index.
///
/// Created by the shell; shared with JS native bindings via [`set_video_gif_store`].
#[derive(Default)]
pub struct VideoGifStore {
    /// Per-node playback timing.  Key = DOM node index (`el.__nid__`).
    /// Populated by the shell after decoding; read by JS native bindings.
    /// Shared by GIF- and FFmpeg-backed entries alike (`VideoPlaybackState`
    /// carries no GIF-specific field), so both decode backends reuse the same
    /// `__lumen_video_play`/`pause`/`seek`/`current_time`/… natives.
    pub playback: Mutex<HashMap<u32, VideoPlaybackState>>,
    /// Pending GIF load requests: `(nid, src_url)` queued by `__lumen_video_load`.
    /// Drained by the shell's tick loop.
    pub pending_loads: Mutex<Vec<(u32, String)>>,
    /// Pending FFmpeg-container load requests: `(nid, src_url)` queued by
    /// `__lumen_video_ffmpeg_load` (GAP-MEDIADECODE срез 6, feature
    /// `ffmpeg-video`). Drained by the shell's tick loop once it links
    /// `lumen-media-ffmpeg` (срез 7, not yet wired).
    pub pending_ffmpeg_loads: Mutex<Vec<(u32, String)>>,
    /// Nodes whose FFmpeg-container load failed (fetch error, undecodable
    /// container, or the first frame could not be produced) — GAP-MEDIADECODE
    /// срез 9. Without this, a corrupted `src` never reaches `playback` and
    /// the shim's poll loop (`__lumen_video_ready`) spins forever with no
    /// `error` event, exactly the same shape a never-decoded GIF had before
    /// this slice. Cleared when a fresh load for the same node is dequeued,
    /// so a `load()` retry after fixing the source is not haunted by the
    /// previous attempt's failure.
    pub load_failures: Mutex<HashMap<u32, String>>,
}

impl VideoGifStore {
    /// Whether a GIF- or FFmpeg-backed `<video>` load is queued and not yet
    /// drained — GAP-MEDIADECODE срез 10: this is the exact condition whose
    /// silence let a resource-selection load sit in `pending_loads`/
    /// `pending_ffmpeg_loads` forever when nothing else happened to trigger a
    /// repaint (`on_about_to_wait`, `crates/shell/src/app/about_to_wait.rs`,
    /// needs a repaint request whenever this is `true`). Pulled out as its
    /// own predicate so the shell-side timer/redraw fix has a unit test —
    /// `about_to_wait.rs` itself needs a real `ActiveEventLoop` and has no
    /// unit-test precedent, but this check touches only `self`.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn has_pending_video_load(&self) -> bool {
        !self.pending_loads.lock().unwrap().is_empty()
            || !self.pending_ffmpeg_loads.lock().unwrap().is_empty()
    }
}

// ── Global registry ───────────────────────────────────────────────────────────

static STORE: OnceLock<RwLock<Option<Arc<VideoGifStore>>>> = OnceLock::new();

fn store_lock() -> &'static RwLock<Option<Arc<VideoGifStore>>> {
    STORE.get_or_init(|| RwLock::new(None))
}

/// Install the video GIF store from the shell.
///
/// Must be called once before any JS context is created.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn set_video_gif_store(s: Arc<VideoGifStore>) {
    *store_lock().write().unwrap() = Some(s);
}

/// Return a clone of the installed store, or `None` in headless/CI mode.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn get_video_gif_store() -> Option<Arc<VideoGifStore>> {
    store_lock().read().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::VideoGifStore;

    #[test]
    fn has_pending_video_load_false_when_both_queues_empty() {
        let store = VideoGifStore::default();
        assert!(!store.has_pending_video_load());
    }

    #[test]
    fn has_pending_video_load_true_for_gif_queue() {
        let store = VideoGifStore::default();
        store.pending_loads.lock().unwrap().push((1, "a.gif".to_string()));
        assert!(store.has_pending_video_load());
    }

    #[test]
    fn has_pending_video_load_true_for_ffmpeg_queue() {
        let store = VideoGifStore::default();
        store
            .pending_ffmpeg_loads
            .lock()
            .unwrap()
            .push((1, "a.mp4".to_string()));
        assert!(store.has_pending_video_load());
    }

    #[test]
    fn has_pending_video_load_false_after_drain() {
        let store = VideoGifStore::default();
        store.pending_loads.lock().unwrap().push((1, "a.gif".to_string()));
        store.pending_loads.lock().unwrap().clear();
        assert!(!store.has_pending_video_load());
    }
}
