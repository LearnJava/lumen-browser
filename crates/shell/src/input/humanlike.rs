//! Human-like input simulation for `InputMode::HumanLike` (§9E).
//!
//! Wraps [`InputSender`] and injects realistic timing and motion:
//!
//! * **Mouse paths** — cubic Bézier curves with randomised control points,
//!   sampled at configurable step count.  Intermediate positions are sent as
//!   [`InputCommand::MouseMove`] events; only the final position fires a
//!   [`InputCommand::Click`].
//!
//! * **Inter-keystroke delay** — Gaussian-distributed pause between each
//!   character, modelling real typing rhythm.
//!
//! * **Pre-click dwell** — brief pause after the cursor reaches the target
//!   before the click fires, mimicking human hesitation.
//!
//! All blocking sleeps happen on the **caller's thread** — the shell event
//! loop is never stalled.  Designed for BrowserSession automation and MCP.
//!
//! # Randomness
//!
//! Uses a self-contained Xorshift-64 PRNG seeded at construction.  No
//! external RNG dependency required.  Use [`HumanLikeSender::with_seed`] for
//! deterministic replay in tests.

// Public API (HumanLikeSender, HumanLikeConfig, InputMode) is consumed by
// BrowserSession / MCP callers, not the shell binary.
#![allow(dead_code)]

use std::time::Duration;
use std::thread;
use super::InputSender;

// ── PRNG (Xorshift-64) ────────────────────────────────────────────────────────

/// Xorshift-64 state.  Must never be zero.
struct Rng(u64);

impl Rng {
    /// Seed from a raw u64.  Replaces zero seed with a non-zero fallback.
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed })
    }

    /// Next pseudo-random u64.
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform float in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Gaussian sample with given `mean` and `sigma` (Box-Muller transform).
    ///
    /// The result is clamped to `[mean - 3*sigma, mean + 3*sigma]` to keep
    /// delay values positive and bounded.
    fn gaussian(&mut self, mean: f64, sigma: f64) -> f64 {
        // Box-Muller: avoid log(0) by clamping u1 away from zero.
        let u1 = self.next_f64().max(1e-15);
        let u2 = self.next_f64();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        let raw = mean + sigma * z;
        // Clamp to ±3σ so delay is always in a sane range.
        raw.clamp(mean - 3.0 * sigma, mean + 3.0 * sigma)
    }
}

// ── Bézier path ───────────────────────────────────────────────────────────────

/// Evaluates a cubic Bézier curve at parameter `t` ∈ [0, 1].
///
/// P(t) = (1-t)³·p0 + 3(1-t)²t·p1 + 3(1-t)t²·p2 + t³·p3
fn bezier(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let u2 = u * u;
    let u3 = u2 * u;
    let t2 = t * t;
    let t3 = t2 * t;
    (
        u3 * p0.0 + 3.0 * u2 * t * p1.0 + 3.0 * u * t2 * p2.0 + t3 * p3.0,
        u3 * p0.1 + 3.0 * u2 * t * p1.1 + 3.0 * u * t2 * p2.1 + t3 * p3.1,
    )
}

/// Generate `steps` waypoints along a randomised cubic Bézier from `start` to
/// `end`.
///
/// Control points are offset perpendicular to the straight path by a random
/// amount proportional to the path length, giving natural arc-shaped motion.
fn bezier_path(
    start: (f64, f64),
    end: (f64, f64),
    steps: u32,
    rng: &mut Rng,
) -> Vec<(f32, f32)> {
    if steps == 0 {
        return vec![];
    }

    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);

    // Perpendicular unit vector.
    let perp = (-dy / len, dx / len);

    // Offset magnitude: up to ±30 % of path length, random sign.
    let scale = len * 0.30;
    let off1 = (rng.next_f64() * 2.0 - 1.0) * scale;
    let off2 = (rng.next_f64() * 2.0 - 1.0) * scale;

    // Control points placed at 1/3 and 2/3 along the path plus perpendicular
    // offset for a natural arc.
    let p1 = (start.0 + dx / 3.0 + perp.0 * off1, start.1 + dy / 3.0 + perp.1 * off1);
    let p2 = (start.0 + 2.0 * dx / 3.0 + perp.0 * off2, start.1 + 2.0 * dy / 3.0 + perp.1 * off2);

    (1..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let (x, y) = bezier(start, p1, p2, end, t);
            (x as f32, y as f32)
        })
        .collect()
}

// ── Configuration ─────────────────────────────────────────────────────────────

/// Timing and motion parameters for [`HumanLikeSender`].
#[derive(Debug, Clone)]
pub struct HumanLikeConfig {
    /// Number of intermediate [`MouseMove`](super::InputCommand::MouseMove)
    /// waypoints sampled along the Bézier path.
    pub mouse_steps: u32,

    /// Mean delay between consecutive mouse-move waypoints, in milliseconds.
    pub mouse_step_ms: f64,

    /// Mean inter-keystroke delay when typing, in milliseconds.
    pub key_mean_ms: f64,

    /// Standard deviation of the inter-keystroke delay (Gaussian), in ms.
    ///
    /// Higher values produce more irregular typing rhythm.
    pub key_sigma_ms: f64,

    /// Dwell time the cursor hovers at the target before the click fires,
    /// in milliseconds.
    pub click_dwell_ms: f64,
}

impl Default for HumanLikeConfig {
    fn default() -> Self {
        Self {
            mouse_steps: 20,
            mouse_step_ms: 8.0,
            key_mean_ms: 60.0,
            key_sigma_ms: 20.0,
            click_dwell_ms: 40.0,
        }
    }
}

// ── InputMode ─────────────────────────────────────────────────────────────────

/// Controls how injected inputs are delivered to the shell.
///
/// * `Direct` — commands are sent immediately with no added delay (default).
/// * `HumanLike` — commands are expanded into timed sequences using
///   [`HumanLikeSender`].
#[derive(Debug, Clone, Default)]
pub enum InputMode {
    /// Immediate, instantaneous injection.  No timing, no Bézier paths.
    #[default]
    Direct,

    /// Human-like timing: Bézier mouse paths, Gaussian keystroke delays,
    /// pre-click dwell.  Uses the supplied configuration.
    HumanLike(HumanLikeConfig),
}

// ── HumanLikeSender ───────────────────────────────────────────────────────────

/// Wraps [`InputSender`] and injects human-like timing and mouse motion.
///
/// Blocking: each method sleeps on the **caller's thread** for the duration
/// of the action.  The shell event loop is not stalled.
///
/// # Example
///
/// ```rust,ignore
/// let (tx, _rx) = lumen_shell::input::channel();
/// let mut human = HumanLikeSender::new(tx, HumanLikeConfig::default());
/// human.click_at(640.0, 360.0);       // arcs to target, dwells, clicks
/// human.type_text("hello world");     // types with Gaussian keystroke gaps
/// ```
pub struct HumanLikeSender {
    inner: InputSender,
    config: HumanLikeConfig,
    /// Last known cursor position (CSS-pixel, viewport-relative).
    cursor_x: f64,
    cursor_y: f64,
    rng: Rng,
}

impl HumanLikeSender {
    /// Create a new sender wrapping `inner` with default configuration.
    ///
    /// The initial cursor position is `(0, 0)`.  The PRNG is seeded from the
    /// current system time for non-deterministic behaviour.
    pub fn new(inner: InputSender, config: HumanLikeConfig) -> Self {
        // Simple time-based seed without extra deps.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs() << 32))
            .unwrap_or(0xDEAD_BEEF_CAFE_1234);
        Self { inner, config, cursor_x: 0.0, cursor_y: 0.0, rng: Rng::new(seed) }
    }

    /// Create a sender with a fixed PRNG seed for deterministic replay.
    pub fn with_seed(inner: InputSender, config: HumanLikeConfig, seed: u64) -> Self {
        Self { inner, config, cursor_x: 0.0, cursor_y: 0.0, rng: Rng::new(seed) }
    }

    /// Move the cursor along a Bézier arc to `(x, y)`, then dwell, then click.
    ///
    /// Sends intermediate [`MouseMove`](super::InputCommand::MouseMove) commands
    /// with `mouse_step_ms` pauses between them, then sleeps `click_dwell_ms`,
    /// then sends a [`Click`](super::InputCommand::Click).
    pub fn click_at(&mut self, x: f32, y: f32) {
        let start = (self.cursor_x, self.cursor_y);
        let end = (x as f64, y as f64);

        let waypoints =
            bezier_path(start, end, self.config.mouse_steps, &mut self.rng);

        for &(wx, wy) in &waypoints {
            self.inner.mouse_move(wx, wy);
            let delay_ms = self.config.mouse_step_ms.max(0.0);
            if delay_ms > 0.0 {
                thread::sleep(Duration::from_millis(delay_ms as u64));
            }
        }

        // Dwell at target before clicking.
        let dwell_ms = self.config.click_dwell_ms.max(0.0);
        if dwell_ms > 0.0 {
            thread::sleep(Duration::from_millis(dwell_ms as u64));
        }

        self.inner.click(x, y);
        self.cursor_x = end.0;
        self.cursor_y = end.1;
    }

    /// Type `text` with Gaussian-distributed inter-keystroke delays.
    ///
    /// Each character is sent as a separate
    /// [`TypeText`](super::InputCommand::TypeText) containing that single
    /// code-point, with a Gaussian-sampled pause after each character (except
    /// the last).
    pub fn type_text(&mut self, text: &str) {
        let chars: Vec<char> = text.chars().collect();
        for (i, ch) in chars.iter().enumerate() {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.inner.type_text(s);

            if i + 1 < chars.len() {
                let delay_ms = self
                    .rng
                    .gaussian(self.config.key_mean_ms, self.config.key_sigma_ms)
                    .max(0.0);
                if delay_ms > 0.0 {
                    thread::sleep(Duration::from_millis(delay_ms as u64));
                }
            }
        }
    }

    /// Scroll to `(x, y)` immediately (no path animation for scrolls).
    pub fn scroll_to(&mut self, x: f32, y: f32) {
        self.inner.scroll(x, y);
    }

    /// Override the assumed cursor starting position without moving it.
    ///
    /// Useful when the caller knows the current OS cursor position and wants
    /// accurate path lengths.
    pub fn set_cursor_position(&mut self, x: f32, y: f32) {
        self.cursor_x = x as f64;
        self.cursor_y = y as f64;
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::channel;

    // ── Rng ──────────────────────────────────────────────────────────────────

    #[test]
    fn rng_non_zero_seed() {
        let mut rng = Rng::new(0);
        // Must produce non-zero values even when seeded with 0.
        let v = rng.next_u64();
        assert_ne!(v, 0);
    }

    #[test]
    fn rng_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn rng_f64_range() {
        let mut rng = Rng::new(12345);
        for _ in 0..1000 {
            let v = rng.next_f64();
            assert!((0.0..1.0).contains(&v), "f64 out of [0,1): {v}");
        }
    }

    #[test]
    fn gaussian_mean_approx() {
        let mut rng = Rng::new(99);
        let n = 10_000;
        let mean = 60.0_f64;
        let sigma = 20.0_f64;
        let sum: f64 = (0..n).map(|_| rng.gaussian(mean, sigma)).sum();
        let observed = sum / n as f64;
        // Allow ±5 % of mean.
        assert!((observed - mean).abs() < mean * 0.05, "mean diverged: {observed}");
    }

    #[test]
    fn gaussian_clamped_non_negative() {
        let mut rng = Rng::new(7);
        let mean = 10.0_f64;
        let sigma = 5.0_f64;
        for _ in 0..10_000 {
            let v = rng.gaussian(mean, sigma);
            // Clamped to mean ± 3σ = [-5, 25].  But negative values should
            // be caught by the max(0.0) guard in callers, not here.
            assert!(v >= mean - 3.0 * sigma, "below clamp: {v}");
            assert!(v <= mean + 3.0 * sigma, "above clamp: {v}");
        }
    }

    // ── Bézier ───────────────────────────────────────────────────────────────

    #[test]
    fn bezier_endpoints() {
        let p0 = (0.0, 0.0);
        let p1 = (10.0, 50.0);
        let p2 = (90.0, 50.0);
        let p3 = (100.0, 0.0);
        let start = bezier(p0, p1, p2, p3, 0.0);
        let end = bezier(p0, p1, p2, p3, 1.0);
        assert!((start.0 - 0.0).abs() < 1e-9);
        assert!((end.0 - 100.0).abs() < 1e-9);
    }

    #[test]
    fn bezier_path_step_count() {
        let mut rng = Rng::new(1);
        let pts = bezier_path((0.0, 0.0), (100.0, 100.0), 15, &mut rng);
        assert_eq!(pts.len(), 15);
    }

    #[test]
    fn bezier_path_ends_near_target() {
        let mut rng = Rng::new(2);
        let target = (200.0_f64, 150.0_f64);
        let pts = bezier_path((0.0, 0.0), target, 30, &mut rng);
        let last = pts.last().unwrap();
        assert!((last.0 as f64 - target.0).abs() < 1.0);
        assert!((last.1 as f64 - target.1).abs() < 1.0);
    }

    #[test]
    fn bezier_path_zero_steps_empty() {
        let mut rng = Rng::new(3);
        let pts = bezier_path((0.0, 0.0), (100.0, 100.0), 0, &mut rng);
        assert!(pts.is_empty());
    }

    // ── HumanLikeSender ──────────────────────────────────────────────────────

    fn make_sender() -> (HumanLikeSender, crate::input::InputReceiver) {
        let (tx, rx) = channel();
        let cfg = HumanLikeConfig {
            mouse_steps: 5,
            mouse_step_ms: 0.0, // no sleep in tests
            key_mean_ms: 0.0,
            key_sigma_ms: 0.0,
            click_dwell_ms: 0.0,
        };
        let sender = HumanLikeSender::with_seed(tx, cfg, 42);
        (sender, rx)
    }

    #[test]
    fn click_at_produces_moves_then_click() {
        let (mut human, rx) = make_sender();
        human.click_at(100.0, 200.0);
        let cmds = rx.drain();
        // 5 waypoints + 1 click = 6 commands.
        assert_eq!(cmds.len(), 6, "expected 5 moves + 1 click, got {}", cmds.len());
        // Last command must be Click at (100, 200).
        match cmds.last().unwrap() {
            crate::input::InputCommand::Click { x, y } => {
                assert!((x - 100.0).abs() < 0.5, "click x wrong: {x}");
                assert!((y - 200.0).abs() < 0.5, "click y wrong: {y}");
            }
            other => panic!("last command should be Click, got {other:?}"),
        }
        // First 5 must be MouseMove.
        for cmd in &cmds[..5] {
            assert!(
                matches!(cmd, crate::input::InputCommand::MouseMove { .. }),
                "expected MouseMove, got {cmd:?}"
            );
        }
    }

    #[test]
    fn click_at_updates_cursor() {
        let (mut human, _rx) = make_sender();
        human.click_at(300.0, 400.0);
        assert!((human.cursor_x - 300.0).abs() < 1.0);
        assert!((human.cursor_y - 400.0).abs() < 1.0);
    }

    #[test]
    fn type_text_one_command_per_char() {
        let (mut human, rx) = make_sender();
        human.type_text("abc");
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 3);
        for (i, cmd) in cmds.iter().enumerate() {
            match cmd {
                crate::input::InputCommand::TypeText { text } => {
                    assert_eq!(text.chars().count(), 1, "char {i} should be single code-point");
                }
                other => panic!("expected TypeText, got {other:?}"),
            }
        }
    }

    #[test]
    fn type_text_empty_sends_nothing() {
        let (mut human, rx) = make_sender();
        human.type_text("");
        assert!(rx.drain().is_empty());
    }

    #[test]
    fn scroll_to_sends_scroll() {
        let (mut human, rx) = make_sender();
        human.scroll_to(0.0, 500.0);
        let cmds = rx.drain();
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], crate::input::InputCommand::Scroll { .. }));
    }

    #[test]
    fn set_cursor_position_affects_path_start() {
        // Two senders with identical seed: one starts at (0,0), the other at
        // (50,50).  Their Bézier paths to the same target should differ.
        let (tx1, rx1) = channel();
        let (tx2, rx2) = channel();
        let cfg = HumanLikeConfig {
            mouse_steps: 5,
            mouse_step_ms: 0.0,
            key_mean_ms: 0.0,
            key_sigma_ms: 0.0,
            click_dwell_ms: 0.0,
        };
        let mut s1 = HumanLikeSender::with_seed(tx1, cfg.clone(), 99);
        let mut s2 = HumanLikeSender::with_seed(tx2, cfg, 99);
        s2.set_cursor_position(50.0, 50.0);

        s1.click_at(200.0, 200.0);
        s2.click_at(200.0, 200.0);

        let c1 = rx1.drain();
        let c2 = rx2.drain();

        // Both produce 5 moves + 1 click.
        assert_eq!(c1.len(), 6);
        assert_eq!(c2.len(), 6);

        // At least one intermediate position must differ.
        let different = c1.iter().zip(c2.iter()).any(|(a, b)| match (a, b) {
            (
                crate::input::InputCommand::MouseMove { x: x1, y: y1 },
                crate::input::InputCommand::MouseMove { x: x2, y: y2 },
            ) => (x1 - x2).abs() > 0.01 || (y1 - y2).abs() > 0.01,
            _ => false,
        });
        assert!(different, "paths starting at different origins should differ");
    }
}
