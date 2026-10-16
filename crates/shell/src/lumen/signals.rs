//! Источники локальных сигналов «сайт не работает» (COMPAT-SIG-01): точки
//! сбора в окне. Правила и журнал — `crate::compat_signals`; здесь они
//! получают часы и адрес текущей страницы.
//!
//! Пишутся только http(s)-страницы вне анонимного профиля. Сигнал не уходит
//! никуда (COMPAT-SIG-05): пока потребителя нет, он виден в stderr и лежит в
//! `compat_signals.db` (журнал и момент последнего выключения защиты по сайту).

use crate::compat_signals::{site_of, Signal, SignalKind, SignalLog, PROTECTION_REPEAT_SECS};
use crate::external_browser::is_handoff_url;
use crate::*;

use super::handoff::now_unix;

fn announce(signal: &Signal) {
    eprintln!("[signal] {} site={} detail={}", signal.kind.code(), signal.site, signal.detail);
}

/// Открыть `compat_signals.db` (при ошибке диска — в памяти), убрать
/// устаревшее и поднять журнал окна из того, что осталось.
pub(crate) fn open_signal_log() -> (SignalLog, Arc<lumen_storage::CompatSignalStore>) {
    let store = Arc::new(crate::window_mode::open_persistent(
        "compat_signals.db",
        lumen_storage::CompatSignalStore::open,
        lumen_storage::CompatSignalStore::open_in_memory,
        "compat_signals init",
    ));
    let _ = store.purge(now_unix(), PROTECTION_REPEAT_SECS);
    let signals = store
        .load_signals()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| {
            Some(Signal {
                kind: SignalKind::from_code(&s.kind)?,
                site: s.site,
                at: s.at,
                detail: s.detail,
            })
        })
        .collect();
    let log = SignalLog::restore(signals, store.load_protection_off().unwrap_or_default());
    (log, store)
}

impl Lumen {
    /// Адрес текущей страницы, если по ней можно собирать сигналы.
    fn signal_url(&self) -> Option<String> {
        let url = self.current_display_url();
        (is_handoff_url(url) && !self.active_profile_is_anonymous()).then(|| url.to_owned())
    }

    /// Пользователь перезагрузил страницу (кнопка, F5, палитра). Три раза
    /// подряд за 30 с — сильный сигнал. Перезагрузки из JS и восстановление
    /// фоновой вкладки сюда не попадают.
    pub(crate) fn note_user_reload(&mut self) {
        self.frustration_reload();
        let Some(url) = self.signal_url() else { return };
        if let Some(signal) = self.compat_signals.record_reload(&url, now_unix()) {
            self.persist_signal(&signal);
        }
    }

    /// Пользователь выключил защиту на текущем сайте; повторно за 7 суток —
    /// сильный сигнал.
    pub(crate) fn note_site_protection_off(&mut self) {
        let Some(url) = self.signal_url() else { return };
        let now = now_unix();
        let _ = self.compat_signal_store.set_protection_off(&site_of(&url), now);
        if let Some(signal) = self.compat_signals.record_protection_off(&url, now) {
            self.persist_signal(&signal);
        }
    }

    /// Страница `url` передана во внешний браузер по причине `reason_code`.
    pub(crate) fn note_open_in_other(&mut self, url: &str, reason_code: &str) {
        if self.active_profile_is_anonymous() {
            return;
        }
        let signal = self.compat_signals.record_open_in_other(url, reason_code, now_unix());
        self.persist_signal(&signal);
    }

    /// «Очистить историю»: журнал сигналов — те же посещённые сайты, стирается
    /// вместе с ней, в памяти и на диске.
    pub(crate) fn forget_all_signals(&mut self) {
        self.compat_signals.clear();
        self.gate_loop.clear();
        self.frustration.clear();
        let _ = self.settings_store.set_compat_chip_limiter("");
        let _ = self.compat_signal_store.clear();
    }

    /// «Очистить данные сайта»: сигналы этого сайта.
    pub(crate) fn forget_site_signals(&mut self, site: &lumen_storage::PartitionKey) {
        self.compat_signals.clear_site(site.site());
        self.gate_loop.forget(site.site());
        self.frustration.forget_site(site.site());
        let _ = self.compat_signal_store.clear_site(site.site());
    }

    /// Показать сигнал в stderr и дописать его в журнал на диске.
    pub(super) fn persist_signal(&self, signal: &Signal) {
        announce(signal);
        let _ = self.compat_signal_store.append(&lumen_storage::StoredSignal {
            kind: signal.kind.code().to_owned(),
            site: signal.site.clone(),
            at: signal.at,
            detail: signal.detail.clone(),
        });
    }
}
