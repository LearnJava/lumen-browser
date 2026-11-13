//! Страница `about:compat` (COMPAT-PACK-PAGE): пакет совместимости, выбор пользователя
//! и журнал. Чистая часть — `crate::compat_page`; выбор лежит в `settings_store`
//! (`compat_pack_off`, `compat_disabled_ids`, `compat_journal`), сам пакет — базовая копия сборки.

use lumen_update_manifest::compat_pack::CompatPolicy;

use crate::compat_indicator;
use crate::compat_page::{self, CompatAction, CompatView, PackSource};
use crate::*;

/// Зеркало [`CompatPolicy`] для потоков вне UI (COMPAT-PACK-IDENTITY): потоки
/// загрузки и пайплайна строят HTTP-клиенты и JS-рантаймы без `&Lumen`, а
/// профиль записи `identity` обязан совпадать во всех слоях документа.
/// UI-поток обновляет зеркало при каждом [`Lumen::compat_policy`] — то есть
/// перед каждым запуском конвейера и после каждой мутации `about:compat`.
static COMPAT_POLICY_MIRROR: std::sync::RwLock<Option<CompatPolicy>> = std::sync::RwLock::new(None);

fn sync_compat_policy_mirror(policy: &CompatPolicy) {
    if let Ok(mut guard) = COMPAT_POLICY_MIRROR.write() {
        *guard = Some(policy.clone());
    }
}

/// Паттерны записей `protection`, активных при этом выборе пользователя
/// (COMPAT-PACK-PROTECTION) — для реестра исключений блокировщика в
/// `lumen_network`. В отладочной сборке пакет не применяется — список пуст.
fn protection_exempt_patterns(policy: &CompatPolicy) -> Vec<String> {
    if cfg!(debug_assertions) {
        return Vec::new();
    }
    let pack = crate::compat_pack_remote::current_pack();
    pack.entries
        .iter()
        .filter(|e| matches!(e.action, lumen_update_manifest::compat_pack::Action::Protection { .. }) && policy.is_entry_on(&e.id))
        .flat_map(|e| e.matches.iter().cloned())
        .collect()
}

/// Профиль записи `identity` для URL документа (COMPAT-PACK-IDENTITY, ADR-034
/// §1): единственный источник для HTTP-клиентов страницы и её JS-рантайма,
/// чтобы `User-Agent`/`Sec-CH-UA*` и `navigator.*` не разошлись. `None` —
/// подходящей записи нет, слои держат процессную идентичность. В debug-сборках
/// пакет всегда пуст (`compat_pack_active`), чтобы тесты были детерминированы.
pub(crate) fn compat_identity_for_url(url: &str) -> Option<lumen_core::IdentityProfile> {
    if cfg!(debug_assertions) {
        return None;
    }
    let parsed = lumen_core::url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_ascii().ok()?;
    let policy = COMPAT_POLICY_MIRROR
        .read()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default();
    crate::compat_pack_remote::current_pack().identity_for_host(&host, &policy)
}

impl Lumen {
    /// Выбор пользователя из настроек. Отладочная сборка пакет не применяет
    /// независимо от выбора — это учитывает [`Self::compat_pack_active`].
    pub(crate) fn compat_policy(&self) -> CompatPolicy {
        let policy = CompatPolicy {
            pack_off: self.settings_store.compat_pack_off(),
            disabled: self.settings_store.compat_disabled_ids(),
        };
        sync_compat_policy_mirror(&policy);
        lumen_network::install_protection_exempt_patterns(&protection_exempt_patterns(&policy));
        policy
    }

    /// Записи пакета, применимые к `host` с учётом выбора пользователя; в отладочной
    /// сборке пакет выключен, список пуст. Единственная точка, откуда потребители пакета
    /// (CSS, идентичность, …) берут записи.
    #[allow(dead_code)] // потребители — COMPAT-PACK-IDENTITY/-PROTECTION/-FLAG (этап 2)
    pub(crate) fn compat_pack_active(&self, host: &str) -> Vec<lumen_update_manifest::compat_pack::CompatEntry> {
        if cfg!(debug_assertions) {
            return Vec::new();
        }
        let pack = crate::compat_pack_remote::current_pack();
        pack.active_entries_for_host(host, &self.compat_policy()).into_iter().cloned().collect()
    }

    /// Данные пакета для внедрения в документ с базой `base` (COMPAT-PACK-CSS):
    /// `css` и `hide_banner` по хосту URL. Отладочная сборка и не-http(s) базы —
    /// пустой набор; видимость через `document.styleSheets` внедрение не получает.
    pub(crate) fn compat_inject_for_base(&self, base: &ResourceBase) -> crate::compat_inject::CompatInject {
        if cfg!(debug_assertions) {
            return crate::compat_inject::CompatInject::none();
        }
        let entries = match &base.kind {
            ResourceBaseKind::Url(u) => {
                let Some(url) = lumen_core::url::Url::parse(u).ok().filter(|u| matches!(u.scheme(), "http" | "https")) else {
                    return crate::compat_inject::CompatInject::none();
                };
                let Some(host) = url.host_ascii().ok() else {
                    return crate::compat_inject::CompatInject::none();
                };
                self.compat_pack_active(&host)
            }
            _ => Vec::new(),
        };
        crate::compat_inject::CompatInject::from_entries(&entries)
    }

    /// Пересчитать применённые к `host` записи пакета (при каждой загрузке страницы) и
    /// написать в консоль по одному `console.info` на запись (COMPAT-PACK-IND). В отладочной
    /// сборке пакет не применяется — список пуст.
    pub(crate) fn refresh_compat_applied(&mut self, host: Option<&str>) {
        self.compat_applied = match host {
            Some(host) if !cfg!(debug_assertions) => {
                let pack = crate::compat_pack_remote::current_pack();
                compat_indicator::applied_fixes(&pack, host, &self.compat_policy())
            }
            _ => Vec::new(),
        };
        if !self.compat_applied.is_empty() {
            self.devtools_console.push_batch(compat_indicator::console_batch(&self.compat_applied));
        }
    }

    /// «Выключить и перезагрузить»: выключить все записи, применённые к текущей странице,
    /// записать это в журнал и перезагрузить — после загрузки счётчик индикатора 0.
    pub(crate) fn compat_disable_applied_and_reload(&mut self) {
        let now = super::handoff::now_unix();
        let store = &self.settings_store;
        for fix in &self.compat_applied {
            match store.set_compat_entry_disabled(&fix.id, true) {
                Ok(()) => {
                    let note = format!("запись {} выключена из индикатора страницы", fix.id);
                    let _ = store.append_compat_journal(now, &note);
                    eprintln!("[compat] {note}");
                }
                Err(err) => eprintln!("[compat] выбор не сохранён: {err}"),
            }
        }
        self.reload();
    }

    /// Страница `about:compat`.
    pub(crate) fn build_compat_source(&self) -> PageSource {
        let pack = crate::compat_pack_remote::current_pack();
        let policy = self.compat_policy();
        let journal = self.settings_store.compat_journal();
        let view = CompatView {
            pack: &pack,
            source: if pack.key_id.is_empty() { PackSource::Base } else { PackSource::Fetched },
            policy: &policy,
            debug_build: cfg!(debug_assertions),
            journal: &journal,
            now_unix: super::handoff::now_unix(),
        };
        PageSource::Static {
            html: compat_page::build_compat_html(&view),
            url: compat_page::COMPAT_URL.to_owned(),
        }
    }

    /// Открыть страницу пакета совместимости.
    pub(crate) fn show_compat_page(&mut self) {
        self.navigate_to(self.build_compat_source());
    }

    /// Клик по ссылке `resolved` со страницы `about:compat`: выполнить действие и
    /// вернуть `true`. Ссылки действий принимаются только с самой этой страницы —
    /// чужая страница не может выключить запись.
    pub(crate) fn apply_compat_link(&mut self, resolved: &str) -> bool {
        if self.current_display_url() != compat_page::COMPAT_URL {
            return false;
        }
        let Some(action) = compat_page::parse_action(resolved) else {
            return false;
        };
        let store = &self.settings_store;
        let now = super::handoff::now_unix();
        let (result, note) = match &action {
            CompatAction::EntryOff { id } => (
                store.set_compat_entry_disabled(id, true),
                format!("запись {id} выключена"),
            ),
            CompatAction::EntryOn { id } => (
                store.set_compat_entry_disabled(id, false),
                format!("запись {id} включена"),
            ),
            CompatAction::PackOff(true) => (store.set_compat_pack_off(true), "список Lumen выключен".to_owned()),
            CompatAction::PackOff(false) => (store.set_compat_pack_off(false), "список Lumen включён".to_owned()),
        };
        match result {
            Ok(()) => {
                let _ = store.append_compat_journal(now, &note);
                eprintln!("[compat] {note}");
            }
            Err(err) => eprintln!("[compat] выбор не сохранён: {err}"),
        }
        self.navigate_replace(self.build_compat_source());
        true
    }
}
