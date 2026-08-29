//! Окружение стилевого прохода: thread-local контексты (`cq*`-единицы, метрики
//! `ch`/`ex`, интерактивное состояние `:hover`/`:focus`/`:active`), флаги
//! forced-colors и print, снимок окружения для rayon-воркеров
//! ([`StyleEnvSnapshot`]), `@scope`-предикат `node_in_scope` и сборка
//! `MediaContext` из вьюпорта и флага тёмной темы.
//!
//! Перенесено батчем SPLIT-ST10 из `crates/engine/layout/src/style.rs`
//! (анкер `thread_local! { CONTAINER_CQ }`) без правок тел: изменена только
//! видимость тех items, которые продолжают звать `style.rs` и его тесты.

use std::cell::Cell;

use lumen_core::geom::Size;
use lumen_css_parser::MediaContext;
use lumen_dom::{Document, NodeId};

use crate::style::{matches_complex, SHADOW_SHEETS};

thread_local! {
    /// CSS Container Queries L1 §6.2 — nearest container dimensions for `cq*` unit resolution.
    /// Set by `set_cq_context` before re-laying out container children; cleared afterwards.
    /// Tuple: (inline_size_px, block_size_px). Block size is 0.0 when not queryable
    /// (container-type: inline-size only exposes inline axis).
    ///
    /// **ADR-008 Invariant 3 note (layout-pure-audit 10D.1):**
    /// This thread-local violates the requirement that `layout()` be a pure function
    /// (depends on hidden state not in function signature).
    /// Refactor to explicit `cq_context: Option<(f32, f32)>` parameter:
    /// see STATUS-P1.md Wave 1 for scheduled refactor (~3-4h, affects 20+ call sites).
    /// Current phase 0 risk: low (container queries Phase 2+, not in simple test pages).
    // SPLIT-ST9: видимость расширена до `crate::style`, потому что `cq*`-единицы
    // резолвит `Length::resolve`, уехавшая в `style::values::length`.
    pub(in crate::style) static CONTAINER_CQ: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
}

/// Sets the nearest-container size for `cq*` unit resolution during the container re-layout pass.
/// `height` is `None` when the container is `inline-size` type (block axis not queryable).
pub fn set_cq_context(width: f32, height: Option<f32>) {
    CONTAINER_CQ.with(|c| c.set(Some((width, height.unwrap_or(0.0)))));
}

/// Clears the `cq*` context after the container re-layout pass completes.
pub fn clear_cq_context() {
    CONTAINER_CQ.with(|c| c.set(None));
}

/// Whether a `cq*` resolution context is currently installed (BUG-802).
///
/// A `cq*` length resolves against this hidden thread-local rather than against
/// anything in a box's own style, so two otherwise identical layouts of the same
/// node can legitimately produce different sizes across a container re-layout
/// pass. Anything that remembers a measured size across calls must refuse to do
/// so while this is `true` — see `box_tree`'s column-probe height memo.
pub fn cq_context_active() -> bool {
    CONTAINER_CQ.with(|c| c.get()).is_some()
}

thread_local! {
    /// CSS Values L4 §5.1.1 — absolute px value of one `ch` and one `ex` unit for
    /// the box currently being laid out: `(char_width('0'), x_height)` measured at
    /// the box's own used `font-size`. `lay_out_inner` pushes this from the active
    /// [`crate::TextMeasurer`] before resolving the box's lengths and restores the
    /// parent's value on exit (RAII, so it is always balanced across recursion).
    /// `None` outside a layout pass (or when no measurer is available) — then
    /// `Length::{Ch,Ex}` fall back to the spec default of `0.5em` (§5.1.1: assume
    /// the "0" glyph is `0.5em` wide and the x-height is `0.5em` when the real
    /// metric is impractical to obtain).
    // SPLIT-ST9: та же причина, что у `CONTAINER_CQ` — `ch`/`ex` резолвит
    // `Length::resolve` из `style::values::length`.
    pub(in crate::style) static FONT_CH_EX: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
}

/// Installs the `ch`/`ex` metric context (absolute px per unit) for the box being
/// laid out and returns the previous value so the caller can restore it (RAII).
/// `None` clears the context, making `Length::{Ch,Ex}` use the `0.5em` fallback.
pub fn push_ch_ex_context(ch_ex: Option<(f32, f32)>) -> Option<(f32, f32)> {
    FONT_CH_EX.with(|c| c.replace(ch_ex))
}

/// Restores the `ch`/`ex` metric context to a value previously returned by
/// [`push_ch_ex_context`], undoing the box's contribution once its subtree is done.
pub fn pop_ch_ex_context(prev: Option<(f32, f32)>) {
    FONT_CH_EX.with(|c| c.set(prev));
}

thread_local! {
    /// Raw NodeId.0 of the currently-hovered element, or `u32::MAX` if none.
    /// Set by `set_interactive_state` before layout; cleared with `clear_interactive_state`.
    /// `:hover` matches the hovered element and all its ancestors (CSS Selectors L4 §4.3).
    pub(in crate::style) static HOVER_NID:  Cell<u32> = const { Cell::new(u32::MAX) };
    /// Raw NodeId.0 of the keyboard-focused element, or `u32::MAX` if none.
    /// `:focus` matches exactly; `:focus-within` matches the element and its ancestors.
    pub(in crate::style) static FOCUS_NID:  Cell<u32> = const { Cell::new(u32::MAX) };
    /// Raw NodeId.0 of the mouse-pressed element, or `u32::MAX` if none.
    /// `:active` matches the active element and all its ancestors (CSS Selectors L4 §4.5).
    pub(in crate::style) static ACTIVE_NID: Cell<u32> = const { Cell::new(u32::MAX) };
}

/// Sets the interactive hover/focus/active state for the next layout pass.
///
/// Call this before `layout_measured` / `layout_measured_hyp` on the layout thread.
/// The thread-locals are read by `matches_pseudo_class` for `:hover`, `:focus`,
/// `:active`, `:focus-within`, and `:focus-visible`.
///
/// Call `clear_interactive_state()` after layout to reset the thread-locals.
pub fn set_interactive_state(
    hover: Option<NodeId>,
    focus: Option<NodeId>,
    active: Option<NodeId>,
) {
    HOVER_NID .with(|h| h.set(hover .map(|n| n.index() as u32).unwrap_or(u32::MAX)));
    FOCUS_NID .with(|f| f.set(focus .map(|n| n.index() as u32).unwrap_or(u32::MAX)));
    ACTIVE_NID.with(|a| a.set(active.map(|n| n.index() as u32).unwrap_or(u32::MAX)));
}

/// Clears hover/focus/active state after layout.
pub fn clear_interactive_state() {
    set_interactive_state(None, None, None);
}

thread_local! {
    /// CSS Color Adjustment L1 §3 — Forced Colors Mode active flag.
    /// Set by the shell via [`set_forced_colors`] from the user's accessibility
    /// preference before a layout pass on this thread. Read by `compute_style`
    /// (system-palette forcing post-pass) and by `media_context_from_viewport`
    /// (the `(forced-colors: active)` media feature).
    pub(in crate::style) static FORCED_COLORS: Cell<bool> = const { Cell::new(false) };
}

/// Enables/disables Forced Colors Mode (CSS Color Adjustment L1 §3) for all
/// subsequent layout passes on the current thread.
///
/// Call before `layout_measured` / `layout_measured_hyp` on the layout thread.
/// The flag is sticky (a UA-wide user preference, not per-pass state), so there
/// is no paired `clear_*` — call with `false` to disable.
pub fn set_forced_colors(active: bool) {
    FORCED_COLORS.with(|f| f.set(active));
}

/// True when Forced Colors Mode is active on the current thread.
pub fn forced_colors_active() -> bool {
    FORCED_COLORS.with(|f| f.get())
}

thread_local! {
    /// Media Queries L4 §2.3 — `print` media type active for the current layout
    /// pass. Set by the shell via [`set_print_media`] before laying out for PDF
    /// output; read by `media_context_from_viewport` so the cascade filters
    /// `@media print` / `@media screen` blocks the same way a print-to-PDF
    /// rendering pipeline should (BUG-270). Defaults to `false` (screen).
    static PRINT_MEDIA: Cell<bool> = const { Cell::new(false) };
}

/// Selects the `print` (`true`) or `screen` (`false`) `@media` type for all
/// subsequent layout passes on the current thread (Media Queries L4 §2.3).
///
/// Call before `layout_measured` / `layout_measured_hyp` on the layout thread.
/// Unlike `set_forced_colors`, this is per-pass state (the screen pipeline is
/// the default), so the shell resets it to `false` after a print pass.
pub fn set_print_media(active: bool) {
    PRINT_MEDIA.with(|p| p.set(active));
}

/// True when the current layout pass renders for `print` media.
pub fn print_media_active() -> bool {
    PRINT_MEDIA.with(|p| p.get())
}

// ─── Parallel style environment (ADR-016 M4.1) ───────────────────────────────

/// Snapshot of all style-pass thread-locals needed for rayon worker threads.
///
/// `compute_style` reads several thread-locals set by the shell before a layout
/// pass (interactive state, forced colors, shadow sheets, etc.). rayon worker
/// threads start with each thread-local at its default value, which would
/// produce incorrect styles for `:hover`/`:focus`/`:active`, shadow DOM, and
/// forced-colors pages.
///
/// Capture a `StyleEnvSnapshot` on the layout thread immediately before
/// spawning parallel work, then call [`StyleEnvSnapshot::install`] at the top
/// of every rayon closure. This restores the correct state on each worker
/// thread.
///
/// It deliberately does **not** touch the per-thread rule-index cache. It used
/// to drop it, because that cache was keyed by the sheet's address; keyed by
/// [`Stylesheet::revision`] it is safe to keep, and keeping it is the point —
/// a full pass fans out repeatedly, and dropping the index on every install
/// made each worker rebuild it every time (33 rebuilds per full pass, BUG-341
/// S21).
///
/// Shadow sheets are cloned from the current thread into the snapshot; for
/// documents without shadow DOM this is a cheap empty-map clone.
#[derive(Clone)]
pub struct StyleEnvSnapshot {
    hover_nid: u32,
    focus_nid: u32,
    active_nid: u32,
    forced_colors: bool,
    print_media: bool,
    shadow_sheets: std::collections::HashMap<NodeId, lumen_css_parser::Stylesheet>,
}

impl StyleEnvSnapshot {
    /// Capture the current thread's style environment.
    pub fn capture() -> Self {
        StyleEnvSnapshot {
            hover_nid:     HOVER_NID.with(Cell::get),
            focus_nid:     FOCUS_NID.with(Cell::get),
            active_nid:    ACTIVE_NID.with(Cell::get),
            forced_colors: FORCED_COLORS.with(Cell::get),
            print_media:   PRINT_MEDIA.with(Cell::get),
            shadow_sheets: SHADOW_SHEETS.with(|m| m.borrow().clone()),
        }
    }

    /// Install this snapshot on the **current** (worker) thread.
    pub fn install(&self) {
        HOVER_NID.with(|h| h.set(self.hover_nid));
        FOCUS_NID.with(|f| f.set(self.focus_nid));
        ACTIVE_NID.with(|a| a.set(self.active_nid));
        FORCED_COLORS.with(|f| f.set(self.forced_colors));
        PRINT_MEDIA.with(|p| p.set(self.print_media));
        SHADOW_SHEETS.with(|m| *m.borrow_mut() = self.shadow_sheets.clone());
    }
}

/// CSS Cascade L6 §3 — donut scoping. Returns `true` when `node` is inside the
/// scope rooted at `root_sel_str` and bounded (below, in the tree) by the
/// optional `limit_sel_str` (`@scope (<root>) to (<limit>)`).
///
/// An element is *in scope* iff it is an inclusive descendant of a scoping-root
/// element and it is **not** an inclusive descendant of a scoping limit that
/// lies *within that same root subtree* (the donut hole, §3.2). Root and limit
/// are resolved in a single ancestor walk so the **nearest** boundary wins: a
/// limit-matching element *above* the scope root does not remove `node` from
/// scope (walking up, the root is reached first).
///
/// Empty `root_sel_str` (`@scope { … }` without an explicit `(<root>)`) →
/// implicit scope = document root: every element is in scope unless it sits
/// under a limit.
pub(in crate::style) fn node_in_scope(
    doc: &Document,
    node: NodeId,
    root_sel_str: &str,
    limit_sel_str: Option<&str>,
) -> bool {
    let root_empty = root_sel_str.trim().is_empty();
    let root_selectors = if root_empty {
        Vec::new()
    } else {
        lumen_css_parser::parse_selector_list(root_sel_str)
    };
    if !root_empty && root_selectors.is_empty() {
        return false;
    }
    let limit_selectors = limit_sel_str
        .filter(|s| !s.trim().is_empty())
        .map(lumen_css_parser::parse_selector_list)
        .unwrap_or_default();
    // Walk `node` and its ancestors. The first boundary encountered decides:
    // a limit (inclusive) → out of scope; a root (inclusive) → in scope.
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n == doc.root() { break; }
        for complex in &limit_selectors {
            if matches_complex(complex, doc, n) {
                return false;
            }
        }
        for complex in &root_selectors {
            if matches_complex(complex, doc, n) {
                return true;
            }
        }
        cur = doc.get(n).parent;
    }
    // No explicit root: implicit document-root scope (only limits cut off).
    root_empty
}

/// Контекст для `@media`-запросов из viewport-а и флага тёмной темы.
/// `dark_mode` отражает OS-предпочтение `prefers-color-scheme: dark`,
/// прокинутое shell-ом через `layout_measured_hyp`.
pub(in crate::style) fn media_context_from_viewport(viewport: Size, dark_mode: bool) -> MediaContext {
    // hover/pointer берут desktop-дефолты (мышь) из `MediaContext::default()`.
    // media_type = "print" во время печати в PDF (BUG-270), иначе "screen".
    MediaContext {
        media_type: if print_media_active() { "print".into() } else { "screen".into() },
        width: viewport.width,
        height: viewport.height,
        prefers_dark: dark_mode,
        prefers_reduced_motion: false,
        forced_colors: forced_colors_active(),
        ..Default::default()
    }
}
