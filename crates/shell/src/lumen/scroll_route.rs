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
        // Контейнеры страницы цель колеса выбирает рендер-поток (`containers`);
        // фреймы по-прежнему решает поток браузера.
        let blockers: Vec<[f32; 4]> = self
            .frames
            .iter()
            .filter(|f| f.parent_doc.is_none())
            .filter_map(|f| f.host_rect)
            .map(|h| [h.x, h.y, h.width, h.height])
            .collect();
        ScrollSnapshot {
            enabled,
            dpr: r.scale_factor() as f32,
            viewport: [host.x, host.y, host.x + host.width, host.y + host.height],
            origin: (host.x, host.y),
            max_y: self.max_scroll(),
            max_x: self.max_scroll_x(),
            blockers,
            containers: if enabled { self.scroll_containers.clone() } else { Vec::new() },
            adopted_gen: self.scroll_adopted_gen,
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
        if fb.cmd_epoch < self.scroll_cmd_epoch {
            // Связь снята до программной прокрутки (правило 7): страницу она
            // не двигает, но поколение усыновлено и контейнеры не теряются.
            if let Some(link) = self.scroll_link.as_ref() {
                link.adopt(fb.gen_id, self.scroll_y, self.scroll_x);
            }
            let skip = self.scroll_cmd_containers.clone();
            let kept = fb.containers.into_iter().filter(|c| !skip.contains(&c.0)).collect();
            self.apply_feedback_containers(kept);
            return false;
        }
        self.scroll_cmd_containers.clear();
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
        self.apply_feedback_containers(fb.containers);
        self.request_redraw();
        true
    }

    /// Программная прокрутка страницы (ADR-032, правило 7): сообщает
    /// рендер-потоку новое смещение под новой эпохой. Вызывается после записи
    /// `scroll_y`/`scroll_x`; обратная связь со старой эпохой отбрасывается.
    pub(crate) fn issue_scroll_command(&mut self) {
        self.issue_container_scroll_command(&[]);
    }

    /// То же, что [`Self::issue_scroll_command`], плюс программная запись
    /// смещений контейнеров `ids` (`el.scrollTop`, `scrollIntoView`, якорь):
    /// рендер-поток снимает с них своё смещение, а устаревшая связь не
    /// откатывает запись.
    pub(crate) fn issue_container_scroll_command(&mut self, ids: &[u32]) {
        self.scroll_cmd_containers.extend_from_slice(ids);
        self.scroll_cmd_epoch += 1;
        let (epoch, y, x) = (self.scroll_cmd_epoch, self.scroll_y, self.scroll_x);
        if let Some(r) = self.renderer.as_mut() {
            r.scroll_command(epoch, y, x, ids);
        }
    }

    /// Контейнеры, которые вело колесо на рендер-потоке: тот же хвост, что у
    /// колеса потока браузера (раскладка, список, `scroll`-события).
    fn apply_feedback_containers(&mut self, containers: Vec<(u32, f32, f32)>) {
        for (id, cx, cy) in containers {
            let Some(node) = self
                .scroll_containers
                .iter()
                .find(|c| c.node.index() as u32 == id && (c.scroll_x != cx || c.scroll_y != cy))
                .map(|c| c.node)
            else {
                continue;
            };
            self.apply_container_scroll(node, cx, cy);
        }
    }
}
