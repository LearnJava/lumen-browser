//! Отчёт о поломке сайта (COMPAT-REPORT): сбор данных окна, три страницы
//! (`about:report`, `-preview`, `-done`) и экспорт в файл или буфер обмена.
//! Правила, очистка и сборка отчёта — `crate::compat_report`; HTML и разбор
//! ссылок — `crate::report_page`.
//!
//! Отчёт — явное разовое действие. Он не зависит от настроек телеметрии, ничего
//! не отправляет по сети и работает из анонимного профиля (там сайт не
//! запоминается на диск).

use lumen_core::ext::ClipboardProvider;
use lumen_update_manifest::compat_pack::{Action, CompatPack};

use crate::compat_page::ymd;
use crate::compat_report::{
    build_report, count_names, export_file_name, js_error_type, known_issue_text, report_text, report_url,
    shuffled_reasons, site_category, AppliedInfo, AutoFields, Draft, ReportedSites,
};
use crate::compat_signals::{site_of, SignalKind};
use crate::devtools::console_panel::ConsoleLevel;
use crate::external_browser::is_handoff_url;
use crate::report_page::{self, Delivered, FormView, PreviewAction, PreviewView};
use crate::*;

use super::handoff::now_unix;

/// Что известно о странице, на которую жалуются: снято при открытии формы,
/// потому что журналы консоли и сети очищаются при уходе со страницы.
#[derive(Debug, Clone)]
pub(crate) struct ReportSession {
    url: String,
    host: String,
    site: String,
    auto: AutoFields,
    blocked_hosts: Vec<String>,
    seed: u64,
    draft: Draft,
    known: Option<String>,
    /// Текст последнего предпросмотра: экспортируется ровно он.
    previewed: Option<String>,
}

/// Состояние отчёта окна.
#[derive(Debug, Default)]
pub(crate) struct ReportState {
    session: Option<ReportSession>,
    /// Память анонимного профиля: живёт, пока открыто окно.
    anon_log: ReportedSites,
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

fn clock_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
}

fn today() -> i64 {
    now_unix().div_euclid(86_400)
}

impl Lumen {
    /// Поля отчёта, которые окно знает о текущей странице без участия пользователя.
    fn collect_report_fields(&mut self, site: &str, host: &str) -> (AutoFields, Vec<String>) {
        self.shields.refresh();
        let crowd = self.active_profile_is_crowd();
        let pack = CompatPack::base(&lumen_storage::PslProvider::new());
        let profile_kind = self.profile_menu.active_entry().map_or_else(
            || "unknown".to_owned(),
            |e| panels::profile_menu::slug_for_profile(&e.name).unwrap_or("custom").to_owned(),
        );
        let languages = crate::config::global().effective_languages(crowd).join(",");
        let entries = self.network_panel.entries_clone();
        let blocked_hosts: Vec<String> = entries.iter().filter(|e| e.blocked).map(|e| host_of(&e.url)).collect();
        let http_errors = count_names(entries.iter().filter_map(|e| match (e.status, e.failed) {
            (Some(status), _) if status >= 400 => Some(status.to_string()),
            (None, true) => {
                let stage = e.reason.as_deref().and_then(|r| r.split(':').next()).unwrap_or("failed");
                Some(format!("net-{stage}"))
            }
            _ => None,
        }));
        let js_errors = count_names(
            self.devtools_console
                .messages()
                .iter()
                .filter(|m| m.level == ConsoleLevel::Error)
                .filter_map(|m| js_error_type(&m.text)),
        );
        let signals = || self.compat_signals.recent().filter(|s| s.site == site);
        let fixed_by: Vec<String> =
            signals().filter(|s| s.kind == SignalKind::FixedBy).map(|s| s.detail.clone()).collect();
        let antibot_vendor =
            signals().filter(|s| s.kind == SignalKind::AntibotLoop).last().map(|s| s.detail.clone());
        let auto = AutoFields {
            lumen_version: env!("CARGO_PKG_VERSION").to_owned(),
            pack_revision: pack.revision,
            identity: format!("languages={languages}"),
            profile_kind,
            protection_on: self.shields.enabled_for_current(),
            blocked_count: self.shields.blocked_total_count(),
            applied: self
                .compat_applied
                .iter()
                .map(|a| AppliedInfo { id: a.id.clone(), root_cause: a.root_cause.clone(), bug: a.bug.clone() })
                .collect(),
            fixed_by,
            // Источников нет: перечень нереализованных API, издатель TLS и признак
            // VPN появятся вместе с механизмами, которые их считают.
            unimplemented_apis: Vec::new(),
            js_errors,
            http_errors,
            tls_issuer: None,
            tls_interceptor: None,
            proxy_or_vpn: None,
            antibot_vendor: antibot_vendor.filter(|v| !v.is_empty()),
            category: site_category(host).map(str::to_owned),
        };
        (auto, blocked_hosts)
    }

    /// Открыть форму отчёта о текущей странице (окно сайта, палитра команд,
    /// адресная строка). На странице отчёта продолжает начатый отчёт.
    pub(crate) fn open_report_form(&mut self) {
        let current = self.current_display_url().to_owned();
        if !report_page::is_report_url(&current) || self.report.session.is_none() {
            self.report.session = is_handoff_url(&current).then(|| {
                let site = site_of(&current);
                let host = host_of(&current);
                let (auto, blocked_hosts) = self.collect_report_fields(&site, &host);
                let pack = CompatPack::base(&lumen_storage::PslProvider::new());
                ReportSession {
                    known: known_issue_text(&pack.entries_for_host(&host)),
                    url: current.clone(),
                    host,
                    site,
                    auto,
                    blocked_hosts,
                    seed: clock_seed(),
                    draft: Draft::default(),
                    previewed: None,
                }
            });
        }
        self.shields.visible = false;
        self.permission.visible = false;
        self.fix_panel.close();
        self.navigate_to(self.build_report_form_source());
    }

    fn build_report_form_source(&self) -> PageSource {
        let html = match &self.report.session {
            Some(s) => {
                let order = shuffled_reasons(s.seed);
                let short_url = report_url(&s.url, false).unwrap_or_default();
                report_page::build_form_html(&FormView {
                    host: &s.host,
                    short_url: &short_url,
                    order: &order,
                    draft: &s.draft,
                    known: s.known.as_deref(),
                    blocked_count: s.auto.blocked_count,
                    no_site: false,
                })
            }
            None => report_page::build_form_html(&FormView {
                host: "",
                short_url: "",
                order: &[],
                draft: &Draft::default(),
                known: None,
                blocked_count: 0,
                no_site: true,
            }),
        };
        PageSource::Static { html, url: report_page::REPORT_URL.to_owned() }
    }

    /// Сайты, о которых сообщали: на диске, а в анонимном профиле — в памяти окна.
    fn reported_sites(&self) -> ReportedSites {
        if self.active_profile_is_anonymous() {
            self.report.anon_log.clone()
        } else {
            ReportedSites::parse(&self.settings_store.compat_reported_sites())
        }
    }

    fn remember_reported(&mut self, site: &str, pack_revision: u32) {
        let mut log = self.reported_sites();
        log.record(site, today(), pack_revision);
        if self.active_profile_is_anonymous() {
            self.report.anon_log = log;
        } else if let Err(err) = self.settings_store.set_compat_reported_sites(&log.to_text()) {
            eprintln!("[report] память об отчётах не сохранена: {err}");
        }
    }

    /// Отправка формы `about:report?...` (GET): показать предпросмотр. Принимается
    /// только со страницы формы — чужая страница не может подложить отчёт.
    pub(crate) fn apply_report_form(&mut self, resolved: &str) -> bool {
        if self.current_display_url() != report_page::REPORT_URL {
            return false;
        }
        let Some(draft) = report_page::parse_form_submit(resolved) else {
            return false;
        };
        let Some(site) = self.report.session.as_ref().map(|s| s.site.clone()) else {
            return false;
        };
        let duplicate = self.reported_sites().reported_on(&site, today());
        let Some(session) = self.report.session.as_mut() else {
            return false;
        };
        session.draft = draft;
        let report = build_report(&session.url, &session.draft, &session.auto, &session.blocked_hosts, &ymd(now_unix()));
        let text = report_text(&report);
        session.previewed = Some(text.clone());
        let html = report_page::build_preview_html(&PreviewView { text: &text, duplicate_today: duplicate });
        self.navigate_to(PageSource::Static { html, url: report_page::PREVIEW_URL.to_owned() });
        true
    }

    /// Клик по ссылке `resolved` со страницы предпросмотра. Принимается только с
    /// неё самой.
    pub(crate) fn apply_report_link(&mut self, resolved: &str) -> bool {
        if self.current_display_url() != report_page::PREVIEW_URL {
            return false;
        }
        let Some(action) = report_page::parse_preview_action(resolved) else {
            return false;
        };
        match action {
            PreviewAction::Edit => self.navigate_to(self.build_report_form_source()),
            PreviewAction::Save | PreviewAction::Copy => self.export_report(action),
        }
        true
    }

    fn export_report(&mut self, action: PreviewAction) {
        let Some(session) = self.report.session.as_ref() else { return };
        let Some(text) = session.previewed.clone() else { return };
        let site = session.site.clone();
        if self.reported_sites().reported_on(&site, today()) {
            return;
        }
        let delivered = match action {
            PreviewAction::Copy => {
                platform::clipboard::PlatformClipboard.write_text(&text);
                Delivered::Clipboard
            }
            _ => match save_report_file(&export_file_name(&site, &ymd(now_unix())), &text) {
                Ok(path) => Delivered::File(path.display().to_string()),
                Err(err) => {
                    eprintln!("[report] файл не сохранён: {err}");
                    notification::show_os_notification("Lumen", &format!("Не удалось сохранить отчёт: {err}"));
                    return;
                }
            },
        };
        let revision = CompatPack::base(&lumen_storage::PslProvider::new()).revision;
        self.remember_reported(&site, revision);
        let remembered = !self.active_profile_is_anonymous();
        self.navigate_to(PageSource::Static {
            html: report_page::build_done_html(&delivered, remembered),
            url: report_page::DONE_URL.to_owned(),
        });
    }

    /// При старте: сообщить о сайтах, о которых писал пользователь и для которых
    /// в новом пакете совместимости появилась правка. Каждый — один раз.
    pub(crate) fn announce_fixed_reports(&self) {
        if cfg!(debug_assertions) || self.active_profile_is_anonymous() {
            return;
        }
        let mut log = ReportedSites::parse(&self.settings_store.compat_reported_sites());
        if log.is_empty() {
            return;
        }
        let pack = CompatPack::base(&lumen_storage::PslProvider::new());
        let fixed = log.take_newly_fixed(pack.revision, |site| {
            pack.entries_for_host(site)
                .iter()
                .any(|e| !matches!(e.action, Action::KnownIssue { .. } | Action::HandoffHint { .. }))
        });
        if fixed.is_empty() {
            return;
        }
        if let Err(err) = self.settings_store.set_compat_reported_sites(&log.to_text()) {
            eprintln!("[report] память об отчётах не сохранена: {err}");
        }
        for site in fixed {
            notification::show_os_notification(
                "Lumen",
                &format!("Сайт {site}, о котором вы сообщали, теперь должен работать: в обновлении есть правка."),
            );
        }
    }
}

/// Записать отчёт в папку загрузок под свободным именем; вернуть путь.
fn save_report_file(name: &str, text: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = crate::download::default_download_dir();
    std::fs::create_dir_all(&dir)?;
    let mut path = dir.join(name);
    let mut n = 2;
    while path.exists() {
        let stem = name.strip_suffix(".json").unwrap_or(name);
        path = dir.join(format!("{stem}-{n}.json"));
        n += 1;
    }
    std::fs::write(&path, text)?;
    Ok(path)
}
