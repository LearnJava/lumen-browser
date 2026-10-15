//! UX-STATUS-BUBBLE: the link-address bubble in the lower corner of the page.
//!
//! Three pure pieces, all taking `now`/geometry as parameters so the rules are
//! unit-tested without a window (docs/research/browser-ux/report.md §4.3,
//! LINK-07…LINK-11):
//!
//! - [`StatusBubble`] — the show/hide timeline: 80 ms before the first
//!   appearance (restarted by every pointer movement, so a fast fly-over does
//!   not flash it), 120 ms fade-in; on leaving the link 250 ms of grace, then a
//!   200 ms fade-out; moving to a neighbouring link swaps the text at once;
//! - [`elide_middle`] — fits the address into the bubble's width: the scheme
//!   and host are never cut, the middle of the path becomes «…», the tail (file
//!   name, extension, query, fragment) stays visible;
//! - [`avoid_mouse`] — keeps the bubble out of the cursor's way: it slides down
//!   when the cursor comes closer than 20 px and jumps to the opposite corner
//!   when there is no room below.
//!
//! The glue that feeds the model from hover/focus and hands the result to the
//! chrome document is `lumen/status_bubble.rs`.

use std::time::{Duration, Instant};

/// The cursor closer than this to the bubble pushes it away (Chromium `kMousePadding`).
pub(crate) const MOUSE_PADDING: f32 = 20.0;

/// Animation frame step while a fade runs.
const FRAME: Duration = Duration::from_millis(16);

/// Timeline lengths; [`Timing::REDUCED`] drops the fades (the OS turned UI
/// animation off) but keeps the delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Timing {
    pub(crate) show_delay: Duration,
    pub(crate) fade_in: Duration,
    pub(crate) hide_delay: Duration,
    pub(crate) fade_out: Duration,
}

impl Timing {
    /// Chromium `status_bubble_views.cc` / Firefox: 80 / 120 / 250 / 200 ms.
    pub(crate) const STANDARD: Self = Self {
        show_delay: Duration::from_millis(80),
        fade_in: Duration::from_millis(120),
        hide_delay: Duration::from_millis(250),
        fade_out: Duration::from_millis(200),
    };
    pub(crate) const REDUCED: Self = Self {
        show_delay: Duration::from_millis(80),
        fade_in: Duration::ZERO,
        hide_delay: Duration::from_millis(250),
        fade_out: Duration::ZERO,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Hidden,
    /// A target appeared; waits `show_delay` after the last pointer movement.
    ShowDelay { since: Instant },
    FadeIn { from: f32, since: Instant },
    Shown,
    /// The target is gone; the bubble holds `level` for `hide_delay`.
    HideDelay { since: Instant, level: f32 },
    FadeOut { from: f32, since: Instant },
}

/// Show/hide timeline of the bubble. `text` is what the bubble currently says;
/// it survives the hide delay and the fade-out and is dropped once hidden.
#[derive(Debug, Clone)]
pub(crate) struct StatusBubble {
    phase: Phase,
    text: String,
    timing: Timing,
}

impl Default for StatusBubble {
    fn default() -> Self {
        Self { phase: Phase::Hidden, text: String::new(), timing: Timing::STANDARD }
    }
}

fn since(now: Instant, t: Instant) -> Duration {
    now.saturating_duration_since(t)
}

/// `elapsed / len` clamped to `0..=1`; a zero-length stage is already over.
fn progress(elapsed: Duration, len: Duration) -> f32 {
    if len.is_zero() {
        1.0
    } else {
        (elapsed.as_secs_f32() / len.as_secs_f32()).clamp(0.0, 1.0)
    }
}

impl StatusBubble {
    /// Switches between the standard timeline and the reduced-motion one.
    pub(crate) fn set_timing(&mut self, timing: Timing) {
        self.timing = timing;
    }

    /// The text to show (empty while hidden).
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Feeds the address under the pointer / on the focused link (`None` — no
    /// link). Call [`Self::advance`] first so a stage that already ended is
    /// not mistaken for the current one.
    pub(crate) fn set_target(&mut self, target: Option<&str>, now: Instant) {
        match target {
            Some(t) => {
                if self.text != t {
                    t.clone_into(&mut self.text);
                }
                self.phase = match self.phase {
                    Phase::Hidden => Phase::ShowDelay { since: now },
                    Phase::HideDelay { level, .. } if level >= 1.0 => Phase::Shown,
                    Phase::HideDelay { level, .. } => Phase::FadeIn { from: level, since: now },
                    Phase::FadeOut { .. } => Phase::FadeIn { from: self.opacity(now), since: now },
                    keep => keep,
                };
            }
            None => {
                self.phase = match self.phase {
                    Phase::ShowDelay { .. } => {
                        self.text.clear();
                        Phase::Hidden
                    }
                    Phase::FadeIn { .. } => Phase::HideDelay { since: now, level: self.opacity(now) },
                    Phase::Shown => Phase::HideDelay { since: now, level: 1.0 },
                    keep => keep,
                };
            }
        }
    }

    /// Any pointer movement restarts the pre-show delay (Firefox model).
    pub(crate) fn pointer_moved(&mut self, now: Instant) {
        if let Phase::ShowDelay { .. } = self.phase {
            self.phase = Phase::ShowDelay { since: now };
        }
    }

    /// Moves the timeline to `now`, crossing every stage boundary on the way.
    pub(crate) fn advance(&mut self, now: Instant) {
        let t = self.timing;
        loop {
            let next = match self.phase {
                Phase::ShowDelay { since: s } if since(now, s) >= t.show_delay => {
                    Phase::FadeIn { from: 0.0, since: s + t.show_delay }
                }
                Phase::FadeIn { since: s, from } if since(now, s) >= t.fade_in || from >= 1.0 => Phase::Shown,
                Phase::HideDelay { since: s, level } if since(now, s) >= t.hide_delay => {
                    Phase::FadeOut { from: level, since: s + t.hide_delay }
                }
                Phase::FadeOut { since: s, .. } if since(now, s) >= t.fade_out => {
                    self.text.clear();
                    Phase::Hidden
                }
                _ => return,
            };
            self.phase = next;
        }
    }

    /// Opacity at `now`, `0.0..=1.0`.
    pub(crate) fn opacity(&self, now: Instant) -> f32 {
        let t = self.timing;
        match self.phase {
            Phase::Hidden | Phase::ShowDelay { .. } => 0.0,
            Phase::Shown => 1.0,
            Phase::FadeIn { from, since: s } => from + (1.0 - from) * progress(since(now, s), t.fade_in),
            Phase::HideDelay { level, .. } => level,
            Phase::FadeOut { from, since: s } => from * (1.0 - progress(since(now, s), t.fade_out)),
        }
    }

    /// Whether the bubble occupies the screen at all (a fully faded one does not).
    pub(crate) fn is_drawn(&self, now: Instant) -> bool {
        !self.text.is_empty() && self.opacity(now) > 0.0
    }

    /// The next instant the timeline needs a look at; `None` while it rests
    /// (hidden, or fully shown).
    pub(crate) fn next_wakeup(&self, now: Instant) -> Option<Instant> {
        let t = self.timing;
        match self.phase {
            Phase::Hidden | Phase::Shown => None,
            Phase::ShowDelay { since: s } => Some(s + t.show_delay),
            Phase::HideDelay { since: s, .. } => Some(s + t.hide_delay),
            Phase::FadeIn { since: s, .. } => Some((now + FRAME).min(s + t.fade_in).max(now)),
            Phase::FadeOut { since: s, .. } => Some((now + FRAME).min(s + t.fade_out).max(now)),
        }
    }
}

/// Fits `url` into `budget` px of text width. `char_w` is the glyph advance.
///
/// The prefix up to the end of the authority (`https://host:port`) is never
/// cut. Of the rest (path, query, fragment) the last path segment is kept
/// whole when it can be — shortened from its start otherwise — and the room
/// left goes to the beginning of the path; the gap becomes «…». Without
/// `scheme://` (a `javascript:` or `mailto:` address) only the scheme and its
/// colon are fixed.
pub(crate) fn elide_middle(url: &str, budget: f32, char_w: &dyn Fn(char) -> f32) -> String {
    let widths: Vec<f32> = url.chars().map(char_w).collect();
    if widths.iter().sum::<f32>() <= budget {
        return url.to_owned();
    }
    let chars: Vec<char> = url.chars().collect();
    let fixed = fixed_prefix_len(url);
    let ellipsis = char_w('…');
    let base: f32 = widths[..fixed].iter().sum::<f32>() + ellipsis;
    let tail = &chars[fixed..];
    let tail_w = &widths[fixed..];
    let n = tail.len();
    // The last segment: after the final `/` that is not the very last char.
    let seg_start = tail[..n.saturating_sub(1)].iter().rposition(|&c| c == '/').map_or(0, |p| p + 1);
    let seg_len = n - seg_start;
    // Width of the kept head `tail[..h]` and kept end `tail[n - t..]`, by prefix sums.
    let mut pre = vec![0.0_f32; n + 1];
    for (i, w) in tail_w.iter().enumerate() {
        pre[i + 1] = pre[i] + w;
    }
    let end_w = |t: usize| pre[n] - pre[n - t];
    // Largest head that still fits next to an end of `t` chars (head and end must not overlap).
    let head_for = |t: usize| -> usize {
        let room = budget - base - end_w(t);
        (0..=n - t).rev().find(|&h| pre[h] <= room).unwrap_or(0)
    };
    let mut best_t = 0;
    let mut best_h = 0;
    for t in (0..=seg_len).rev() {
        let h = head_for(t);
        if h >= 1 || t == 0 {
            best_t = t;
            best_h = h;
            break;
        }
    }
    if best_h + best_t >= n {
        return url.to_owned();
    }
    let mut out: String = chars[..fixed].iter().collect();
    out.extend(&tail[..best_h]);
    out.push('…');
    out.extend(&tail[n - best_t..]);
    out
}

/// Number of leading chars that must never be elided: through the authority
/// (`scheme://host:port`), or through `scheme:` for an authority-less address.
fn fixed_prefix_len(url: &str) -> usize {
    let byte_end = match url.find("://") {
        Some(sep) => {
            let start = sep + 3;
            url[start..].find(['/', '?', '#']).map_or(url.len(), |p| start + p)
        }
        None => url.find(':').map_or(0, |p| p + 1),
    };
    url[..byte_end].chars().count()
}

/// Which lower corner the bubble sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Corner {
    #[default]
    Left,
    Right,
}

/// Where the bubble stands: its corner and how far it has slid down (CSS px).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Placement {
    pub(crate) corner: Corner,
    pub(crate) slide: f32,
}

/// A point/rect pair in the page-host coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BubbleBox {
    /// Width and height of the bubble.
    pub(crate) w: f32,
    pub(crate) h: f32,
    /// Width and height of the area the bubble lives in.
    pub(crate) host_w: f32,
    pub(crate) host_h: f32,
    /// Distance from the bottom of the host to the bottom of the monitor
    /// (how far the bubble can slide down before it leaves the screen).
    pub(crate) room_below: f32,
    /// What the horizontal scrollbar of the page takes at the bottom.
    pub(crate) bottom_inset: f32,
}

/// Chromium `AvoidMouse`: the cursor within [`MOUSE_PADDING`] of the bubble
/// pushes it down by `(20 + y)² / 20`, `y` being the cursor's depth below the
/// bubble's top edge, but by no more than its own height; when the screen has
/// no room below, the bubble crosses to the opposite corner instead.
/// `cursor` is relative to the host's top-left; `None` — no cursor over the page.
pub(crate) fn avoid_mouse(b: &BubbleBox, cursor: Option<(f32, f32)>) -> Placement {
    let Some((cx, cy)) = cursor else { return Placement::default() };
    let top = b.host_h - b.bottom_inset - b.h;
    let near = |left: f32| {
        cx >= left - MOUSE_PADDING
            && cx <= left + b.w + MOUSE_PADDING
            && cy >= top - MOUSE_PADDING
            && cy <= top + b.h + MOUSE_PADDING
    };
    if !near(0.0) {
        return Placement::default();
    }
    let right_left = (b.host_w - b.w).max(0.0);
    if b.room_below < b.h && !near(right_left) {
        return Placement { corner: Corner::Right, slide: 0.0 };
    }
    let y = cy - top;
    let slide = ((MOUSE_PADDING + y).powi(2) / MOUSE_PADDING).min(b.h).min(b.room_below.max(0.0));
    Placement { corner: Corner::Left, slide }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn at(t0: Instant, n: u64) -> Instant {
        t0 + ms(n)
    }

    /// Drives the model to `now` the way the event loop does.
    fn step(b: &mut StatusBubble, target: Option<&str>, now: Instant) {
        b.advance(now);
        b.set_target(target, now);
        b.advance(now);
    }

    #[test]
    fn appears_after_80ms_and_fades_in_over_120() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        step(&mut b, Some("https://a.ru/"), t0);
        assert_eq!(b.opacity(at(t0, 79)), 0.0);
        b.advance(at(t0, 80));
        assert!(b.opacity(at(t0, 80)).abs() < 1e-6);
        assert!((b.opacity(at(t0, 140)) - 0.5).abs() < 1e-3);
        b.advance(at(t0, 200));
        assert_eq!(b.opacity(at(t0, 200)), 1.0);
        assert_eq!(b.next_wakeup(at(t0, 200)), None);
    }

    #[test]
    fn pointer_movement_restarts_the_show_delay() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        step(&mut b, Some("https://a.ru/"), t0);
        b.pointer_moved(at(t0, 60));
        b.advance(at(t0, 100));
        assert!(!b.is_drawn(at(t0, 100)), "40 ms after the last move the bubble must still wait");
        assert_eq!(b.next_wakeup(at(t0, 100)), Some(at(t0, 140)));
        b.advance(at(t0, 200));
        assert!(b.is_drawn(at(t0, 200)));
    }

    #[test]
    fn leaving_the_link_waits_250ms_then_fades_over_200() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        step(&mut b, Some("u"), t0);
        step(&mut b, Some("u"), at(t0, 500));
        assert_eq!(b.opacity(at(t0, 500)), 1.0);
        step(&mut b, None, at(t0, 600));
        assert_eq!(b.opacity(at(t0, 849)), 1.0, "grace period keeps the bubble");
        assert_eq!(b.text(), "u");
        b.advance(at(t0, 950));
        assert!((b.opacity(at(t0, 950)) - 0.5).abs() < 1e-3);
        b.advance(at(t0, 1050));
        assert_eq!(b.opacity(at(t0, 1050)), 0.0);
        assert_eq!(b.text(), "");
        assert!(!b.is_drawn(at(t0, 1050)));
    }

    #[test]
    fn neighbouring_link_swaps_text_without_fade() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        step(&mut b, Some("one"), t0);
        step(&mut b, Some("one"), at(t0, 500));
        step(&mut b, Some("two"), at(t0, 510));
        assert_eq!(b.text(), "two");
        assert_eq!(b.opacity(at(t0, 510)), 1.0);
    }

    #[test]
    fn returning_during_the_grace_period_cancels_the_hide() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        step(&mut b, Some("one"), t0);
        step(&mut b, Some("one"), at(t0, 500));
        step(&mut b, None, at(t0, 600));
        step(&mut b, Some("two"), at(t0, 700));
        assert_eq!(b.next_wakeup(at(t0, 700)), None);
        step(&mut b, Some("two"), at(t0, 2000));
        assert_eq!(b.opacity(at(t0, 2000)), 1.0);
    }

    #[test]
    fn returning_mid_fade_out_resumes_from_the_current_opacity() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        step(&mut b, Some("one"), t0);
        step(&mut b, Some("one"), at(t0, 500));
        step(&mut b, None, at(t0, 600));
        b.advance(at(t0, 950));
        let mid = b.opacity(at(t0, 950));
        step(&mut b, Some("two"), at(t0, 950));
        assert!((b.opacity(at(t0, 950)) - mid).abs() < 1e-3);
        assert!(b.opacity(at(t0, 1000)) > mid);
    }

    #[test]
    fn leaving_before_the_first_show_never_flashes() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        step(&mut b, Some("u"), t0);
        step(&mut b, None, at(t0, 50));
        step(&mut b, None, at(t0, 400));
        assert!(!b.is_drawn(at(t0, 400)));
        assert_eq!(b.next_wakeup(at(t0, 400)), None);
    }

    #[test]
    fn late_tick_crosses_all_stages() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        step(&mut b, Some("u"), t0);
        b.advance(at(t0, 5000));
        assert_eq!(b.opacity(at(t0, 5000)), 1.0);
    }

    #[test]
    fn reduced_motion_has_no_fades() {
        let t0 = Instant::now();
        let mut b = StatusBubble::default();
        b.set_timing(Timing::REDUCED);
        step(&mut b, Some("u"), t0);
        b.advance(at(t0, 80));
        assert_eq!(b.opacity(at(t0, 80)), 1.0);
        step(&mut b, None, at(t0, 100));
        b.advance(at(t0, 350));
        assert_eq!(b.opacity(at(t0, 350)), 0.0);
    }

    fn mono(_: char) -> f32 {
        1.0
    }

    #[test]
    fn short_address_is_untouched() {
        assert_eq!(elide_middle("https://a.ru/x", 40.0, &mono), "https://a.ru/x");
    }

    #[test]
    fn long_path_is_cut_in_the_middle_and_keeps_host_and_file() {
        let url = "https://example.com/aaaa/bbbb/cccc/dddd/report-2026.pdf#page=3";
        let out = elide_middle(url, 40.0, &mono);
        assert!(out.starts_with("https://example.com/"), "{out}");
        assert!(out.ends_with("report-2026.pdf#page=3") || out.ends_with("2026.pdf#page=3"), "{out}");
        assert!(out.contains('…'));
        assert!(out.chars().count() <= 40, "{out}");
    }

    #[test]
    fn host_is_never_cut() {
        let url = "https://very-long-host-name.example.org/a/b/c";
        let out = elide_middle(url, 10.0, &mono);
        assert!(out.starts_with("https://very-long-host-name.example.org"), "{out}");
    }

    #[test]
    fn overlong_last_segment_keeps_its_end() {
        let url = "https://a.ru/dir/file-name-that-is-really-long.tar.gz";
        let out = elide_middle(url, 30.0, &mono);
        assert!(out.ends_with(".tar.gz"), "{out}");
        assert!(out.chars().count() <= 30, "{out}");
        assert!(out.starts_with("https://a.ru/"), "{out}");
    }

    #[test]
    fn authority_less_address_fixes_only_the_scheme() {
        let out = elide_middle("javascript:doSomethingVeryLongAndVerboseHere()", 20.0, &mono);
        assert!(out.starts_with("javascript:"), "{out}");
        assert!(out.ends_with(')'), "{out}");
    }

    #[test]
    fn cyrillic_path_is_cut_on_char_boundaries() {
        let url = "https://сайт.рф/новости/сегодня/очень-длинный-заголовок-статьи.html";
        let out = elide_middle(url, 36.0, &mono);
        assert!(out.contains('…'));
        assert!(out.ends_with(".html"), "{out}");
    }

    fn geometry(room_below: f32) -> BubbleBox {
        BubbleBox { w: 200.0, h: 24.0, host_w: 1000.0, host_h: 600.0, room_below, bottom_inset: 0.0 }
    }

    #[test]
    fn far_cursor_leaves_the_bubble_alone() {
        assert_eq!(avoid_mouse(&geometry(500.0), Some((500.0, 300.0))), Placement::default());
        assert_eq!(avoid_mouse(&geometry(500.0), None), Placement::default());
    }

    #[test]
    fn near_cursor_slides_the_bubble_down_nonlinearly_up_to_its_height() {
        // top = 576. A cursor 10 px above the bubble: y = -10 → (20-10)²/20 = 5.
        let p = avoid_mouse(&geometry(500.0), Some((50.0, 566.0)));
        assert_eq!(p.corner, Corner::Left);
        assert!((p.slide - 5.0).abs() < 1e-3, "{p:?}");
        // Cursor on the bubble: y = 12 → 51 px, capped to the height 24.
        let p = avoid_mouse(&geometry(500.0), Some((50.0, 588.0)));
        assert_eq!(p.slide, 24.0);
    }

    #[test]
    fn no_room_below_sends_the_bubble_to_the_opposite_corner() {
        let p = avoid_mouse(&geometry(0.0), Some((50.0, 588.0)));
        assert_eq!(p, Placement { corner: Corner::Right, slide: 0.0 });
    }

    #[test]
    fn cursor_near_both_corners_slides_within_the_room_there_is() {
        let narrow = BubbleBox { host_w: 220.0, ..geometry(0.0) };
        let p = avoid_mouse(&narrow, Some((110.0, 588.0)));
        assert_eq!(p, Placement { corner: Corner::Left, slide: 0.0 });
    }

    #[test]
    fn scrollbar_inset_lifts_the_bubble_zone() {
        let g = BubbleBox { bottom_inset: 8.0, ..geometry(500.0) };
        // The bubble's top is 568 now; the cursor at 590 is on it.
        assert!(avoid_mouse(&g, Some((50.0, 590.0))).slide > 0.0);
        // The cursor at 560 is 8 px above the top: inside the padding.
        assert!(avoid_mouse(&g, Some((50.0, 560.0))).slide > 0.0);
        assert_eq!(avoid_mouse(&g, Some((50.0, 540.0))), Placement::default());
    }
}
