//! UX-STATUS-BUBBLE: склейка пузыря адреса ссылки ([`crate::status_bubble`]) с
//! окном — откуда берётся адрес (наведение, Tab), кто будит цикл событий и
//! где пузырь рисуется.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use lumen_layout::TextMeasurer;

use super::Lumen;
use crate::assets::INTER_FONT;
use crate::status_bubble::{build_overlay, layout};
use crate::{links, scrollbar};

/// Размер шрифта, которым меряется пузырь (совпадает с отрисовкой).
const MEASURE_PX: f32 = 12.0;

/// Мерка ширины символа bundled Inter; `None`, если шрифт не разобрался.
fn measurer() -> Option<&'static lumen_paint::FontMeasurer<'static>> {
    static FONT: OnceLock<Option<lumen_font::Font<'static>>> = OnceLock::new();
    static MEASURER: OnceLock<Option<lumen_paint::FontMeasurer<'static>>> = OnceLock::new();
    MEASURER
        .get_or_init(|| {
            let font = FONT.get_or_init(|| lumen_font::Font::parse(INTER_FONT).ok()).as_ref()?;
            lumen_paint::FontMeasurer::new(font).ok()
        })
        .as_ref()
}

impl Lumen {
    fn status_bubble_now_ms(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0
    }

    /// Адрес ссылки под курсором или в фокусе клавиатуры в читаемом виде
    /// (относительный href — абсолютным, LINK-09). Внешний `None` — документ
    /// сейчас занят, состояние пузыря трогать нельзя; внутренний `None` —
    /// ссылки нет (элемент без `href` пузырь не показывает, LINK-11).
    fn status_link_text(&self) -> Option<Option<String>> {
        let focus = self.focus_from_keyboard.then_some(self.focused_node).flatten();
        let Some(node) = self.status_hover_source.or(focus) else { return Some(None) };
        let src = self.layout_source.as_ref()?;
        let href = {
            let doc = src.document.try_lock().ok()?;
            links::find_link_href(&doc, node)
        };
        Some(href.map(|h| lumen_core::url_display::display_url(&self.source.resolve_href(&h)).0))
    }

    /// Пересчитать цель пузыря: после движения мыши (`moved`), Tab и, пока он
    /// жив, на каждом пробуждении — `href` мог смениться скриптом (LINK-11).
    pub(crate) fn status_bubble_sync(&mut self, moved: bool) {
        if self.deterministic.enabled {
            return;
        }
        // Пока открыто контекстное меню, пузырь держится как есть (LINK-11).
        if self.page_context_menu.is_open() {
            return;
        }
        let Some(text) = self.status_link_text() else { return };
        let now = self.status_bubble_now_ms();
        self.status_bubble.set_reduced_motion(crate::platform::os_params::current().reduce_motion);
        self.status_bubble.set_target(text.as_deref(), now, moved);
        let tick = self.status_bubble.tick(now);
        if tick.repaint {
            self.request_redraw();
        }
    }

    /// Пробуждение цикла: продвинуть часы пузыря, вернуть следующий срок.
    pub(crate) fn status_bubble_next_wakeup(&mut self) -> Option<Instant> {
        if self.deterministic.enabled || self.status_bubble.is_idle() {
            return None;
        }
        if self.status_hover_source.is_some() || self.focus_from_keyboard {
            self.status_bubble_sync(false);
        }
        let now = self.status_bubble_now_ms();
        let tick = self.status_bubble.tick(now);
        if tick.repaint {
            self.request_redraw();
        }
        tick.wake.map(|ms| self.epoch + Duration::from_secs_f64((ms / 1000.0).max(0.0)))
    }

    /// Команды отрисовки пузыря в оконных CSS px; `None`, если он не виден.
    pub(crate) fn status_bubble_overlay(
        &mut self,
        pal: &crate::panels::themes::Palette,
    ) -> Option<lumen_paint::DisplayList> {
        let now = self.status_bubble_now_ms();
        let (text, alpha) = {
            let (t, a) = self.status_bubble.visible(now)?;
            (t.to_owned(), a)
        };
        let host = self.chrome_page_host_rect?;
        let m = measurer()?;
        let char_w = |c: char| m.char_width(c, MEASURE_PX);
        let (vw, _, v_present) = self.hbar_layout();
        let inset = if !self.no_scrollbar
            && scrollbar::hscrollbar_visible(self.scroll_x, self.content_width, vw, self.viewport_height_css(), v_present)
        {
            scrollbar::SCROLLBAR_WIDTH
        } else {
            0.0
        };
        let dpr = self.renderer.as_ref().map_or(1.0_f32, |r| r.scale_factor() as f32).max(1e-6);
        let pointer = self.cursor_position.map(|p| (p.x as f32 / dpr, p.y as f32 / dpr));
        let lay = layout(&text, host, inset, pointer, self.status_bubble.min_width(), &char_w);
        self.status_bubble.note_width(lay.rect.width);
        Some(build_overlay(&lay, alpha, pal))
    }
}
