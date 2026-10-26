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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use lumen_core::ext::VideoDecoder;

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
    /// Playback position in ms at a given real-clock instant, scaled by
    /// `rate` (`video.playbackRate` — GAP-MEDIADECODE срез 18). `rate` only
    /// scales the elapsed portion since `play_epoch_ms`, not `position_ms`
    /// itself: a rate change is expected to call [`Self::freeze`] first (with
    /// the *old* rate) so the accumulated position never gets retroactively
    /// rescaled, exactly like a `seek` re-anchors the epoch instead of
    /// touching past position.
    pub fn current_ms(&self, real_now_ms: u64, rate: f64) -> u64 {
        if let Some(epoch) = self.play_epoch_ms {
            let elapsed = real_now_ms.saturating_sub(epoch) as f64 * rate;
            self.position_ms + (elapsed.max(0.0) as u64)
        } else {
            self.position_ms
        }
    }

    /// Whether playback has naturally ended (finite loop count exhausted).
    pub fn is_ended(&self, real_now_ms: u64, rate: f64) -> bool {
        if self.cycle_ms == 0 || self.loop_count == 0 {
            return false;
        }
        let total = self.cycle_ms.saturating_mul(u64::from(self.loop_count));
        self.current_ms(real_now_ms, rate) >= total
    }

    /// Duration in seconds exposed to JS as `video.duration`.
    pub fn duration_secs(&self) -> f64 {
        if self.loop_count == 0 {
            return f64::INFINITY;
        }
        let ms = self.cycle_ms.saturating_mul(u64::from(self.loop_count));
        ms as f64 / 1000.0
    }

    /// Snapshot `position_ms` to the current playback position (at `rate`)
    /// and clear epoch.
    pub fn freeze(&mut self, real_now_ms: u64, rate: f64) {
        self.position_ms = self.current_ms(real_now_ms, rate);
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
    /// Shared by GIF- and container-backed entries alike (`VideoPlaybackState`
    /// carries no GIF-specific field), so both decode backends reuse the same
    /// `__lumen_video_play`/`pause`/`seek`/`current_time`/… natives.
    pub playback: Mutex<HashMap<u32, VideoPlaybackState>>,
    /// Pending GIF load requests: `(nid, src_url)` queued by `__lumen_video_load`.
    /// Drained by the shell's tick loop.
    pub pending_loads: Mutex<Vec<(u32, String)>>,
    /// Pending media-container load requests: `(nid, src_url)` queued by
    /// `__lumen_video_container_load` (registered only when a [`VideoDecoder`]
    /// is installed, see [`set_video_decoder`]). Drained by the shell's tick
    /// loop (`tick_video_sessions`), which opens the container through that
    /// decoder.
    pub pending_container_loads: Mutex<Vec<(u32, String)>>,
    /// `<video>` nodes attached to a `MediaSource` (COMPAT-MSE): `nid → source id`.
    /// Set by `__lumen_mse_attach` when `<video src=blob:MediaSource>` starts
    /// loading, removed by `__lumen_mse_detach`. Unlike a container load there are
    /// no bytes to fetch — the frames arrive through `appendBuffer` into the shared
    /// `lumen_media_mse` registry — so the shell's `tick_video_sessions` reconciles
    /// this map with its sessions: it creates `playback` once the buffers hold
    /// tracks and opens the session once they hold video frames.
    pub mse_attached: Mutex<HashMap<u32, u64>>,
    /// Nodes whose media-container load failed (fetch error, undecodable
    /// container, or the first frame could not be produced) — GAP-MEDIADECODE
    /// срез 9. Without this, a corrupted `src` never reaches `playback` and
    /// the shim's poll loop (`__lumen_video_ready`) spins forever with no
    /// `error` event, exactly the same shape a never-decoded GIF had before
    /// this slice. Cleared when a fresh load for the same node is dequeued,
    /// so a `load()` retry after fixing the source is not haunted by the
    /// previous attempt's failure.
    pub load_failures: Mutex<HashMap<u32, String>>,
    /// `(volume, muted)` last set via JS `video.volume =`/`video.muted =`,
    /// keyed by node — GAP-MEDIADECODE remainder: routes those two JS-side
    /// properties (`crates/js/src/shim/video_element.js`'s `_volume`/`_muted`
    /// closures) to the video audio sink. Kept as its own map rather than a
    /// field on `VideoPlaybackState` because that struct is wholesale
    /// reconstructed on every load/decode completion and on tab-switch
    /// restore (`page_snapshot.rs`) — a field there would silently reset to
    /// its default the moment a fresh decode finishes, discarding whatever
    /// the page had already set. Absent entry = spec default (`volume: 1.0`,
    /// `muted: false`).
    pub audio_levels: Mutex<HashMap<u32, (f32, bool)>>,
    /// `video.playbackRate` last set via JS, keyed by node — GAP-MEDIADECODE
    /// срез 18. Same reasoning as `audio_levels`: kept out of
    /// `VideoPlaybackState` because that struct is wholesale reconstructed on
    /// every decode completion and tab-switch restore. Absent entry = spec
    /// default (`1.0`). Only scales the native `currentTime` timer (the
    /// `current_ms`/`is_ended`/`freeze` elapsed-time math); it does not touch
    /// decode/PCM speed — that would need resampling and is out of scope
    /// here (see the remainder note on GAP-MEDIADECODE срез 17).
    pub playback_rates: Mutex<HashMap<u32, f64>>,
    /// Set by the JS natives that change `playback` (`play`/`pause`/`seek`/
    /// `playbackRate`), taken by the shell's `on_about_to_wait`. Frames of a
    /// playing video are paced by a timer that `tick_video_sessions` arms only
    /// from inside `RedrawRequested`, and it stops arming at the end of the
    /// media; without this wake an `autoplay` start or a `loop` restart (both
    /// issued from JS timers) waits for an unrelated redraw — mouse move,
    /// scroll — and the video freezes on one frame meanwhile.
    pub playback_changed: AtomicBool,
    /// Last `(now_ms, Instant)` pair seen by those natives. `play_epoch_ms` is
    /// in the page's `performance.now()` time, whose zero is the document's
    /// time origin, not the shell's start; the shell reads `playback` through
    /// [`Self::js_now_ms`] so both sides use one clock. With the shell's own
    /// clock a video ran ahead of `currentTime` by the time between window
    /// start and navigation — on a page opened minutes later `is_ended` held
    /// from the first tick and the video never left its first frame.
    pub js_clock: Mutex<Option<(u64, std::time::Instant)>>,
}

impl VideoGifStore {
    /// Whether a GIF- or container-backed `<video>` load is queued and not yet
    /// drained — GAP-MEDIADECODE срез 10: this is the exact condition whose
    /// silence let a resource-selection load sit in `pending_loads`/
    /// `pending_container_loads` forever when nothing else happened to trigger a
    /// repaint (`on_about_to_wait`, `crates/shell/src/app/about_to_wait.rs`,
    /// needs a repaint request whenever this is `true`). Pulled out as its
    /// own predicate so the shell-side timer/redraw fix has a unit test —
    /// `about_to_wait.rs` itself needs a real `ActiveEventLoop` and has no
    /// unit-test precedent, but this check touches only `self`.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn has_pending_video_load(&self) -> bool {
        !self.pending_loads.lock().unwrap().is_empty()
            || !self.pending_container_loads.lock().unwrap().is_empty()
    }

    /// Record that JS changed a video's playback state at page time `now_ms`
    /// (see `playback_changed` and `js_clock`).
    pub fn note_playback_change(&self, now_ms: f64) {
        *self.js_clock.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((now_ms.max(0.0) as u64, std::time::Instant::now()));
        self.playback_changed.store(true, Ordering::Release);
    }

    /// The page's `performance.now()` in milliseconds, extrapolated from the
    /// last sample; `None` before any video started playing.
    pub fn js_now_ms(&self) -> Option<u64> {
        self.js_clock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .map(|(ms, at)| ms + at.elapsed().as_millis() as u64)
    }

    /// Whether playback changed since the last call — the shell then needs one
    /// redraw so `tick_video_sessions` picks the change up and re-arms its timer.
    pub fn take_playback_change(&self) -> bool {
        self.playback_changed.swap(false, Ordering::AcqRel)
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

// ── Video decoder registry ────────────────────────────────────────────────────

static DECODER: OnceLock<RwLock<Option<Arc<dyn VideoDecoder>>>> = OnceLock::new();

fn decoder_lock() -> &'static RwLock<Option<Arc<dyn VideoDecoder>>> {
    DECODER.get_or_init(|| RwLock::new(None))
}

/// Install the shell's container decoder (UX-VIDEO-NATIVE: `lumen-native`).
///
/// Must be called once before any JS context is created: `canPlayType` asks
/// it what it can open, and `__lumen_video_container_load` is registered only
/// when one is installed.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn set_video_decoder(d: Arc<dyn VideoDecoder>) {
    *decoder_lock().write().unwrap() = Some(d);
}

/// Return a clone of the installed decoder, or `None` in headless/CI mode.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn get_video_decoder() -> Option<Arc<dyn VideoDecoder>> {
    decoder_lock().read().unwrap().clone()
}

/// Drop the installed decoder — test-only: the registry is process-global.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) fn clear_video_decoder() {
    *decoder_lock().write().unwrap() = None;
}

/// Serializes tests that depend on the process-global decoder being absent or
/// being a specific one.
#[cfg(test)]
pub(crate) static DECODER_TEST_LOCK: Mutex<()> = Mutex::new(());

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
    fn has_pending_video_load_true_for_container_queue() {
        let store = VideoGifStore::default();
        store
            .pending_container_loads
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

    #[test]
    fn take_playback_change_reports_each_change_once() {
        let store = VideoGifStore::default();
        assert!(!store.take_playback_change(), "fresh store has no change");
        store.note_playback_change(10.0);
        store.note_playback_change(20.0);
        assert!(store.take_playback_change(), "a noted change wakes the shell");
        assert!(!store.take_playback_change(), "taken once, then cleared");
    }

    /// The shell reads playback in page time: the last sample plus the time
    /// passed since it, not the shell's own start-relative clock.
    #[test]
    fn js_now_ms_continues_from_the_last_page_time_sample() {
        let store = VideoGifStore::default();
        assert_eq!(store.js_now_ms(), None, "no sample before any playback change");
        store.note_playback_change(5_000.0);
        let now = store.js_now_ms().expect("sample recorded");
        assert!((5_000..5_000 + 1_000).contains(&now), "page time continues from 5000, got {now}");
    }
}
