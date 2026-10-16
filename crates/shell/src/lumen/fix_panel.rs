//! COMPAT-PANEL: действия секции «Сайт работает не так?» в окне сайта
//! (`#permPopover`). Состояние и правила — `crate::compat_panel`; здесь они
//! получают страницу, перезагрузку, сброс данных сайта и журнал сигналов.
//!
//! Открывается щитом в адресной строке и правым кликом по «Обновить»: любой
//! шаг — два клика. Шаги «без защит на вкладку» и «как Chrome» ждут своих
//! механизмов (COMPAT-PANEL-TAB-SHIELDS, COMPAT-ID) и в лесенку не вынесены.

use std::time::Instant;

use crate::compat_panel::Step;
use crate::compat_signals::site_of;
use crate::external_browser::{find_by_id, enumerate, is_handoff_url, HandoffReason};
use crate::resource_base::ResourceBase;
use crate::*;

use super::handoff::now_unix;

impl Lumen {
    /// Адрес и сайт текущей страницы, если шаги к ней применимы (http(s)).
    fn fix_target(&self) -> Option<(String, String)> {
        let url = self.current_display_url();
        is_handoff_url(url).then(|| (url.to_owned(), site_of(url)))
    }

    /// Показать или скрыть окно сайта (щит, Ctrl+Shift+S, палитра); при показе
    /// секция начинает с чистого предупреждения.
    pub(crate) fn toggle_site_popover(&mut self) {
        self.shields.toggle();
        if self.shields.visible {
            self.fix_panel_opened();
        } else {
            self.fix_panel.close();
        }
    }

    fn fix_panel_opened(&mut self) {
        let site = self.fix_target().map(|(_, site)| site).unwrap_or_default();
        self.fix_panel.open_for(&site);
    }

    /// Правый клик по «Обновить»: то же окно сайта, открытое сразу.
    pub(crate) fn open_fix_panel(&mut self) {
        self.shields.visible = true;
        self.shields.refresh();
        self.fix_panel_opened();
        self.relayout_chrome_host();
        self.request_redraw();
    }

    fn close_site_popover(&mut self) {
        self.shields.visible = false;
        self.permission.visible = false;
        self.fix_panel.close();
        self.relayout_chrome_host();
        self.request_redraw();
    }

    /// Модель секции для chrome.
    pub(crate) fn fix_panel_model(&self) -> lumen_chrome::ChromeFixPanelModel {
        self.fix_panel.model(self.fix_target().is_some())
    }

    /// Шаг «Обновить без кэша».
    pub(crate) fn fix_no_cache(&mut self) {
        let Some((_, site)) = self.fix_target() else { return };
        self.fix_panel.step_done(Step::NoCache, &site, true, None, Instant::now());
        self.close_site_popover();
        crate::config::request_hard_reload();
        self.note_user_reload();
        self.reload();
    }

    /// Первое нажатие «Сбросить данные сайта…»: предупреждение, ничего не удалено.
    pub(crate) fn fix_reset_ask(&mut self) {
        if self.fix_target().is_some() {
            self.fix_panel.ask_reset_confirmation();
        }
        self.relayout_chrome_host();
    }

    /// «Отмена» в предупреждении о сбросе.
    pub(crate) fn fix_reset_cancel(&mut self) {
        self.fix_panel.cancel_reset();
        self.relayout_chrome_host();
    }

    /// Подтверждённый сброс: данные сайта удалены, страница перезагружается.
    pub(crate) fn fix_reset_confirm(&mut self) {
        if !self.fix_panel.is_confirming_reset() {
            return;
        }
        let Some((url, site)) = self.fix_target() else { return };
        let Some(key) = lumen_storage::PartitionKey::parse(&url) else { return };
        self.clear_site_data(&key);
        self.fix_panel.step_done(Step::ResetSite, &site, true, None, Instant::now());
        self.close_site_popover();
        self.reload();
    }

    /// Шаг «Открыть в другом браузере» (один раз, без правила).
    pub(crate) fn fix_handoff(&mut self) {
        let Some((url, site)) = self.fix_target() else { return };
        match self.open_url_in_other_browser(&url, HandoffReason::Other, false) {
            Some(browser_id) => {
                self.fix_panel.step_done(Step::OtherBrowser, &site, false, Some(browser_id), Instant::now());
                self.close_site_popover();
            }
            None => {
                self.fix_panel.set_note("Страница никуда не передана — причина в уведомлении системы.");
                self.relayout_chrome_host();
            }
        }
    }

    /// Ответ на «Заработало?». «Да» пишет `fixed_by` в журнал сигналов (вне
    /// анонимного профиля) и, если страницу открывали в другом браузере,
    /// сохраняет правило «открывать этот сайт там» на 30 дней. «Нет» оставляет
    /// окно открытым — лесенка показывает следующие шаги.
    pub(crate) fn fix_answer(&mut self, worked: bool) {
        let Some(asked) = self.fix_panel.answer() else { return };
        if !worked {
            self.relayout_chrome_host();
            self.request_redraw();
            return;
        }
        let url = self.current_display_url().to_owned();
        if is_handoff_url(&url) && !self.active_profile_is_anonymous() {
            let signal = self.compat_signals.record_fixed_by(&url, asked.step.code(), now_unix());
            self.persist_signal(&signal);
            if let Some(id) = asked.browser_id.as_deref() {
                self.remember_handoff(&url, id, HandoffReason::Other);
                let name = find_by_id(&enumerate(), id).map_or_else(|| id.to_owned(), |b| b.name.clone());
                notification::show_os_notification(
                    "Lumen",
                    &format!("Ссылки на этот сайт 30 дней будут открываться в {name}. Отменить: about:handoff."),
                );
            }
        }
        self.close_site_popover();
    }

    /// Страница `raw` загружена: если на нём ждали результата шага, начинается
    /// отсчёт 3 с до вопроса «Заработало?».
    pub(crate) fn fix_panel_load_done(&mut self, raw: &page_source::RawPage) {
        if let ResourceBase::Url(url) = &raw.base {
            self.fix_panel.load_done(&site_of(url), Instant::now());
        }
    }

    /// Ход времени для цикла событий: когда пора спросить, окно сайта
    /// открывается само. Возвращает ближайший срок, на который нужно проснуться.
    pub(crate) fn fix_panel_tick(&mut self) -> Option<Instant> {
        if self.deterministic.enabled {
            return None;
        }
        let site = self.fix_target().map(|(_, site)| site).unwrap_or_default();
        if self.fix_panel.tick(Instant::now(), &site) {
            self.shields.visible = true;
            self.shields.refresh();
            self.relayout_chrome_host();
            self.request_redraw();
        }
        self.fix_panel.next_wakeup()
    }
}
