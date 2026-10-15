//! Страница `about:compat` (COMPAT-PACK-PAGE): пакет совместимости, выбор пользователя
//! и журнал. Чистая часть — `crate::compat_page`; выбор лежит в `settings_store`
//! (`compat_pack_off`, `compat_disabled_ids`, `compat_journal`), сам пакет — базовая копия сборки.

use lumen_update_manifest::compat_pack::{CompatPack, CompatPolicy};

use crate::compat_page::{self, CompatAction, CompatView, PackSource};
use crate::*;

impl Lumen {
    /// Выбор пользователя из настроек. Отладочная сборка пакет не применяет
    /// независимо от выбора — это учитывает [`Self::compat_pack_active`].
    pub(crate) fn compat_policy(&self) -> CompatPolicy {
        CompatPolicy {
            pack_off: self.settings_store.compat_pack_off(),
            disabled: self.settings_store.compat_disabled_ids(),
        }
    }

    /// Записи пакета, применимые к `host` с учётом выбора пользователя; в отладочной
    /// сборке пакет выключен, список пуст. Единственная точка, откуда потребители пакета
    /// (CSS, идентичность, …) берут записи.
    #[allow(dead_code)] // потребители — COMPAT-PACK-CSS/-IDENTITY/-PROTECTION/-FLAG (этап 2)
    pub(crate) fn compat_pack_active(&self, host: &str) -> Vec<lumen_update_manifest::compat_pack::CompatEntry> {
        if cfg!(debug_assertions) {
            return Vec::new();
        }
        let pack = CompatPack::base(&lumen_storage::PslProvider::new());
        pack.active_entries_for_host(host, &self.compat_policy()).into_iter().cloned().collect()
    }

    /// Страница `about:compat`.
    pub(crate) fn build_compat_source(&self) -> PageSource {
        let pack = CompatPack::base(&lumen_storage::PslProvider::new());
        let policy = self.compat_policy();
        let journal = self.settings_store.compat_journal();
        let view = CompatView {
            pack: &pack,
            source: PackSource::Base,
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
