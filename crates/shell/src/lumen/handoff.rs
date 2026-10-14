//! Передача страницы во внешний браузер (COMPAT-HANDOFF): команды палитры и
//! перехват клика по ссылке при живом правиле на 30 дней.
//!
//! Чистая часть (перечисление браузеров, выбор по причине, проверки URL,
//! тексты) — `crate::external_browser`; правила на диске —
//! `lumen_storage::ExternalBrowserRules`. Здесь они сходятся с состоянием окна.

use crate::external_browser::{
    enumerate, find_by_id, handoff_notice, is_cross_site_handoff, is_handoff_url, launch,
    no_suitable_message, pick, HandoffReason, Pick,
};
use crate::*;

/// Файл-метка «предупреждение перед первой передачей уже показано».
const WARNED_MARKER: &str = "handoff-warned";

pub(super) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// `true` ровно один раз за установку: первая передача сопровождается
/// предупреждением (COMPAT-HO-03). Не удалось записать метку — предупреждение
/// лишний раз лучше, чем его отсутствие.
fn take_first_handoff() -> bool {
    let path = adblock::browser_data_dir().join(WARNED_MARKER);
    if path.exists() {
        return false;
    }
    let _ = std::fs::create_dir_all(adblock::browser_data_dir());
    let _ = std::fs::write(&path, b"1");
    true
}

impl Lumen {
    /// Меню страницы по правому клику без сдвига (жесты остаются на протяжке):
    /// пункты «Открыть в другом браузере» — только на http(s)-странице и вне
    /// chrome. `true` — меню открыто, клик обработан.
    pub(crate) fn try_open_page_menu(&mut self, x_css: f32, y_css: f32) -> bool {
        if self.point_over_chrome(x_css, y_css) || !is_handoff_url(self.current_display_url()) {
            return false;
        }
        self.page_context_menu.open_page(x_css, y_css);
        self.relayout_chrome_host();
        true
    }

    /// Передать текущую страницу другому установленному браузеру.
    ///
    /// Без `remember` — одноразово, правила не создаётся. С `remember` по клику
    /// пользователя создаётся правило сайта на 30 дней: ссылки на него потом
    /// открываются во внешнем браузере ([`Self::link_handed_off`]). Уходит
    /// только URL (COMPAT-HO-03); результат — запущенный браузер или причина,
    /// по которой подходящего нет, — показывается уведомлением ОС.
    pub(crate) fn open_in_other_browser(&mut self, reason: HandoffReason, remember: bool) {
        let url = self.current_display_url().to_owned();
        self.open_url_in_other_browser(&url, reason, remember);
    }

    /// То же для заданного адреса: экран ошибки сертификата передаёт адрес,
    /// на который навигация не состоялась, а не адрес показанной страницы.
    pub(crate) fn open_url_in_other_browser(&mut self, url: &str, reason: HandoffReason, remember: bool) {
        let browsers = enumerate();
        let own = std::env::current_exe().ok();
        let body = if !is_handoff_url(url) {
            "Во внешний браузер передаются только http(s)-страницы.".to_owned()
        } else if remember && self.active_profile_is_anonymous() {
            "В анонимном профиле правила передачи не сохраняются.".to_owned()
        } else {
            match pick(reason, &browsers, own.as_deref()) {
                Pick::NoSuitable => no_suitable_message(reason).to_owned(),
                Pick::Browser(b) => match launch(b, url) {
                    Ok(()) => {
                        eprintln!("[handoff] reason={} browser={} remember={remember}", reason.code(), b.id);
                        self.note_open_in_other(url, reason.code());
                        if remember {
                            self.remember_handoff(url, &b.id, reason);
                        }
                        handoff_notice(&b.name, take_first_handoff())
                    }
                    Err(err) => format!("Не удалось запустить {}: {err}", b.name),
                },
            }
        };
        notification::show_os_notification("Lumen", &body);
    }

    /// Страница `about:handoff`: живые правила со сведёнными именами браузеров.
    pub(crate) fn build_handoff_source(&self) -> PageSource {
        let now = now_unix();
        let browsers = enumerate();
        let version = env!("CARGO_PKG_VERSION");
        let rows: Vec<handoff_page::HandoffRow> = self
            .external_browser_rules
            .list_active(now)
            .unwrap_or_default()
            .into_iter()
            .map(|rule| handoff_page::HandoffRow {
                browser_name: find_by_id(&browsers, &rule.browser_id).map(|b| b.name.clone()),
                needs_recheck: rule.needs_recheck(version),
                site: rule.site,
                reason: rule.reason,
                expires_at: rule.expires_at,
            })
            .collect();
        PageSource::Static {
            html: handoff_page::build_handoff_html(&rows, now),
            url: handoff_page::HANDOFF_URL.to_owned(),
        }
    }

    /// Открыть страницу «Сайты, открытые в другом браузере».
    pub(crate) fn show_handoff_sites(&mut self) {
        self.navigate_to(self.build_handoff_source());
    }

    /// Клик по ссылке `resolved` со страницы `about:handoff`: выполнить действие
    /// и вернуть `true`. Ссылки действий принимаются только с самой этой страницы —
    /// чужая страница не может снять или продлить правило.
    pub(crate) fn apply_handoff_link(&mut self, resolved: &str) -> bool {
        if self.current_display_url() != handoff_page::HANDOFF_URL {
            return false;
        }
        let Some(action) = handoff_page::parse_action(resolved) else {
            return false;
        };
        let rules = &self.external_browser_rules;
        match action {
            handoff_page::HandoffAction::Remove { site } => {
                let _ = rules.remove(&site);
            }
            handoff_page::HandoffAction::Keep { site } => {
                let _ = rules.renew(&site, env!("CARGO_PKG_VERSION"), now_unix());
            }
            handoff_page::HandoffAction::Recheck { site } => {
                let _ = rules.remove(&site);
                eprintln!("[handoff] recheck site={site}");
                self.navigate_to(PageSource::from_arg(Some(&format!("https://{site}/"))));
                return true;
            }
        }
        self.navigate_replace(self.build_handoff_source());
        true
    }

    fn remember_handoff(&self, url: &str, browser_id: &str, reason: HandoffReason) {
        let version = env!("CARGO_PKG_VERSION");
        if let Err(err) =
            self.external_browser_rules.grant(url, browser_id, reason.code(), version, now_unix())
        {
            eprintln!("[handoff] правило не сохранено: {err}");
        }
    }

    /// Клик по ссылке `href`: если на её сайт есть живое правило, открыть ссылку
    /// во внешнем браузере и вернуть `true` (Lumen никуда не переходит).
    ///
    /// Не перехватывается: ссылка внутри текущего сайта, не http(s), анонимный
    /// профиль, браузер из правила пропал или не запустился — тогда страница
    /// открывается в Lumen как обычно.
    pub(crate) fn link_handed_off(&mut self, href: &str) -> bool {
        let target = self.source.resolve_href(href);
        if !is_cross_site_handoff(self.current_display_url(), &target)
            || self.active_profile_is_anonymous()
        {
            return false;
        }
        let Ok(Some(rule)) = self.external_browser_rules.active_for(&target, now_unix()) else {
            return false;
        };
        let browsers = enumerate();
        let Some(browser) = find_by_id(&browsers, &rule.browser_id) else {
            return false;
        };
        if let Err(err) = launch(browser, &target) {
            eprintln!("[handoff] {} не запустился: {err}", browser.name);
            return false;
        }
        eprintln!("[handoff] rule site={} browser={}", rule.site, browser.id);
        notification::show_os_notification("Lumen", &handoff_notice(&browser.name, take_first_handoff()));
        true
    }
}
