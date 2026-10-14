//! Локальные сигналы «сайт не работает» (COMPAT-SIG-01): сильные сигналы, по
//! каждому из которых позже можно предложить чип помощи.
//!
//! Чистая часть без часов и ввода-вывода: время приходит аргументом (unix-секунды).
//! Детектор ничего не отправляет (COMPAT-SIG-05) и не хранит адреса целиком —
//! только сайт (eTLD+1) и код сигнала. Журнал живёт в памяти окна; на диск его
//! пишет `lumen/signals.rs` (`lumen_storage::CompatSignalStore`) и поднимает
//! при старте через [`SignalLog::restore`], чтобы окно 7 суток переживало
//! перезапуск. Анонимный профиль в журнал не пишет (решает вызывающий).
//!
//! Сильные сигналы (отчёт site-compat §COMPAT-SIG-01):
//! - 3 перезагрузки одного URL за 30 с;
//! - повторное выключение защиты на сайте за 7 дней;
//! - использование «Открыть в другом браузере».

use std::collections::{HashMap, VecDeque};

/// Окно перезагрузок, секунды.
pub const RELOAD_WINDOW_SECS: i64 = 30;
/// Сколько перезагрузок одного URL в окне считаются сигналом.
pub const RELOAD_COUNT: usize = 3;
/// Окно повторного выключения защиты, секунды (7 суток).
pub const PROTECTION_REPEAT_SECS: i64 = 7 * 24 * 60 * 60;
/// Сколько последних сигналов хранит журнал.
pub const LOG_CAP: usize = 100;

/// Вид сильного сигнала.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    /// 3 перезагрузки одного URL за 30 с.
    ReloadBurst,
    /// Защита на сайте выключена повторно в пределах 7 суток.
    ProtectionOffAgain,
    /// Страница передана во внешний браузер.
    OpenInOther,
}

impl SignalKind {
    /// Вид по коду из [`SignalKind::code`]; неизвестный код (запись новой
    /// версии) — `None`.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        [Self::ReloadBurst, Self::ProtectionOffAgain, Self::OpenInOther]
            .into_iter()
            .find(|k| k.code() == code)
    }

    /// Стабильный код сигнала (для журнала и будущего opt-in агрегата).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::ReloadBurst => "reload3",
            Self::ProtectionOffAgain => "protection_off_again",
            Self::OpenInOther => "open_in_other",
        }
    }
}

/// Один сигнал: что, на каком сайте, когда.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    /// Вид сигнала.
    pub kind: SignalKind,
    /// Сайт (eTLD+1), не полный адрес.
    pub site: String,
    /// Момент, unix-секунды.
    pub at: i64,
    /// Уточнение без текста пользователя: код причины передачи, иначе пусто.
    pub detail: String,
}

/// Журнал сигналов окна и счётчики, по которым они выводятся.
#[derive(Debug, Default)]
pub struct SignalLog {
    log: VecDeque<Signal>,
    /// Моменты перезагрузок по полному URL (внутри окна).
    reloads: HashMap<String, Vec<i64>>,
    /// Когда защита на сайте была выключена в последний раз.
    protection_off: HashMap<String, i64>,
}

/// Сайт (eTLD+1) адреса; без хоста — сам адрес в нижнем регистре.
pub fn site_of(url: &str) -> String {
    lumen_storage::PartitionKey::parse(url)
        .map_or_else(|| url.to_ascii_lowercase(), |k| k.site().to_owned())
}

impl SignalLog {
    /// Журнал, поднятый с диска: сигналы от старых к новым и моменты выключения
    /// защиты по сайтам. Лишнее сверх [`LOG_CAP`] отбрасывается с головы.
    #[must_use]
    pub fn restore(signals: Vec<Signal>, protection_off: Vec<(String, i64)>) -> Self {
        let skip = signals.len().saturating_sub(LOG_CAP);
        Self {
            log: signals.into_iter().skip(skip).collect(),
            reloads: HashMap::new(),
            protection_off: protection_off.into_iter().collect(),
        }
    }

    fn push(&mut self, kind: SignalKind, site: String, at: i64, detail: &str) -> Signal {
        if self.log.len() == LOG_CAP {
            self.log.pop_front();
        }
        let signal = Signal { kind, site, at, detail: detail.to_owned() };
        self.log.push_back(signal.clone());
        signal
    }

    /// Пользовательская перезагрузка `url` в момент `now`. Третья за окно даёт
    /// [`SignalKind::ReloadBurst`]; после сигнала счёт этого URL начинается заново.
    pub fn record_reload(&mut self, url: &str, now: i64) -> Option<Signal> {
        self.reloads.retain(|_, ts| {
            ts.retain(|&t| now - t <= RELOAD_WINDOW_SECS);
            !ts.is_empty()
        });
        let ts = self.reloads.entry(url.to_owned()).or_default();
        ts.push(now);
        if ts.len() < RELOAD_COUNT {
            return None;
        }
        self.reloads.remove(url);
        Some(self.push(SignalKind::ReloadBurst, site_of(url), now, ""))
    }

    /// Защита на сайте `url` выключена в момент `now`. Если за предыдущие 7
    /// суток она уже выключалась — [`SignalKind::ProtectionOffAgain`].
    pub fn record_protection_off(&mut self, url: &str, now: i64) -> Option<Signal> {
        let site = site_of(url);
        let prev = self.protection_off.insert(site.clone(), now);
        match prev {
            Some(p) if now - p <= PROTECTION_REPEAT_SECS => {
                Some(self.push(SignalKind::ProtectionOffAgain, site, now, ""))
            }
            _ => None,
        }
    }

    /// Страница `url` передана во внешний браузер по причине `reason_code`.
    pub fn record_open_in_other(&mut self, url: &str, reason_code: &str, now: i64) -> Signal {
        self.push(SignalKind::OpenInOther, site_of(url), now, reason_code)
    }

    /// Сигналы от старых к новым.
    #[allow(dead_code)] // потребитель — чип помощи (COMPAT-SIG-04 / COMPAT-PANEL)
    pub fn recent(&self) -> impl Iterator<Item = &Signal> {
        self.log.iter()
    }

    /// Сколько сигналов вида `kind` на сайте `url` есть в журнале.
    #[must_use]
    #[allow(dead_code)] // потребитель — ограничитель чипов (COMPAT-SIG-04)
    pub fn count_for(&self, kind: SignalKind, url: &str) -> usize {
        let site = site_of(url);
        self.log.iter().filter(|s| s.kind == kind && s.site == site).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://www.example.co.uk/page?q=1";

    #[test]
    fn three_reloads_in_window_make_a_signal() {
        let mut log = SignalLog::default();
        assert!(log.record_reload(URL, 100).is_none());
        assert!(log.record_reload(URL, 110).is_none());
        let s = log.record_reload(URL, 130).expect("третья за 30 с");
        assert_eq!(s.kind, SignalKind::ReloadBurst);
        assert_eq!(s.site, "example.co.uk");
        assert_eq!(s.kind.code(), "reload3");
    }

    #[test]
    fn slow_reloads_do_not_count() {
        let mut log = SignalLog::default();
        for t in [0, 20, 41, 62, 83] {
            assert!(log.record_reload(URL, t).is_none(), "t={t}");
        }
        assert_eq!(log.recent().count(), 0);
    }

    #[test]
    fn reloads_of_different_urls_are_counted_apart() {
        let mut log = SignalLog::default();
        assert!(log.record_reload("https://a.example/", 0).is_none());
        assert!(log.record_reload("https://b.example/", 1).is_none());
        assert!(log.record_reload("https://a.example/", 2).is_none());
        assert!(log.record_reload("https://b.example/", 3).is_none());
        assert!(log.record_reload("https://a.example/", 4).is_some());
    }

    #[test]
    fn counting_restarts_after_a_signal() {
        let mut log = SignalLog::default();
        for t in [0, 1, 2] {
            let _ = log.record_reload(URL, t);
        }
        assert!(log.record_reload(URL, 3).is_none());
        assert!(log.record_reload(URL, 4).is_none());
        assert!(log.record_reload(URL, 5).is_some());
        assert_eq!(log.recent().count(), 2);
    }

    #[test]
    fn second_protection_off_within_a_week_is_a_signal() {
        let mut log = SignalLog::default();
        assert!(log.record_protection_off("https://shop.example.com/", 0).is_none());
        let s = log
            .record_protection_off("https://example.com/cart", PROTECTION_REPEAT_SECS)
            .expect("повтор ровно через 7 суток");
        assert_eq!(s.kind, SignalKind::ProtectionOffAgain);
        assert_eq!(s.site, "example.com");
    }

    #[test]
    fn protection_off_after_a_week_is_first_again() {
        let mut log = SignalLog::default();
        assert!(log.record_protection_off("https://example.com/", 0).is_none());
        assert!(log.record_protection_off("https://example.com/", PROTECTION_REPEAT_SECS + 1).is_none());
        assert!(log.record_protection_off("https://other.example/", 5).is_none());
    }

    #[test]
    fn open_in_other_is_logged_with_reason_only() {
        let mut log = SignalLog::default();
        let s = log.record_open_in_other(URL, "drm", 7);
        assert_eq!(
            (s.kind, s.site.as_str(), s.detail.as_str()),
            (SignalKind::OpenInOther, "example.co.uk", "drm")
        );
        assert_eq!(log.count_for(SignalKind::OpenInOther, "https://example.co.uk/"), 1);
        assert_eq!(log.count_for(SignalKind::OpenInOther, "https://example.com/"), 0);
    }

    #[test]
    fn restored_protection_window_still_fires() {
        let mut log = SignalLog::restore(Vec::new(), vec![("example.com".to_owned(), 100)]);
        let s = log.record_protection_off("https://example.com/", 100 + 3 * 86_400);
        assert_eq!(s.map(|s| s.kind), Some(SignalKind::ProtectionOffAgain));
    }

    #[test]
    fn restore_keeps_the_newest_cap_signals_and_codes_round_trip() {
        let signals: Vec<Signal> = (0..(LOG_CAP as i64 + 5))
            .map(|at| Signal {
                kind: SignalKind::OpenInOther,
                site: "a.com".into(),
                at,
                detail: String::new(),
            })
            .collect();
        let log = SignalLog::restore(signals, Vec::new());
        assert_eq!(log.recent().count(), LOG_CAP);
        assert_eq!(log.recent().next().map(|s| s.at), Some(5));
        for k in [SignalKind::ReloadBurst, SignalKind::ProtectionOffAgain, SignalKind::OpenInOther] {
            assert_eq!(SignalKind::from_code(k.code()), Some(k));
        }
        assert_eq!(SignalKind::from_code("future_kind"), None);
    }

    #[test]
    fn log_is_bounded() {
        let mut log = SignalLog::default();
        for i in 0..(LOG_CAP as i64 + 20) {
            let _ = log.record_open_in_other(URL, "other", i);
        }
        assert_eq!(log.recent().count(), LOG_CAP);
        assert_eq!(log.recent().next().map(|s| s.at), Some(20));
    }
}
