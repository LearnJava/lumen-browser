//! COMPAT-FRUSTRATION: подключение детектора поведения (`crate::compat_frustration`)
//! к окну. Здесь он получает часы, адрес и сайт текущей страницы, а чип —
//! путь в хром и в панель «Сайт работает не так?».
//!
//! Работает только на http(s)-страницах вне анонимного профиля; ничего не
//! отправляет (COMPAT-SIG-05): память ограничителя лежит в `settings.db`.

use std::time::{Duration, Instant};

use crate::compat_frustration::{is_blank, Frustration, Limiter};
use crate::compat_signals::site_of;
use crate::external_browser::is_handoff_url;
use crate::*;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Детектор с памятью ограничителя из настроек.
pub(crate) fn load_frustration(store: &lumen_storage::BrowserSettings) -> Frustration {
    Frustration::with_limiter(Limiter::from_text(&store.compat_chip_limiter(), now_ms()))
}

impl Lumen {
    /// Сайт текущей страницы, если за ней можно наблюдать.
    fn frustration_site(&self) -> Option<String> {
        let url = self.current_display_url();
        (is_handoff_url(url) && !self.active_profile_is_anonymous()).then(|| site_of(url))
    }

    fn save_chip_limiter(&self) {
        let _ = self.settings_store.set_compat_chip_limiter(&self.frustration.limiter().to_text());
    }

    /// Пользовательская перезагрузка текущей страницы.
    pub(super) fn frustration_reload(&mut self) {
        let url = self.current_display_url().to_owned();
        if let Some(site) = self.frustration_site() {
            self.frustration.on_reload(&url, &site, now_ms());
        }
    }

    /// Страница загружена и установлена: через 3 с проверить белый экран.
    pub(crate) fn frustration_page_installed(&mut self) {
        if let Some(site) = self.frustration_site() {
            self.frustration.on_page_installed(&site, now_ms());
        }
    }

    /// Третья антибот-стена сайта `url` за минуту.
    pub(crate) fn frustration_antibot_loop(&mut self, url: &str) {
        if is_handoff_url(url) && !self.active_profile_is_anonymous() {
            self.frustration.on_antibot_loop(&site_of(url), now_ms());
        }
    }

    /// Страница `url` упала.
    pub(crate) fn frustration_crash(&mut self, url: &str) {
        if is_handoff_url(url) && !self.active_profile_is_anonymous() {
            self.frustration.on_crash(&site_of(url), now_ms());
        }
    }

    /// Нажата клавиша: чип не появится во время набора.
    pub(crate) fn frustration_key(&mut self) {
        self.frustration.note_key(now_ms());
    }

    /// Пользователь сам взялся за шаг панели: чип сайта не нужен.
    pub(crate) fn frustration_panel_used(&mut self) {
        if let Some(site) = self.frustration_site() {
            self.frustration.on_panel_used(&site);
        }
    }

    /// Текст чипа для хрома.
    pub(crate) fn help_chip_label(&self) -> Option<String> {
        self.frustration.chip().map(|c| c.trigger.label().to_owned())
    }

    /// Клик по тексту чипа: открывается панель сайта.
    pub(crate) fn help_chip_open(&mut self) {
        self.frustration.accept();
        self.save_chip_limiter();
        self.open_fix_panel();
    }

    /// «Не сейчас»: тишина на сайте 7 суток.
    pub(crate) fn help_chip_dismiss(&mut self) {
        self.frustration.dismiss(now_ms());
        self.save_chip_limiter();
        self.relayout_chrome_host();
        self.request_redraw();
    }

    /// Ход времени для цикла событий. Возвращает срок следующего пробуждения.
    pub(crate) fn frustration_tick(&mut self) -> Option<Instant> {
        if self.deterministic.enabled {
            return None;
        }
        let site = self.frustration_site().unwrap_or_default();
        let now = now_ms();
        let layout = self.layout_box.as_ref();
        let dom_nodes = self
            .layout_source
            .as_ref()
            .and_then(|ls| ls.document.lock().ok().map(|d| d.node_count()))
            .unwrap_or(0);
        let tick = self
            .frustration
            .tick(now, &site, || is_blank(dom_nodes, layout.map_or(0, count_rendered_units)));
        if tick.changed {
            if let Some(chip) = self.frustration.chip() {
                eprintln!("[chip] {} site={}", chip.trigger.code(), chip.site);
            }
            self.save_chip_limiter();
            self.relayout_chrome_host();
            self.request_redraw();
        }
        tick.next_wakeup_ms.map(|at| {
            Instant::now() + Duration::from_millis(u64::try_from((at - now).max(0)).unwrap_or(0))
        })
    }
}
