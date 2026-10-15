//! UX-OSPARAMS: system input parameters read from the OS instead of baked-in
//! constants.
//!
//! A hand-written engine inherits none of what a native control gets for free:
//! the double-click interval, the pointer slop of a click and of a drag, the
//! lines a wheel notch scrolls, the caret's width and blink rhythm, the hover
//! delay and the "animations off" switch all live in the OS settings. People
//! with a tremor raise the double-click time, people on a free-spinning wheel
//! lower the line count; a browser that hard-codes 500 ms / 4 px / 40 px
//! silently ignores both.
//!
//! The values live in one process-wide registry ([`current`]), because the
//! consumers sit on different threads (the browser thread handles clicks and
//! the wheel, the render thread the scroll route). On Windows a watcher thread
//! re-reads everything when the OS broadcasts `WM_SETTINGCHANGE` — a *sent*
//! message, so winit's message hook never sees it — and bumps [`generation`].
//! Other platforms keep the defaults (the previous behaviour) until their
//! readers are written.
//!
//! The decisions themselves are pure methods of [`OsParams`], so tests drive
//! them with explicit values and never depend on the machine's settings.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Once, RwLock};

#[cfg(target_os = "windows")]
mod windows;

/// How far one wheel notch scrolls, as the OS setting states it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelScroll {
    /// A notch scrolls this many text lines (`SPI_GETWHEELSCROLLLINES`).
    Lines(u32),
    /// A notch scrolls one page (`WHEEL_PAGESCROLL`).
    Page,
}

/// Snapshot of the OS input parameters. `Copy`, so a consumer takes one with
/// [`current`] and never holds the registry lock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OsParams {
    /// Longest gap between the two presses of a double click, ms
    /// (`GetDoubleClickTime`).
    pub double_click_ms: u32,
    /// Pointer travel allowed between the two presses of a double click, per
    /// axis, CSS px (`SM_CXDOUBLECLK`/`SM_CYDOUBLECLK`).
    pub double_click_slop: (f32, f32),
    /// Pointer travel after which a press becomes a drag, CSS px
    /// (`SM_CXDRAG`/`SM_CYDRAG` at 96 dpi, which is CSS px by definition).
    pub drag_threshold: (f32, f32),
    /// What one wheel notch scrolls (`SPI_GETWHEELSCROLLLINES`).
    pub wheel: WheelScroll,
    /// Text caret width, device px (`SPI_GETCARETWIDTH`).
    pub caret_width_px: f32,
    /// Caret blink period, ms; `None` — the caret does not blink
    /// (`GetCaretBlinkTime` = `INFINITE`).
    pub caret_blink_ms: Option<u32>,
    /// The caret stops blinking and stays solid after this long without input,
    /// ms; `None` — it blinks forever (`SPI_GETCARETTIMEOUT`).
    pub caret_timeout_ms: Option<u32>,
    /// Pointer rest time before hover effects start, ms
    /// (`SPI_GETMOUSEHOVERTIME`).
    pub hover_ms: u32,
    /// The user turned UI animation off (`SPI_GETCLIENTAREAANIMATION` =
    /// false) — the source of `prefers-reduced-motion: reduce`.
    pub reduce_motion: bool,
}

/// Pixels one 3-line wheel notch scrolls: Chromium's 100 px, linear in the
/// configured line count (100/3 px per line).
const WHEEL_PX_PER_THREE_LINES: f32 = 100.0;
/// Share of the viewport a "page" notch scrolls — Chromium's paging fraction,
/// which keeps a little of the previous page in view.
const PAGE_FRACTION: f32 = 0.875;

impl OsParams {
    /// What the browser used before it read the OS: the Windows factory
    /// settings, and the fixed fallback of every platform without a reader.
    pub const DEFAULT: Self = Self {
        double_click_ms: 500,
        double_click_slop: (4.0, 4.0),
        drag_threshold: (4.0, 4.0),
        wheel: WheelScroll::Lines(3),
        caret_width_px: 1.0,
        caret_blink_ms: Some(500),
        caret_timeout_ms: None,
        hover_ms: 400,
        reduce_motion: false,
    };

    /// Clamps values a broken or hostile settings store could make harmful:
    /// a zero interval would make every click a double click, a zero wheel
    /// count a dead wheel, a huge slop one giant drag handle.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        let slop = |v: f32, d: f32| if v.is_finite() && v >= 1.0 { v.min(64.0) } else { d };
        self.double_click_ms = self.double_click_ms.clamp(100, 5_000);
        self.double_click_slop = (
            slop(self.double_click_slop.0, Self::DEFAULT.double_click_slop.0),
            slop(self.double_click_slop.1, Self::DEFAULT.double_click_slop.1),
        );
        self.drag_threshold = (
            slop(self.drag_threshold.0, Self::DEFAULT.drag_threshold.0),
            slop(self.drag_threshold.1, Self::DEFAULT.drag_threshold.1),
        );
        self.wheel = match self.wheel {
            WheelScroll::Lines(0) => Self::DEFAULT.wheel,
            WheelScroll::Lines(n) => WheelScroll::Lines(n.min(100)),
            WheelScroll::Page => WheelScroll::Page,
        };
        self.caret_width_px = if self.caret_width_px.is_finite() {
            self.caret_width_px.clamp(1.0, 20.0)
        } else {
            Self::DEFAULT.caret_width_px
        };
        self.caret_blink_ms = self.caret_blink_ms.map(|v| v.clamp(100, 10_000));
        self.hover_ms = self.hover_ms.clamp(10, 10_000);
        self
    }

    /// Whether a press `gap_ms` after the previous one and `(dx, dy)` CSS px
    /// away from it is the second half of a double click.
    #[must_use]
    pub fn is_double_click(&self, gap_ms: f64, dx: f32, dy: f32) -> bool {
        (0.0..=f64::from(self.double_click_ms)).contains(&gap_ms)
            && dx.abs() <= self.double_click_slop.0
            && dy.abs() <= self.double_click_slop.1
    }

    /// Whether a pointer that moved `(dx, dy)` CSS px since the press has left
    /// the click zone and starts a drag. Radial, like the 4 px circle it
    /// replaces; the radius is the larger of the two axis settings.
    #[must_use]
    pub fn exceeds_drag(&self, dx: f32, dy: f32) -> bool {
        let r = self.drag_threshold.0.max(self.drag_threshold.1);
        dx * dx + dy * dy >= r * r
    }

    /// CSS px one wheel notch scrolls in a viewport `viewport_h` CSS px tall.
    /// Three lines (the Windows default) give Chromium's 100 px; the amount
    /// is linear in the line count.
    #[must_use]
    pub fn wheel_notch_px(&self, viewport_h: f32) -> f32 {
        match self.wheel {
            WheelScroll::Lines(n) => WHEEL_PX_PER_THREE_LINES * n as f32 / 3.0,
            WheelScroll::Page => (viewport_h * PAGE_FRACTION).max(WHEEL_PX_PER_THREE_LINES),
        }
    }
}

impl Default for OsParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

static CURRENT: RwLock<OsParams> = RwLock::new(OsParams::DEFAULT);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static LOADED: Once = Once::new();
/// Set by [`freeze_defaults`]: the registry then ignores the OS entirely.
static FROZEN: AtomicBool = AtomicBool::new(false);

/// Reads the platform's settings once, on first use.
fn ensure_loaded() {
    LOADED.call_once(|| {
        if !FROZEN.load(Ordering::Acquire) {
            publish(read_os());
        }
    });
}

/// The current parameters. Cheap: one read lock and a `Copy`.
#[must_use]
pub fn current() -> OsParams {
    ensure_loaded();
    // A poisoned lock still holds a whole `OsParams` (it is replaced by value),
    // so the data is usable.
    *CURRENT.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Counter bumped whenever a re-read changed something. A consumer that
/// caches a derived value compares it to its own copy to know when to redo it.
#[must_use]
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// Pins the registry to [`OsParams::DEFAULT`] and stops the OS watcher from
/// changing it. Deterministic runs (graphic tests, golden dumps) call this so
/// the machine's accessibility settings cannot move a pixel.
pub fn freeze_defaults() {
    FROZEN.store(true, Ordering::Release);
    publish(OsParams::DEFAULT);
}

/// Stores `next`; returns whether it differs from what was stored.
fn publish(next: OsParams) -> bool {
    let next = next.sanitized();
    let mut cur = CURRENT.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    if *cur == next {
        return false;
    }
    *cur = next;
    GENERATION.fetch_add(1, Ordering::AcqRel);
    true
}

/// Re-reads the OS settings and publishes them. Returns whether anything
/// changed. A no-op while the registry is [frozen](freeze_defaults).
pub fn reload() -> bool {
    ensure_loaded();
    if FROZEN.load(Ordering::Acquire) {
        return false;
    }
    publish(read_os())
}

#[cfg(target_os = "windows")]
fn read_os() -> OsParams {
    windows::read()
}

#[cfg(not(target_os = "windows"))]
fn read_os() -> OsParams {
    OsParams::DEFAULT
}

/// Starts following OS settings changes: whenever a re-read changes the
/// registry, `on_change` runs (on the watcher thread — keep it to waking the
/// app, e.g. `window.request_redraw()`). Idempotent; a no-op off Windows and
/// while frozen.
pub fn start_watcher(on_change: impl Fn() + Send + Sync + 'static) {
    #[cfg(target_os = "windows")]
    if !FROZEN.load(Ordering::Acquire) {
        windows::start_watcher(Box::new(on_change));
    }
    #[cfg(not(target_os = "windows"))]
    drop(on_change);
}

#[cfg(test)]
mod tests;
