//! Источники локальных сигналов «сайт не работает» (COMPAT-SIG-01): точки
//! сбора в окне. Правила и журнал — `crate::compat_signals`; здесь они
//! получают часы и адрес текущей страницы.
//!
//! Пишутся только http(s)-страницы вне анонимного профиля. Сигнал не уходит
//! никуда (COMPAT-SIG-05): пока потребителя нет, он виден в stderr.

use crate::compat_signals::Signal;
use crate::external_browser::is_handoff_url;
use crate::*;

use super::handoff::now_unix;

fn announce(signal: &Signal) {
    eprintln!("[signal] {} site={} detail={}", signal.kind.code(), signal.site, signal.detail);
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
        let Some(url) = self.signal_url() else { return };
        if let Some(signal) = self.compat_signals.record_reload(&url, now_unix()) {
            announce(&signal);
        }
    }

    /// Пользователь выключил защиту на текущем сайте; повторно за 7 суток —
    /// сильный сигнал.
    pub(crate) fn note_site_protection_off(&mut self) {
        let Some(url) = self.signal_url() else { return };
        if let Some(signal) = self.compat_signals.record_protection_off(&url, now_unix()) {
            announce(&signal);
        }
    }

    /// Страница `url` передана во внешний браузер по причине `reason_code`.
    pub(crate) fn note_open_in_other(&mut self, url: &str, reason_code: &str) {
        if self.active_profile_is_anonymous() {
            return;
        }
        let signal = self.compat_signals.record_open_in_other(url, reason_code, now_unix());
        announce(&signal);
    }
}
