//! Where a link click goes when a modifier key or the middle button is used
//! (UX-LINK-CLICKS).
//!
//! A plain left click navigates the current tab — that path lives in
//! `click_activate`. Here: Ctrl/⌘ or the middle button → a lazy background tab
//! right of the current one; with Shift added → the same tab in the foreground;
//! Shift alone → a new window; Alt → nothing (no downloads yet).

use crate::*;

/// Bit layout of the `mod_flags` byte passed to `_lumen_dispatch_mouse_event`.
pub(crate) const MOD_CTRL: u8 = 1;
pub(crate) const MOD_SHIFT: u8 = 1 << 1;
pub(crate) const MOD_ALT: u8 = 1 << 2;
pub(crate) const MOD_SUPER: u8 = 1 << 3;

/// Destination of a link activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkOpen {
    /// Ordinary navigation of the current tab (target/rel rules apply).
    Here,
    /// New tab right of the current one, current tab stays in front.
    BackgroundTab,
    /// New tab right of the current one, switched to at once.
    ForegroundTab,
    /// New OS window.
    Window,
    /// Swallowed: Alt+click.
    Nothing,
}

impl LinkOpen {
    /// Decide from the modifier flags (see `MOD_*`) and whether the middle
    /// button was used. ⌘ counts as Ctrl on macOS only.
    pub(crate) fn from_click(mods: u8, middle: bool) -> Self {
        let ctrl = mods & MOD_CTRL != 0 || (cfg!(target_os = "macos") && mods & MOD_SUPER != 0);
        let shift = mods & MOD_SHIFT != 0;
        if mods & MOD_ALT != 0 {
            return Self::Nothing;
        }
        match (ctrl || middle, shift) {
            (true, true) => Self::ForegroundTab,
            (true, false) => Self::BackgroundTab,
            (false, true) => Self::Window,
            (false, false) => Self::Here,
        }
    }
}

impl Lumen {
    /// Modifier flags as `_lumen_dispatch_mouse_event` takes them.
    pub(crate) fn modifier_flags(&self) -> u8 {
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        flag(self.modifiers.control_key(), MOD_CTRL)
            | flag(self.modifiers.shift_key(), MOD_SHIFT)
            | flag(self.modifiers.alt_key(), MOD_ALT)
            | flag(self.modifiers.super_key(), MOD_SUPER)
    }

    /// [`Self::place_link`] behind the gates an ordinary navigation passes:
    /// CSP `navigate-to` and the cross-browser handoff rule. For callers that
    /// have not run them yet (the middle button).
    pub(crate) fn open_link_with(
        &mut self,
        how: LinkOpen,
        href: &str,
        csp_gate: Option<&(Vec<lumen_network::csp::CspPolicy>, String)>,
    ) {
        if matches!(how, LinkOpen::Here | LinkOpen::Nothing) || !links::is_navigable_href(href) {
            return;
        }
        if self.navigate_to_link_blocked(csp_gate, href) || self.link_handed_off(href) {
            return;
        }
        self.place_link(how, href, csp_gate);
    }

    /// Opens the link `href` of the current document as `how` says. `Here`
    /// is not handled (the caller navigates).
    pub(crate) fn place_link(
        &mut self,
        how: LinkOpen,
        href: &str,
        csp_gate: Option<&(Vec<lumen_network::csp::CspPolicy>, String)>,
    ) {
        if !links::is_navigable_href(href) {
            return;
        }
        let resolved = self.resolve_and_upgrade_href(csp_gate, href);
        // `about:newtab?…` / `about:handoff?…` are in-place actions of internal
        // pages, not documents a tab or window could load.
        if resolved.starts_with("about:") {
            return;
        }
        match how {
            LinkOpen::BackgroundTab => self.open_link_in_tab(&resolved, false),
            LinkOpen::ForegroundTab => self.open_link_in_tab(&resolved, true),
            LinkOpen::Window => self.open_link_in_window(&resolved),
            LinkOpen::Here | LinkOpen::Nothing => {}
        }
    }

    /// Middle button pressed over the page: a link under the cursor opens in a
    /// background tab (foreground with Shift). Links inside frames are left to
    /// a later slice.
    pub(crate) fn open_link_under_middle_click(&mut self) {
        if self.page_dialog.is_some() || self.tab_context_menu.is_open() || self.page_context_menu.is_open() {
            return;
        }
        let Some(cursor) = self.cursor_position else { return };
        let dpr = self.renderer.as_ref().map_or(1.0_f32, |r| r.scale_factor() as f32).max(1e-6);
        let (x_css, y_css) = ((cursor.x as f32) / dpr, (cursor.y as f32) / dpr);
        if self.point_over_chrome(x_css, y_css) {
            return;
        }
        let target = self.pointer_target(x_css, y_css);
        let Some(hit) = target.page.filter(|_| target.frame.is_none()) else { return };
        let link = self.layout_source.as_ref().and_then(|src| {
            let doc = src.document.lock().ok()?;
            let root = doc.root();
            links::find_link(&doc, hit.source_node)
                .map(|(_, href)| (href, crate::csp_enforce::document_csp_policy(&doc, root)))
        });
        let Some((href, csp_gate)) = link else { return };
        let how = LinkOpen::from_click(self.modifier_flags(), true);
        self.open_link_with(how, &href, csp_gate.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_click_stays_here() {
        assert_eq!(LinkOpen::from_click(0, false), LinkOpen::Here);
    }

    #[test]
    fn ctrl_click_and_middle_open_background_tab() {
        assert_eq!(LinkOpen::from_click(MOD_CTRL, false), LinkOpen::BackgroundTab);
        assert_eq!(LinkOpen::from_click(0, true), LinkOpen::BackgroundTab);
    }

    #[test]
    fn shift_adds_foreground() {
        assert_eq!(LinkOpen::from_click(MOD_CTRL | MOD_SHIFT, false), LinkOpen::ForegroundTab);
        assert_eq!(LinkOpen::from_click(MOD_SHIFT, true), LinkOpen::ForegroundTab);
    }

    #[test]
    fn shift_click_opens_window() {
        assert_eq!(LinkOpen::from_click(MOD_SHIFT, false), LinkOpen::Window);
    }

    #[test]
    fn alt_swallows_the_click() {
        assert_eq!(LinkOpen::from_click(MOD_ALT, false), LinkOpen::Nothing);
        assert_eq!(LinkOpen::from_click(MOD_ALT | MOD_CTRL, false), LinkOpen::Nothing);
        assert_eq!(LinkOpen::from_click(MOD_ALT, true), LinkOpen::Nothing);
    }

    #[test]
    fn super_alone_is_not_ctrl_off_macos() {
        let want = if cfg!(target_os = "macos") { LinkOpen::BackgroundTab } else { LinkOpen::Here };
        assert_eq!(LinkOpen::from_click(MOD_SUPER, false), want);
    }
}
