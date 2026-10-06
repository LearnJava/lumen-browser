//! Поток браузера в маршрутизации колеса (ADR-032, срез 3): публикует снимок
//! прокрутки главному потоку и усыновляет смещение, которое вернул
//! рендер-поток.
//!
//! Сторона главного потока — [`crate::wheel_scroll::WheelRouter`], сторона
//! рендер-потока — `render_thread::RenderState`.

use crate::wheel_scroll::{ScrollSnapshot, wheel_route_disabled};
use crate::*;

impl Lumen {
    /// Подключает обратную связь рендер-потока сразу после создания бэкенда.
    /// Без потока браузера или без рендер-потока (CPU-бэкенд, откат) ручки нет,
    /// и колесо остаётся на этом потоке.
    pub(crate) fn attach_scroll_route(&mut self) {
        let Some(shared) = self.scroll_shared.clone() else {
            return;
        };
        let link = crate::render_thread::take_last_link();
        if let Some(link) = link.as_ref() {
            link.attach(Arc::clone(&shared));
        }
        shared.set_link(link.clone());
        self.scroll_link = link;
    }

    /// Снимок того, что решает главный поток: можно ли колесо над этой точкой
    /// отдать рендер-потоку.
    fn build_scroll_snapshot(&self) -> ScrollSnapshot {
        let (true, Some(host), Some(r)) =
            (self.scroll_link.is_some(), self.chrome_page_host_rect, self.renderer.as_ref())
        else {
            return ScrollSnapshot::default();
        };
        let any_panel = self.dom_inspector.visible
            || self.privacy.visible
            || self.network_panel.visible
            || self.read_later_panel.visible
            || self.settings_panel.visible
            || self.bookmark_panel.visible
            || self.history_panel.visible
            || self.shortcuts_panel.visible
            || self.cert_panel.visible
            || self.vertical_tabs.visible;
        let enabled = !wheel_route_disabled()
            && !any_panel
            && self.split_view.is_none()
            && self.snap_containers.is_empty()
            && self.layout_box.is_some();
        let mut blockers: Vec<[f32; 4]> = self
            .scroll_containers
            .iter()
            .map(|c| [c.clip_rect.x, c.clip_rect.y, c.clip_rect.width, c.clip_rect.height])
            .collect();
        blockers.extend(
            self.frames
                .iter()
                .filter(|f| f.parent_doc.is_none())
                .filter_map(|f| f.host_rect)
                .map(|h| [h.x, h.y, h.width, h.height]),
        );
        ScrollSnapshot {
            enabled,
            dpr: r.scale_factor() as f32,
            viewport: [host.x, host.y, host.x + host.width, host.y + host.height],
            origin: (host.x, host.y),
            max_y: self.max_scroll(),
            max_x: self.max_scroll_x(),
            blockers,
            wheel_listeners: Vec::new(),
        }
    }

    /// Публикует снимок, если он изменился. Вызывается из цикла потока браузера
    /// после каждой пачки сообщений.
    pub(crate) fn publish_scroll_snapshot(&mut self) {
        let Some(shared) = self.scroll_shared.as_ref() else {
            return;
        };
        let snap = self.build_scroll_snapshot();
        if self.scroll_snapshot_sent.as_ref() != Some(&snap) {
            shared.publish_snapshot(snap.clone());
            self.scroll_snapshot_sent = Some(snap);
        }
    }

    /// Усыновляет смещение, которое вело колесо на рендер-потоке. `true` — оно
    /// изменилось и страница перерисуется.
    pub(crate) fn adopt_scroll_feedback(&mut self) -> bool {
        let Some(shared) = self.scroll_shared.as_ref() else {
            return false;
        };
        let Some(fb) = shared.take_feedback() else {
            return false;
        };
        if fb.gen_id <= self.scroll_adopted_gen {
            return false;
        }
        self.scroll_adopted_gen = fb.gen_id;
        let y = clamp_scroll(fb.y, self.max_scroll());
        let x = clamp_scroll(fb.x, self.max_scroll_x());
        // Кривую и инерцию теперь ведёт рендер-поток.
        self.scroll_anim = None;
        self.momentum_anim = None;
        self.scroll_y = y;
        self.scroll_x = x;
        if let Some(link) = self.scroll_link.as_ref() {
            link.adopt(fb.gen_id, y, x);
        }
        self.request_redraw();
        true
    }
}
