//! Find-in-page from the shell's side: the find bar's own key handling, the
//! match list for the current query and scrolling the active match into view.
//!
//! The search itself and the match model are `crate::find`; what needs `Lumen`
//! is everything around it - the query lives in the find bar widget, the text
//! being searched is the live document, and moving to a match is a page scroll
//! that must go through the same clamping the wheel does.

use crate::*;

impl Lumen {
    pub(crate) fn handle_find_key(&mut self, code: KeyCode, key_event: &KeyEvent) {
        let shift = self.modifiers.shift_key();
        let ctrl_or_super = self.modifiers.control_key() || self.modifiers.super_key();

        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.find.close();
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.find.backspace();
                self.scroll_to_active_match();
                self.request_redraw();
            }
            // Enter / F3 — следующий матч (Shift — предыдущий).
            // Ctrl+G / Cmd+G — то же (Firefox-стиль find-next), Shift — предыдущий.
            KeyCode::Enter | KeyCode::F3 => {
                if !key_event.repeat {
                    let total = self.current_matches().len();
                    if shift {
                        self.find.prev(total);
                    } else {
                        self.find.next(total);
                    }
                    self.scroll_to_active_match();
                    self.request_redraw();
                }
            }
            KeyCode::KeyG if ctrl_or_super && !key_event.repeat => {
                let total = self.current_matches().len();
                if shift {
                    self.find.prev(total);
                } else {
                    self.find.next(total);
                }
                self.scroll_to_active_match();
                self.request_redraw();
            }
            // Ctrl+R — переключить plain-text ↔ regex режим.
            KeyCode::KeyR if ctrl_or_super && !key_event.repeat => {
                self.find.toggle_regex_mode();
                self.scroll_to_active_match();
                self.request_redraw();
            }
            _ => {
                // Текстовый ввод. При модификаторах Ctrl/Cmd не вставляем —
                // это shortcut в адрес find-а (или будущих чего-то ещё), не
                // символ для query. Без них text — это уже layout-aware
                // символ от winit, с учётом IME / dead-keys.
                if ctrl_or_super {
                    return;
                }
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                {
                    self.find.append_str(text);
                    self.scroll_to_active_match();
                    self.request_redraw();
                }
            }
        }
        // CC-9 (docs/tasks/p1-css-chrome.md): `#findBar`'s engine-rendered
        // value/count (`Self::chrome_model_snapshot`) is baked into
        // `self.chrome_layout` at `relayout_chrome_host` time, not
        // recomputed every `RedrawRequested` — every branch above mutates
        // `self.find`, so without this call the on-screen bar would keep
        // showing stale text/count. Mirrors the same call at the end of
        // `Self::handle_address_bar_key` (CC-7). No-op off the flag.
        self.relayout_chrome_host();
    }

    /// Если активный match вне видимой части viewport-а — сдвигает scroll так,
    /// чтобы он попал в верхнюю четверть окна. Вызывается после любого
    /// действия, меняющего active match: next/prev, backspace, текстовый ввод.
    /// При закрытом баре / пустом query / отсутствии матчей — no-op.
    fn scroll_to_active_match(&mut self) {
        let matches = self.current_matches();
        if matches.is_empty() {
            return;
        }
        let active = self.find.active_index();
        let Some(m) = matches.get(active) else {
            return;
        };
        let vh = self.viewport_height_css();
        if let Some(target) = find::scroll_to_match(m.rect, vh, self.scroll_y) {
            self.start_smooth_scroll(target);
        }
    }

    /// Пересчитывает текущий список совпадений.
    ///
    /// - Plain-text режим: substring search по DrawText-командам display list.
    /// - Regex режим (Ctrl+R): regex по [`TextFragment`][lumen_layout::TextFragment]
    ///   из [`collect_visible_text`][lumen_layout::collect_visible_text]; позиции
    ///   берутся из `TextFragment.rect`, `dl_index` — lookup по (x, y, text) в DL.
    pub(crate) fn current_matches(&self) -> Vec<find::FindMatch> {
        if !self.find.is_open() || self.find.query().is_empty() {
            return Vec::new();
        }
        let Ok(font) = lumen_font::Font::parse(INTER_FONT) else {
            return Vec::new();
        };
        let Ok(measurer) = lumen_paint::FontMeasurer::new(&font) else {
            return Vec::new();
        };
        if self.find.is_regex_mode() {
            let frags = self.layout_box.as_ref().map_or_else(Vec::new, |lb| {
                lumen_layout::collect_visible_text(lb)
            });
            find::find_matches_regex(&frags, &self.display_list, self.find.query(), &measurer)
        } else {
            find::find_matches(&self.display_list, self.find.query(), &measurer)
        }
    }
}
