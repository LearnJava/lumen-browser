//! Permanent toolbar strip below the tab bar.
//!
//! `build_toolbar` produces a viewport-locked `DisplayList` for the row.
//! `hit_test` maps CSS-px (x, y) → `ToolbarHit` for mouse dispatch. Mirrors
//! the `tabs::strip` pattern (build/hit_test pair, absolute-window-space
//! coordinates).
//!
//! Scope (DS-9, `docs/tasks/p1-design-v3.md`): navigation cluster (back /
//! forward / reload) on the left, a fixed action cluster on the right (find,
//! web sidebar, AI sidebar, downloads, DevTools, settings).
//!
//! DS-10: the centre hosts the inline omnibox (`address_bar` module) — this
//! module owns its layout (`omnibox_rects`, sized/centred between the two
//! clusters, capped at `OMNIBOX_MAX_W`) and hit-testing, but delegates the
//! actual field/text rendering to `address_bar::build_inline_field` (the
//! suggestion dropdown is a separate overlay the caller draws on top, see
//! `address_bar::build_dropdown`).
//!
//! DS-14: a profile avatar button (26×26 circle — the one chrome element the
//! design system exempts from the squircle-only rule, since a circle is the
//! profile's identity signature) sits first in the left cluster, before nav.
//! Clicking it is `ToolbarHit::Profile`; the caller toggles
//! `panels::profile_menu::ProfileMenuPanel` and renders its dropdown as a
//! separate overlay anchored at `avatar_x()`, mirroring how the omnibox
//! dropdown is layered on top of this module's own field rendering.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

use crate::address_bar;
use crate::panels::themes::Palette;
use crate::tabs::strip::TAB_BAR_HEIGHT;
use crate::theme_tokens::{radius, size};

/// Total CSS-px height of the tab bar + toolbar stack. This is the y-origin
/// of the page content region and of every chrome panel anchored "below the
/// bars" — see `docs/tasks/p1-design-v3.md` DS-9 step 2/3.
pub const CHROME_H: f32 = TAB_BAR_HEIGHT + size::TOOLBAR_H;

/// Side length of a toolbar button in CSS px (`.tb-btn` in the prototype).
const BTN_SZ: f32 = 26.0;

/// Gap between adjacent buttons within a cluster.
const BTN_GAP: f32 = 2.0;

/// Horizontal padding between the window edge and the outermost cluster.
const CLUSTER_PAD: f32 = 10.0;

/// Icon glyph size in CSS px, matching `tabs::strip`'s button icons.
const ICON_SZ: f32 = 12.0;

/// Number of buttons in the right-hand action cluster.
const RIGHT_BTN_COUNT: usize = 6;

/// Max width of the inline omnibox field (design reference:
/// `.omnibox-wrap{ max-width:680px }`).
const OMNIBOX_MAX_W: f32 = 680.0;
/// Height of the omnibox field (`.omnibox{ height:32px }`), centred within
/// the taller `size::TOOLBAR_H` row.
const OMNIBOX_H: f32 = 32.0;
/// Gap between the omnibox field and the nav/action clusters on either side.
const OMNIBOX_GAP: f32 = 8.0;
/// Side length of the lock/star/shield icon-buttons inside the omnibox.
const OMNI_ICON_SZ: f32 = 22.0;
/// Horizontal padding inside the omnibox field, between its border and the
/// lock icon / star+shield cluster.
const OMNI_PAD: f32 = 8.0;
/// Gap between the star and shield icon-buttons.
const OMNI_ICON_GAP: f32 = 2.0;

/// Foreground colour of the avatar's initial-letter glyph — always white
/// regardless of theme or profile colour, matching `.avatar{ color:#fff }`
/// in the design reference (the coloured circle behind it always provides
/// enough contrast, unlike arbitrary chrome text).
const AVATAR_FG: Color = Color { r: 255, g: 255, b: 255, a: 255 };

/// Rendering data for the profile avatar button (DS-14): the active
/// profile's accent colour and the first letter of its display name.
#[derive(Debug, Clone)]
pub struct AvatarBadge {
    /// Single-character (grapheme) label drawn centred on the circle.
    pub letter: String,
    /// Circle fill colour — the active profile's accent.
    pub color: Color,
}

/// A click target within the toolbar row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarHit {
    /// No button under the cursor (still within the toolbar row).
    Empty,
    /// Click on the profile avatar — toggles the profile switcher dropdown.
    Profile,
    /// Navigate back one entry in session history.
    Back,
    /// Navigate forward one entry in session history.
    Forward,
    /// Reload the active tab.
    Reload,
    /// Click on the omnibox field itself (outside the lock/star/shield
    /// icons) — focuses it, same as Ctrl+L/F6.
    Omnibox,
    /// Click on the TLS padlock icon — opens the certificate panel.
    Lock,
    /// Click on the star icon — bookmarks the current page.
    Star,
    /// Click on the shield icon — toggles the shields popover.
    Shield,
    /// Toggle the find-in-page bar.
    Find,
    /// Toggle the web sidebar.
    WebSidebar,
    /// Toggle the AI sidebar.
    AiSidebar,
    /// Toggle the downloads panel.
    Downloads,
    /// Toggle the DevTools console.
    DevTools,
    /// Toggle the settings panel.
    Settings,
}

/// Which right-cluster buttons should render in their "open" (lit) state —
/// mirrors the corresponding panel's `visible` flag.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolbarActive {
    /// `self.find.is_open()`.
    pub find: bool,
    /// `self.sidebar.visible` (web sidebar).
    pub web_sidebar: bool,
    /// `self.ai_panel.visible`.
    pub ai_sidebar: bool,
    /// `self.downloads.visible`.
    pub downloads: bool,
    /// `self.devtools_console.visible`.
    pub devtools: bool,
    /// `self.settings_panel.visible`.
    pub settings: bool,
    /// `self.downloads.active_count() > 0` (DS-19): draws the green dot
    /// indicator on the downloads button (`.tb-dot` in the design
    /// reference), independent of whether the popover itself is open.
    pub downloads_has_active: bool,
}

/// Left edge x-coordinate of the profile avatar button (DS-14) — the
/// leading element of the left cluster, before the nav buttons.
pub fn avatar_x() -> f32 {
    CLUSTER_PAD
}

/// Left edge x-coordinate of each nav-cluster button (back, forward,
/// reload), offset past the avatar button.
fn left_btn_x(idx: usize) -> f32 {
    CLUSTER_PAD + BTN_SZ + BTN_GAP + idx as f32 * (BTN_SZ + BTN_GAP)
}

/// Left edge x-coordinate of the `idx`-th right-cluster button (0 = find,
/// ..= 5 = settings), given the window width.
fn right_btn_x(window_w: f32, idx: usize) -> f32 {
    let cluster_w = RIGHT_BTN_COUNT as f32 * BTN_SZ + (RIGHT_BTN_COUNT - 1) as f32 * BTN_GAP;
    window_w - CLUSTER_PAD - cluster_w + idx as f32 * (BTN_SZ + BTN_GAP)
}

/// Compute the geometry of the inline omnibox (DS-10): the field itself,
/// centred in the space between the nav cluster and the action cluster and
/// capped at `OMNIBOX_MAX_W`, plus the lock/star/shield icon-button sub-rects
/// and the text area between them. Shared by `hit_test` and `build_toolbar`
/// (which delegates the field's own rendering to `address_bar`).
pub fn omnibox_rects(window_w: f32) -> address_bar::FieldRects {
    // Avatar + back/forward/reload = 4 elements.
    let left_cluster_w = 4.0 * BTN_SZ + 3.0 * BTN_GAP;
    let right_cluster_w =
        RIGHT_BTN_COUNT as f32 * BTN_SZ + (RIGHT_BTN_COUNT - 1) as f32 * BTN_GAP;
    let avail_x0 = CLUSTER_PAD + left_cluster_w + OMNIBOX_GAP;
    let avail_x1 = window_w - CLUSTER_PAD - right_cluster_w - OMNIBOX_GAP;
    let avail_w = (avail_x1 - avail_x0).max(0.0);
    let field_w = avail_w.min(OMNIBOX_MAX_W);
    let field_x = avail_x0 + (avail_w - field_w) * 0.5;
    let field_y = TAB_BAR_HEIGHT + (size::TOOLBAR_H - OMNIBOX_H) * 0.5;
    let field = Rect::new(field_x, field_y, field_w, OMNIBOX_H);

    let icon_y = field.y + (OMNIBOX_H - OMNI_ICON_SZ) * 0.5;
    let lock = Rect::new(field.x + OMNI_PAD, icon_y, OMNI_ICON_SZ, OMNI_ICON_SZ);
    let shield = Rect::new(
        field.x + field.width - OMNI_PAD - OMNI_ICON_SZ,
        icon_y,
        OMNI_ICON_SZ,
        OMNI_ICON_SZ,
    );
    let star =
        Rect::new(shield.x - OMNI_ICON_GAP - OMNI_ICON_SZ, icon_y, OMNI_ICON_SZ, OMNI_ICON_SZ);
    let text_x0 = lock.x + lock.width + OMNI_PAD;
    let text_x1 = star.x - OMNI_PAD;
    let text = Rect::new(text_x0, field.y, (text_x1 - text_x0).max(0.0), field.height);

    address_bar::FieldRects { field, lock, text, star, shield }
}

/// Whether CSS-px `(x, y)` falls inside `rect`.
fn rect_contains(rect: Rect, x: f32, y: f32) -> bool {
    (rect.x..rect.x + rect.width).contains(&x) && (rect.y..rect.y + rect.height).contains(&y)
}

/// Hit-test a click at CSS-px `(x, y)` against the toolbar row.
///
/// Returns `ToolbarHit::Empty` if `y` falls outside `TAB_BAR_HEIGHT..CHROME_H`.
pub fn hit_test(x: f32, y: f32, window_w: f32) -> ToolbarHit {
    if !(TAB_BAR_HEIGHT..CHROME_H).contains(&y) {
        return ToolbarHit::Empty;
    }
    let ax = avatar_x();
    if (ax..ax + BTN_SZ).contains(&x) {
        return ToolbarHit::Profile;
    }
    let nav = [ToolbarHit::Back, ToolbarHit::Forward, ToolbarHit::Reload];
    for (i, hit) in nav.into_iter().enumerate() {
        let bx = left_btn_x(i);
        if (bx..bx + BTN_SZ).contains(&x) {
            return hit;
        }
    }
    let right = [
        ToolbarHit::Find,
        ToolbarHit::WebSidebar,
        ToolbarHit::AiSidebar,
        ToolbarHit::Downloads,
        ToolbarHit::DevTools,
        ToolbarHit::Settings,
    ];
    for (i, hit) in right.into_iter().enumerate() {
        let bx = right_btn_x(window_w, i);
        if (bx..bx + BTN_SZ).contains(&x) {
            return hit;
        }
    }
    let omni = omnibox_rects(window_w);
    if rect_contains(omni.lock, x, y) {
        return ToolbarHit::Lock;
    }
    if rect_contains(omni.star, x, y) {
        return ToolbarHit::Star;
    }
    if rect_contains(omni.shield, x, y) {
        return ToolbarHit::Shield;
    }
    if rect_contains(omni.field, x, y) {
        return ToolbarHit::Omnibox;
    }
    ToolbarHit::Empty
}

/// Uniform corner radii helper (mirrors the identically-named private helper
/// in other chrome modules, e.g. `page_context_menu.rs`).
fn corners(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r }
}

/// Diameter of the active-download indicator dot (`.tb-dot`).
const DOT_SZ: f32 = 6.0;
/// Border ring width around the indicator dot (`.tb-dot{ border:1.5px solid … }`).
const DOT_BORDER: f32 = 1.5;

/// Push one button (rounded-rect background + centered glyph) into `out`.
///
/// `dot` draws the small green indicator circle in the button's top-right
/// corner (DS-19: `.tb-dot`, used by the downloads button while any download
/// is active — independent of `active`, which lights the button when its
/// panel is open).
fn push_btn(out: &mut DisplayList, btn_x: f32, glyph: &str, active: bool, pal: &Palette, dot: bool) {
    let bg = if active { pal.item_selected_bg } else { pal.toolbar_bg };
    let icon_color = if active { pal.accent } else { pal.text_dim };
    let btn_y = TAB_BAR_HEIGHT + (size::TOOLBAR_H - BTN_SZ) * 0.5;
    if active {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(btn_x, btn_y, BTN_SZ, BTN_SZ),
            radii: corners(radius::MD),
            color: bg,
        });
    }
    let icon_x = btn_x + (BTN_SZ - ICON_SZ) * 0.5;
    let icon_y = btn_y + (BTN_SZ - ICON_SZ * 1.2) * 0.5;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(icon_x, icon_y, ICON_SZ, ICON_SZ * 1.2),
        text: glyph.to_owned(),
        font_size: ICON_SZ,
        color: icon_color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: None,
    });
    if dot {
        let ring = DOT_SZ + DOT_BORDER * 2.0;
        let ring_x = btn_x + BTN_SZ - ring - 3.0;
        let ring_y = btn_y + 3.0;
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(ring_x, ring_y, ring, ring),
            radii: corners(ring * 0.5),
            color: pal.toolbar_bg,
        });
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(ring_x + DOT_BORDER, ring_y + DOT_BORDER, DOT_SZ, DOT_SZ),
            radii: corners(DOT_SZ * 0.5),
            color: crate::theme_tokens::badge::GREEN,
        });
    }
}

/// Push the profile avatar circle (DS-14): a filled circle in the profile's
/// accent colour with its initial letter centred in white. Unlike
/// [`push_btn`], this is unconditional — the avatar always renders filled,
/// there is no "inactive/transparent" state.
fn push_avatar(out: &mut DisplayList, badge: &AvatarBadge) {
    let x = avatar_x();
    let y = TAB_BAR_HEIGHT + (size::TOOLBAR_H - BTN_SZ) * 0.5;
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(x, y, BTN_SZ, BTN_SZ),
        radii: corners(BTN_SZ * 0.5),
        color: badge.color,
    });
    let letter_w = BTN_SZ * 0.6;
    let letter_x = x + (BTN_SZ - letter_w) * 0.5;
    let letter_y = y + (BTN_SZ - ICON_SZ * 1.2) * 0.5;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(letter_x, letter_y, letter_w, ICON_SZ * 1.2),
        text: badge.letter.clone(),
        font_size: ICON_SZ,
        color: AVATAR_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::BOLD,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: None,
    });
}

/// Build a viewport-locked display list for the toolbar row.
///
/// Renders the bar background + bottom divider, the left navigation cluster
/// (back/forward/reload — always enabled; DoD only requires the buttons call
/// the existing handlers, not disabled-state fidelity), the right action
/// cluster, and the inline omnibox field (DS-10, delegated to
/// `address_bar::build_inline_field`). `active` lights the buttons whose
/// panel is currently open. `address_bar_state`/`current_url` drive the
/// omnibox field: not focused shows `current_url`, focused shows the live
/// editable input.
///
/// Does not draw the suggestion dropdown — see `address_bar::build_dropdown`,
/// called separately by the caller only while the bar is focused. Likewise
/// does not draw the profile switcher dropdown (DS-14) — the caller renders
/// `panels::profile_menu::build_panel` separately while it is visible.
pub fn build_toolbar(
    window_w: f32,
    pal: &Palette,
    active: ToolbarActive,
    address_bar_state: &address_bar::AddressBarState,
    current_url: &str,
    avatar: &AvatarBadge,
) -> DisplayList {
    let mut out = DisplayList::with_capacity(2 + 10 * 2 + 8);
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, TAB_BAR_HEIGHT, window_w, size::TOOLBAR_H),
        color: pal.toolbar_bg,
    });
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, CHROME_H - 1.0, window_w, 1.0),
        color: pal.divider,
    });

    push_avatar(&mut out, avatar);
    push_btn(&mut out, left_btn_x(0), "\u{2190}", false, pal, false); // ← back
    push_btn(&mut out, left_btn_x(1), "\u{2192}", false, pal, false); // → forward
    push_btn(&mut out, left_btn_x(2), "\u{21BB}", false, pal, false); // ↻ reload

    push_btn(&mut out, right_btn_x(window_w, 0), "\u{2315}", active.find, pal, false); // ⌕ find
    push_btn(&mut out, right_btn_x(window_w, 1), "\u{25EB}", active.web_sidebar, pal, false); // ◫ web sidebar
    push_btn(&mut out, right_btn_x(window_w, 2), "\u{2726}", active.ai_sidebar, pal, false); // ✦ AI sidebar
    push_btn(&mut out, right_btn_x(window_w, 3), "\u{2B07}", active.downloads, pal, active.downloads_has_active); // ⬇ downloads
    push_btn(&mut out, right_btn_x(window_w, 4), "\u{2692}", active.devtools, pal, false); // ⚒ DevTools
    push_btn(&mut out, right_btn_x(window_w, 5), "\u{2699}", active.settings, pal, false); // ⚙ settings

    let omni_rects = omnibox_rects(window_w);
    let mut field_cmds =
        address_bar::build_inline_field(address_bar_state, current_url, &omni_rects, pal);
    out.append(&mut field_cmds);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dark() -> Palette {
        Palette::DARK
    }

    fn avatar() -> AvatarBadge {
        AvatarBadge { letter: "K".to_owned(), color: Color { r: 0, g: 102, b: 255, a: 255 } }
    }

    #[test]
    fn hit_test_outside_row_is_empty() {
        assert_eq!(hit_test(20.0, 0.0, 1024.0), ToolbarHit::Empty);
        assert_eq!(hit_test(20.0, CHROME_H + 1.0, 1024.0), ToolbarHit::Empty);
    }

    #[test]
    fn hit_test_avatar() {
        let y = TAB_BAR_HEIGHT + 1.0;
        assert_eq!(hit_test(avatar_x() + 2.0, y, 1024.0), ToolbarHit::Profile);
    }

    #[test]
    fn hit_test_left_cluster() {
        let y = TAB_BAR_HEIGHT + 1.0;
        assert_eq!(hit_test(left_btn_x(0) + 2.0, y, 1024.0), ToolbarHit::Back);
        assert_eq!(hit_test(left_btn_x(1) + 2.0, y, 1024.0), ToolbarHit::Forward);
        assert_eq!(hit_test(left_btn_x(2) + 2.0, y, 1024.0), ToolbarHit::Reload);
    }

    #[test]
    fn hit_test_right_cluster() {
        let y = TAB_BAR_HEIGHT + 1.0;
        let w = 1024.0;
        assert_eq!(hit_test(right_btn_x(w, 0) + 2.0, y, w), ToolbarHit::Find);
        assert_eq!(hit_test(right_btn_x(w, 1) + 2.0, y, w), ToolbarHit::WebSidebar);
        assert_eq!(hit_test(right_btn_x(w, 2) + 2.0, y, w), ToolbarHit::AiSidebar);
        assert_eq!(hit_test(right_btn_x(w, 3) + 2.0, y, w), ToolbarHit::Downloads);
        assert_eq!(hit_test(right_btn_x(w, 4) + 2.0, y, w), ToolbarHit::DevTools);
        assert_eq!(hit_test(right_btn_x(w, 5) + 2.0, y, w), ToolbarHit::Settings);
    }

    #[test]
    fn hit_test_gap_between_buttons_is_empty() {
        let y = TAB_BAR_HEIGHT + 1.0;
        // Just past the back button, inside the 2 px gap before forward.
        assert_eq!(hit_test(left_btn_x(0) + BTN_SZ + 1.0, y, 1024.0), ToolbarHit::Empty);
    }

    #[test]
    fn hit_test_omnibox_icons_and_field() {
        let w = 1024.0;
        let rects = omnibox_rects(w);
        let icon_y = rects.lock.y + 2.0;
        assert_eq!(hit_test(rects.lock.x + 2.0, icon_y, w), ToolbarHit::Lock);
        assert_eq!(hit_test(rects.star.x + 2.0, icon_y, w), ToolbarHit::Star);
        assert_eq!(hit_test(rects.shield.x + 2.0, icon_y, w), ToolbarHit::Shield);
        // Text area sits at the same y-range as the field itself, above the
        // (vertically inset) icon-buttons — pick a y outside their range.
        assert_eq!(hit_test(rects.text.x + 2.0, rects.field.y + 1.0, w), ToolbarHit::Omnibox);
    }

    #[test]
    fn omnibox_field_is_capped_at_max_width() {
        // At 1920px there is plenty of room between the clusters — the field
        // must stay at OMNIBOX_MAX_W, not stretch to fill it.
        let rects = omnibox_rects(1920.0);
        assert!((rects.field.width - OMNIBOX_MAX_W).abs() < 1e-3);
    }

    #[test]
    fn build_toolbar_emits_background_nine_buttons_and_omnibox_field() {
        let bar = address_bar::AddressBarState::default();
        let cmds = build_toolbar(
            1024.0, &dark(), ToolbarActive::default(), &bar, "https://example.com", &avatar(),
        );
        let fill_rects = cmds
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRect { .. }))
            .count();
        let rounded_rects = cmds
            .iter()
            .filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. }))
            .count();
        let texts = cmds.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).count();
        // Background + divider = 2 plain FillRects; no button is "active" so
        // no button-highlight FillRoundedRect is emitted — only the avatar
        // circle (always drawn) + the omnibox field's own border+background
        // (2) = 3. 9 buttons + avatar letter + lock/star/shield + host text
        // = 14 glyphs.
        assert_eq!(fill_rects, 2);
        assert_eq!(rounded_rects, 3);
        assert_eq!(texts, 14);
    }

    #[test]
    fn build_toolbar_active_button_gets_highlight() {
        let active = ToolbarActive { settings: true, ..Default::default() };
        let bar = address_bar::AddressBarState::default();
        let cmds =
            build_toolbar(1024.0, &dark(), active, &bar, "https://example.com", &avatar());
        let highlights =
            cmds.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).count();
        // 1 settings-button highlight + avatar circle + 2 for the
        // (always-drawn) omnibox field border+background.
        assert_eq!(highlights, 4);
    }

    #[test]
    fn chrome_h_is_tab_bar_plus_toolbar() {
        assert!((CHROME_H - (TAB_BAR_HEIGHT + size::TOOLBAR_H)).abs() < 1e-6);
    }

    #[test]
    fn downloads_active_indicator_adds_two_rounded_rects() {
        let bar = address_bar::AddressBarState::default();
        let without = build_toolbar(
            1024.0, &dark(), ToolbarActive::default(), &bar, "https://example.com", &avatar(),
        );
        let active = ToolbarActive { downloads_has_active: true, ..Default::default() };
        let with_dot =
            build_toolbar(1024.0, &dark(), active, &bar, "https://example.com", &avatar());
        let count = |cmds: &DisplayList| {
            cmds.iter().filter(|c| matches!(c, DisplayCommand::FillRoundedRect { .. })).count()
        };
        // The dot is drawn as two rounded rects (border ring + fill), added
        // on top of the button's own (inactive, so undrawn) highlight.
        assert_eq!(count(&with_dot) - count(&without), 2);
    }

    #[test]
    fn downloads_active_indicator_uses_green_badge_colour() {
        let bar = address_bar::AddressBarState::default();
        let active = ToolbarActive { downloads_has_active: true, ..Default::default() };
        let cmds =
            build_toolbar(1024.0, &dark(), active, &bar, "https://example.com", &avatar());
        let has_green = cmds.iter().any(|c| {
            matches!(c, DisplayCommand::FillRoundedRect { color, .. }
                if *color == crate::theme_tokens::badge::GREEN)
        });
        assert!(has_green, "downloads button must show the green active-download dot");
    }
}
