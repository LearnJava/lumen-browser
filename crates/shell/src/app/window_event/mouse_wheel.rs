//! Ветка `WindowEvent::MouseWheel` цикла событий (SPLIT-SH2).
//!
//! Тело ветки вынесено из `Lumen::window_event` (`main.rs`) как есть, с
//! дедентом на 8 пробелов и без единой правки логики; строки внутри
//! многострочных строковых литералов дедент не затронул.

use crate::*;

impl Lumen {
    pub(crate) fn on_mouse_wheel(&mut self, delta: MouseScrollDelta, phase: TouchPhase) {
        // DevTools inspector intercepts the wheel while visible (§7E.2):
        // scroll the active tab's property list. The Network tab is
        // page-wide and scrolls even without a pinned element.
        if self.dom_inspector.visible
            && (self.dom_inspector.selected.is_some()
                || self.dom_inspector.active_tab
                    == devtools::inspector::InspectorTab::Network)
        {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            if lines > 0.0 {
                self.dom_inspector.scroll_up(lines.abs().ceil() as usize);
            } else if lines < 0.0 {
                self.dom_inspector.scroll_down(lines.abs().ceil() as usize);
            }
            self.request_redraw();
            return;
        }
        // Privacy network panel intercepts the wheel while visible:
        // scroll the request list instead of the page.
        if self.privacy.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            let tab_h = toolbar::CHROME_H;
            let win_h = self.viewport_height_css() + tab_h;
            let body_h = panels::privacy_panel::list_body_height(win_h, tab_h);
            if lines > 0.0 {
                self.privacy.scroll_up(lines.abs().ceil() as usize);
            } else if lines < 0.0 {
                self.privacy.scroll_down(lines.abs().ceil() as usize, body_h);
            }
            self.request_redraw();
            return;
        }
        // DevTools network panel intercepts the wheel while visible:
        // scroll the request list instead of the page.
        if self.network_panel.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            if lines > 0.0 {
                self.network_panel.scroll_up(lines.abs().ceil() as usize);
            } else if lines < 0.0 {
                self.network_panel.scroll_down(lines.abs().ceil() as usize);
            }
            self.request_redraw();
            return;
        }
        // §12.3 Read-later panel intercepts the wheel while visible.
        if self.read_later_panel.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            let max_scroll = self.read_later_panel.max_scroll();
            if lines > 0.0 {
                self.read_later_panel.scroll_up();
            } else if lines < 0.0 {
                self.read_later_panel.scroll_down(max_scroll);
            }
            self.request_redraw();
            return;
        }
        // Settings panel intercepts the wheel while visible: scroll the
        // active section's content rather than the page.
        if self.settings_panel.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            self.settings_panel.scroll_by(-lines * LINE_STEP_CSS_PX);
            self.request_redraw();
            return;
        }
        // Bookmark panel intercepts the wheel while visible: scroll the
        // bookmark list rather than the page.
        if self.bookmark_panel.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            // winit: wheel up → lines > 0 → scroll content up (scroll_y -=).
            self.bookmark_panel.scroll_by(-lines * LINE_STEP_CSS_PX);
            self.request_redraw();
            return;
        }
        // History panel intercepts the wheel while visible.
        if self.history_panel.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            self.history_panel.scroll_by(-lines * LINE_STEP_CSS_PX);
            self.request_redraw();
            return;
        }
        // Keyboard shortcuts panel intercepts the wheel while visible (§D-4).
        if self.shortcuts_panel.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            self.shortcuts_panel.scroll_by(-lines * LINE_STEP_CSS_PX);
            self.request_redraw();
            return;
        }
        // Certificate viewer panel intercepts the wheel while visible (§D-1).
        if self.cert_panel.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            self.cert_panel.scroll_by(-lines * LINE_STEP_CSS_PX);
            self.request_redraw();
            return;
        }
        // Vertical tabs panel (GG-4) intercepts the wheel while visible.
        if self.vertical_tabs.visible {
            let lines = match delta {
                MouseScrollDelta::LineDelta(_, l) => l,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 40.0,
            };
            let tab_h = toolbar::CHROME_H;
            let win_h = self.viewport_height_css() + tab_h;
            let panel_h = win_h - tab_h;
            self.vertical_tabs.scroll_by(
                -lines * LINE_STEP_CSS_PX,
                self.tab_strip.len(),
                panel_h,
            );
            self.request_redraw();
            return;
        }
        // winit отдаёт два типа дельты:
        // - LineDelta(cols, lines): mouse wheel notch, нет momentum.
        // - PixelDelta({x, y}): тачпад, device px, делим на DPR.
        //   Отслеживаем velocity для momentum при TouchPhase::Ended.
        // Y: winit y > 0 — wheel up → scroll_y -= delta.
        // X: winit x > 0 — wheel left → scroll_x -= delta.
        // Shift+вертикальный wheel → горизонтальный скролл.
        let dpr = self
            .renderer
            .as_ref()
            .map_or(1.0_f32, |r| r.scale_factor() as f32);
        let shift = self.modifiers.shift_key();

        // In split mode, check if right pane is focused; route scroll there.
        let right_pane_focused = self
            .split_view
            .as_ref()
            .is_some_and(|sv| sv.focused == panels::split_view::SplitFocus::Right);

        match delta {
            MouseScrollDelta::LineDelta(cols, lines) => {
                // Mouse wheel: дискретные тики, momentum не нужен.
                self.momentum_anim = None;
                self.forward_momentum_stop();
                self.touchpad_vel = (0.0, 0.0);
                let dx = -cols * 40.0;
                let dy = -lines * 40.0;
                let (dx_css, dy_css) = if shift { (dy, 0.0) } else { (dx, dy) };
                if right_pane_focused {
                    let vh = self.viewport_height_css();
                    let vw = (self.viewport_width_css() / 2.0).floor();
                    if let Some(ref mut sv) = self.split_view {
                        if dy_css != 0.0 {
                            let max =
                                (sv.right.content_height - vh).max(0.0);
                            sv.right.scroll_y =
                                (sv.right.scroll_y + dy_css).clamp(0.0, max);
                        }
                        if dx_css != 0.0 {
                            let max = (sv.right.content_width - vw).max(0.0);
                            sv.right.scroll_x =
                                (sv.right.scroll_x + dx_css).clamp(0.0, max);
                        }
                    }
                    self.request_redraw();
                } else if self.try_scroll_frame_overflow_container(dx_css, dy_css) {
                    // FRAME-3 срез 3: overflow-контейнер ВНУТРИ под-документа —
                    // самый глубокий скроллер под курсором, проверяется до
                    // прокрутки фрейма целиком.
                } else if self.try_scroll_frame(dx_css, dy_css) {
                    // BUG-480 срез 17: колесо над содержимым фрейма
                    // крутит его под-документ, а не страницу. Проверяется
                    // ДО overflow-контейнеров: фрейм — ближайший к курсору
                    // скроллер, а контейнеры в этом списке — всегда боксы
                    // СТРАНИЦЫ, то есть предки хоста фрейма.
                } else if self.try_scroll_overflow_container(dx_css, dy_css) {
                    // Wheel was consumed by an overflow container — do not
                    // scroll the page.
                } else {
                    if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                    self.scroll_by_smooth(dy_css);
                    // BUG-821: the window 'scroll' event used to be fired
                    // right here, which made it a property of the mouse
                    // wheel rather than of the position. It is now fired
                    // from the RedrawRequested scroll step for every
                    // movement — including the frames of the smooth
                    // animation this wheel notch just started — so a
                    // second dispatch here would only duplicate it (and
                    // would still fire at the very bottom, where the
                    // wheel changes nothing).
                }
            }
            MouseScrollDelta::PixelDelta(p) => {
                let raw_x = -(p.x as f32) / dpr.max(1e-6);
                let raw_y = -(p.y as f32) / dpr.max(1e-6);
                let (dx_css, dy_css) = if shift { (raw_y, 0.0) } else { (raw_x, raw_y) };

                match phase {
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        // Палец снят: запускаем momentum если есть
                        // скорость (фаза Ended) или сбрасываем (Cancelled).
                        if phase == TouchPhase::Ended {
                            let (vx, vy) = self.touchpad_vel;
                            if vx.abs() + vy.abs() >= momentum_anim::MIN_VELOCITY_PX_MS {
                                let now = self.epoch.elapsed().as_secs_f64() * 1000.0;
                                self.momentum_anim =
                                    Some(momentum_anim::MomentumAnim::new(vy, vx, now));
                                // ADR-016 M1.3: рендер-поток продолжит
                                // инерцию сам, если UI-поток застопорится.
                                self.forward_momentum_start(vy, vx);
                                self.request_redraw();
                            }
                        }
                        self.touchpad_vel = (0.0, 0.0);
                    }
                    TouchPhase::Started => {
                        // Новый жест: сбросить momentum и velocity.
                        self.momentum_anim = None;
                        self.forward_momentum_stop();
                        self.touchpad_vel = (0.0, 0.0);
                        let now = self.epoch.elapsed().as_secs_f64() * 1000.0;
                        self.touchpad_vel_time_ms = now;
                        if right_pane_focused {
                            let vh = self.viewport_height_css();
                            if let Some(ref mut sv) = self.split_view
                                && dy_css != 0.0
                            {
                                let max =
                                    (sv.right.content_height - vh).max(0.0);
                                sv.right.scroll_y =
                                    (sv.right.scroll_y + dy_css).clamp(0.0, max);
                            }
                            self.request_redraw();
                        } else if self.try_scroll_frame_overflow_container(dx_css, dy_css) {
                            // FRAME-3 срез 3: то же соглашение, что у LineDelta-ветки.
                        } else if self.try_scroll_frame(dx_css, dy_css) {
                            // BUG-480 срез 17: тот же адресат, что и у колеса.
                        } else if self.try_scroll_overflow_container(dx_css, dy_css) {
                            // Touchpad gesture started over overflow container.
                        } else {
                            if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                            self.scroll_by_smooth(dy_css);
                        }
                    }
                    TouchPhase::Moved => {
                        // Палец движется: обновляем scroll и velocity (EWMA).
                        let now = self.epoch.elapsed().as_secs_f64() * 1000.0;
                        let dt = (now - self.touchpad_vel_time_ms).max(1.0);
                        self.touchpad_vel_time_ms = now;
                        // EWMA alpha = 0.6: быстро следует за движением,
                        // сглаживает дрожание.
                        const ALPHA: f32 = 0.6;
                        let inst_x = dx_css / dt as f32;
                        let inst_y = dy_css / dt as f32;
                        let (vx, vy) = self.touchpad_vel;
                        self.touchpad_vel = (
                            ALPHA * inst_x + (1.0 - ALPHA) * vx,
                            ALPHA * inst_y + (1.0 - ALPHA) * vy,
                        );
                        if right_pane_focused {
                            let vh = self.viewport_height_css();
                            if let Some(ref mut sv) = self.split_view
                                && dy_css != 0.0
                            {
                                let max =
                                    (sv.right.content_height - vh).max(0.0);
                                sv.right.scroll_y =
                                    (sv.right.scroll_y + dy_css).clamp(0.0, max);
                            }
                            self.request_redraw();
                        } else if self.try_scroll_frame_overflow_container(dx_css, dy_css) {
                            // FRAME-3 срез 3: то же соглашение, что у LineDelta-ветки.
                        } else if self.try_scroll_frame(dx_css, dy_css) {
                            // BUG-480 срез 17: тот же адресат, что и у колеса.
                        } else if self.try_scroll_overflow_container(dx_css, dy_css) {
                            // Touchpad move over overflow container.
                        } else {
                            if dx_css != 0.0 { self.scroll_x_by(dx_css); }
                            self.scroll_by_smooth(dy_css);
                        }
                    }
                }
            }
        }
    }
}
