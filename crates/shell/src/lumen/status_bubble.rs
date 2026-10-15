//! UX-STATUS-BUBBLE: glue between the timeline ([`crate::status_bubble`]) and
//! the page — which address the bubble shows (hovered link, else a link focused
//! by the keyboard), how it is fitted and placed, when the loop must wake.

use std::time::Instant;

use super::Lumen;
use crate::platform;
use crate::status_bubble::{avoid_mouse, elide_middle, BubbleBox, Corner, Timing};

/// Font size, horizontal padding (both sides, plus the one border) and height of
/// `.status-bubble` in `assets/chrome/chrome.html`.
const FONT_PX: f32 = 12.0;
const PAD_X: f32 = 21.0;
const HEIGHT: f32 = 23.0;

impl Lumen {
    /// The address to show, formatted for a human (`url_display::display_url`:
    /// decoded path, homoglyph host → punycode), or `None` without a link.
    fn status_bubble_address(&self) -> Option<String> {
        let node = match self.hovered_nid {
            Some(n) => n,
            None if self.focus_from_keyboard => self.focused_node?,
            None => return None,
        };
        let src = self.layout_source.as_ref()?;
        let href = {
            let doc = src.document.lock().unwrap_or_else(|e| e.into_inner());
            crate::links::find_link_href(&doc, node)?
        };
        let resolved = self.source.resolve_href(&href);
        Some(lumen_core::url_display::display_url(&resolved).0)
    }

    /// Distance from the bottom of the window to the bottom of its monitor, CSS px.
    fn status_bubble_room_below(&self) -> f32 {
        let Some(w) = self.window.as_ref() else { return 0.0 };
        let (Some(mon), Ok(pos)) = (w.current_monitor(), w.outer_position()) else { return 0.0 };
        let dpr = w.scale_factor().max(1e-6);
        let win_bottom = f64::from(pos.y) + f64::from(w.outer_size().height);
        let mon_bottom = f64::from(mon.position().y) + f64::from(mon.size().height);
        ((mon_bottom - win_bottom).max(0.0) / dpr) as f32
    }

    /// What `#statusBubble` should say right now.
    fn status_bubble_view(&self, now: Instant) -> lumen_chrome::ChromeStatusBubbleModel {
        let none = lumen_chrome::ChromeStatusBubbleModel::default();
        let (Some(host), Some(measurer)) = (self.chrome_page_host_rect, crate::relayout::chrome_measurer()) else {
            return none;
        };
        if !self.status_bubble.is_drawn(now) {
            return none;
        }
        use lumen_layout::TextMeasurer;
        let char_w = |c: char| measurer.char_width(c, FONT_PX);
        let budget = (host.width * 0.5 - PAD_X).max(0.0);
        let text = elide_middle(self.status_bubble.text(), budget, &char_w);
        let text_w: f32 = text.chars().map(char_w).sum();
        let (vw, _, v_present) = self.hbar_layout();
        let inset = if !self.no_scrollbar
            && crate::scrollbar::hscrollbar_visible(
                self.scroll_x,
                self.content_width,
                vw,
                self.viewport_height_css(),
                v_present,
            ) {
            crate::scrollbar::SCROLLBAR_WIDTH
        } else {
            0.0
        };
        let dpr = self.renderer.as_ref().map_or(1.0_f32, |r| r.scale_factor() as f32).max(1e-6);
        let cursor = self.cursor_position.map(|p| (p.x as f32 / dpr - host.x, p.y as f32 / dpr - host.y));
        let geometry = BubbleBox {
            w: (text_w + PAD_X).min(host.width * 0.5),
            h: HEIGHT,
            host_w: host.width,
            host_h: host.height,
            room_below: self.status_bubble_room_below(),
            bottom_inset: inset,
        };
        let placement = avoid_mouse(&geometry, cursor);
        lumen_chrome::ChromeStatusBubbleModel {
            text,
            opacity: self.status_bubble.opacity(now),
            right: placement.corner == Corner::Right,
            bottom: inset - placement.slide,
        }
    }

    /// Event-loop hook: advances the timeline, rebinds the chrome when the
    /// bubble changed and returns the next instant the loop must wake at.
    pub(crate) fn status_bubble_tick(&mut self) -> Option<Instant> {
        if self.deterministic.enabled {
            return None;
        }
        let now = Instant::now();
        let os = platform::os_params::current();
        self.status_bubble.set_timing(if os.reduce_motion { Timing::REDUCED } else { Timing::STANDARD });
        self.status_bubble.advance(now);
        if self.cursor_position != self.status_bubble_cursor {
            self.status_bubble_cursor = self.cursor_position;
            self.status_bubble.pointer_moved(now);
        }
        // LINK-11: an open link context menu keeps the bubble as it was.
        if !self.page_context_menu.is_open() {
            let target = self.status_bubble_address();
            self.status_bubble.set_target(target.as_deref(), now);
            self.status_bubble.advance(now);
        }
        let view = self.status_bubble_view(now);
        if view != self.status_bubble_pushed {
            self.status_bubble_pushed = view;
            self.relayout_chrome_host();
            self.request_redraw();
        }
        self.status_bubble.next_wakeup(now)
    }
}
