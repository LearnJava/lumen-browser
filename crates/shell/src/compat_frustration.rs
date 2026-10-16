//! Детектор «сайт ведёт себя не так» по поведению и ограничитель подсказок
//! (COMPAT-FRUSTRATION; `docs/research/site-compat/report.md` §8.3, COMPAT-SIG-04/05).
//!
//! Чистая часть без окна, часов и ввода-вывода: время приходит аргументом
//! (unix-миллисекунды). Детектор ничего не отправляет и не хранит адресов —
//! только сайт (eTLD+1), время и счётчики. Результат — [`Chip`]: подсказка у
//! адресной строки, которая ведёт в панель «Сайт работает не так?»
//! (`crate::compat_panel`). Подключение к окну — `crate::lumen::frustration`.
//!
//! Четыре сигнала, каждого достаточно для чипа:
//! - 2 перезагрузки одного URL за 30 с;
//! - белый экран через 3 с после загрузки (DOM есть, нарисовано ноль);
//! - тот же антибот-вызов ≥3 раз за 60 с (решает `crate::gate_detect::GateLoop`);
//! - повторное падение страницы сайта за 10 с.
//!
//! Ограничитель ([`Limiter`]): ≤1 чип на сайт за 24 ч, ≤3 чипа за 24 ч всего,
//! «Не сейчас» — 7 суток тишины на сайте, 3 отказа подряд — автоматический чип
//! выключен на 30 суток (панель остаётся). Чип не показывается во время набора
//! (нажатие клавиши за последние 2 с) и не живёт на чужом сайте.

use std::collections::HashMap;

/// Окно перезагрузок, мс.
pub const RELOAD_WINDOW_MS: i64 = 30_000;
/// Сколько перезагрузок одного URL в окне дают чип.
pub const RELOAD_COUNT: usize = 2;
/// Через сколько после загрузки проверяется белый экран, мс.
pub const BLANK_DELAY_MS: i64 = 3_000;
/// Окно повторного падения, мс.
pub const CRASH_WINDOW_MS: i64 = 10_000;
/// Сколько узлов DOM делают страницу «содержательной» для проверки белого экрана
/// (то же число, что у журнала `--health-log`: `health_log::BROKEN_RENDER_DOM_MIN`).
pub const BLANK_DOM_MIN: usize = 20;
/// Тишина после нажатия клавиши: пока она не прошла, чип откладывается, мс.
pub const TYPING_GRACE_MS: i64 = 2_000;
/// Сколько подсказка ждёт подходящего момента, прежде чем устареть, мс.
pub const CANDIDATE_TTL_MS: i64 = 60_000;

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
/// Не чаще одного чипа на сайт за это время.
pub const SITE_COOLDOWN_MS: i64 = DAY_MS;
/// Не более трёх чипов всего за это время.
pub const DAY_LIMIT: usize = 3;
/// «Не сейчас»: тишина на сайте.
pub const SNOOZE_MS: i64 = 7 * DAY_MS;
/// Столько отказов подряд выключают автоматический чип.
pub const DISMISS_STREAK_OFF: u32 = 3;
/// Срок, на который автоматический чип выключен после серии отказов.
pub const AUTO_OFF_MS: i64 = 30 * DAY_MS;
/// Сколько сайтов помнит ограничитель (старые вытесняются).
const SITES_CAP: usize = 200;

/// Что именно вызвало подсказку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Две перезагрузки одного URL за 30 с.
    RepeatedReload,
    /// Через 3 с после загрузки ничего не нарисовано.
    BlankScreen,
    /// Антибот-вызов повторился ≥3 раз за минуту.
    AntibotLoop,
    /// Страница сайта упала дважды за 10 с.
    RepeatedCrash,
}

impl Trigger {
    /// Стабильный код (для stderr и тестов).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::RepeatedReload => "reload2",
            Self::BlankScreen => "blank_screen",
            Self::AntibotLoop => "antibot_loop",
            Self::RepeatedCrash => "repeat_crash",
        }
    }

    /// Короткий текст чипа: вопрос словами пользователя.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RepeatedReload => "Страница не грузится как надо?",
            Self::BlankScreen => "Пустая страница?",
            Self::AntibotLoop => "Сайт не пускает?",
            Self::RepeatedCrash => "Страница падает?",
        }
    }
}

/// Показанная (или готовая к показу) подсказка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// Сайт (eTLD+1), на котором чип живёт.
    pub site: String,
    /// Причина.
    pub trigger: Trigger,
}

/// Белый экран: DOM содержательный, а нарисованных единиц (текст, `<img>`,
/// `<canvas>`, `<video>`) ноль.
#[must_use]
pub fn is_blank(dom_nodes: usize, rendered_units: usize) -> bool {
    dom_nodes >= BLANK_DOM_MIN && rendered_units == 0
}

/// Память ограничителя: что и когда уже показывали. Переживает перезапуск
/// ([`Limiter::to_text`] / [`Limiter::from_text`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Limiter {
    /// Сайт → когда чип показан в последний раз.
    last_shown: HashMap<String, i64>,
    /// Сайт → до какого момента на нём тишина («Не сейчас»).
    snoozed_until: HashMap<String, i64>,
    /// Моменты показа за последние сутки.
    shown_at: Vec<i64>,
    /// Отказов подряд (клик по чипу обнуляет).
    streak: u32,
    /// Автоматический чип выключен до этого момента.
    off_until: i64,
}

impl Limiter {
    /// Можно ли сейчас показать чип для `site`.
    #[must_use]
    pub fn allows(&self, site: &str, now: i64) -> bool {
        if now < self.off_until {
            return false;
        }
        if self.snoozed_until.get(site).is_some_and(|&until| now < until) {
            return false;
        }
        if self.last_shown.get(site).is_some_and(|&at| now - at < SITE_COOLDOWN_MS) {
            return false;
        }
        self.shown_at.iter().filter(|&&at| now - at < DAY_MS).count() < DAY_LIMIT
    }

    fn record_shown(&mut self, site: &str, now: i64) {
        self.shown_at.retain(|&at| now - at < DAY_MS);
        self.shown_at.push(now);
        self.last_shown.insert(site.to_owned(), now);
        self.trim(now);
    }

    fn record_accepted(&mut self) {
        self.streak = 0;
    }

    fn record_dismissed(&mut self, site: &str, now: i64) {
        self.snoozed_until.insert(site.to_owned(), now + SNOOZE_MS);
        self.streak += 1;
        if self.streak >= DISMISS_STREAK_OFF {
            self.off_until = now + AUTO_OFF_MS;
            self.streak = 0;
        }
        self.trim(now);
    }

    /// Выбросить записи, которые уже ничего не запрещают, и держать размер в рамках.
    fn trim(&mut self, now: i64) {
        self.last_shown.retain(|_, &mut at| now - at < SITE_COOLDOWN_MS);
        self.snoozed_until.retain(|_, &mut until| now < until);
        self.shown_at.retain(|&at| now - at < DAY_MS);
        while self.snoozed_until.len() > SITES_CAP {
            let Some(oldest) = self.snoozed_until.iter().min_by_key(|(_, u)| **u).map(|(s, _)| s.clone()) else {
                break;
            };
            self.snoozed_until.remove(&oldest);
        }
    }

    /// Забыть сайт («очистить данные сайта»).
    pub fn forget_site(&mut self, site: &str) {
        self.last_shown.remove(site);
        self.snoozed_until.remove(site);
    }

    /// Сериализация в одну строку настроек: записи через `\n`, поля через `\t`.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = vec![format!("streak\t{}", self.streak), format!("off\t{}", self.off_until)];
        out.extend(self.shown_at.iter().map(|at| format!("day\t{at}")));
        out.extend(self.last_shown.iter().map(|(s, at)| format!("shown\t{s}\t{at}")));
        out.extend(self.snoozed_until.iter().map(|(s, u)| format!("snooze\t{s}\t{u}")));
        out.join("\n")
    }

    /// Обратное к [`Self::to_text`]; непонятные строки пропускаются.
    #[must_use]
    pub fn from_text(text: &str, now: i64) -> Self {
        let mut l = Self::default();
        for line in text.lines() {
            let mut f = line.split('\t');
            match (f.next(), f.next(), f.next()) {
                (Some("streak"), Some(n), None) => l.streak = n.parse().unwrap_or(0),
                (Some("off"), Some(t), None) => l.off_until = t.parse().unwrap_or(0),
                (Some("day"), Some(t), None) => l.shown_at.extend(t.parse::<i64>()),
                (Some("shown"), Some(s), Some(t)) => {
                    if let Ok(t) = t.parse() {
                        l.last_shown.insert(s.to_owned(), t);
                    }
                }
                (Some("snooze"), Some(s), Some(t)) => {
                    if let Ok(t) = t.parse() {
                        l.snoozed_until.insert(s.to_owned(), t);
                    }
                }
                _ => {}
            }
        }
        l.trim(now);
        l
    }
}

/// Результат хода времени.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// Чип появился или исчез — нужна перерисовка хрома.
    pub changed: bool,
    /// Когда проснуться в следующий раз (unix-мс), если что-то ждёт.
    pub next_wakeup_ms: Option<i64>,
}

/// Детектор: накапливает события окна и решает, когда показать чип.
#[derive(Debug, Default)]
pub struct Frustration {
    /// Моменты перезагрузок по полному URL (в пределах окна).
    reloads: HashMap<String, Vec<i64>>,
    /// Сайт → момент последнего падения.
    crashes: HashMap<String, i64>,
    /// Проверка белого экрана: сайт и момент проверки.
    blank_due: Option<(String, i64)>,
    /// Подсказка, ждущая подходящего момента, и когда она возникла.
    pending: Option<(Chip, i64)>,
    shown: Option<Chip>,
    last_key_ms: Option<i64>,
    limiter: Limiter,
}

impl Frustration {
    /// Детектор с памятью ограничителя, поднятой из настроек.
    #[must_use]
    pub fn with_limiter(limiter: Limiter) -> Self {
        Self { limiter, ..Self::default() }
    }

    /// Память ограничителя (для записи в настройки).
    #[must_use]
    pub fn limiter(&self) -> &Limiter {
        &self.limiter
    }

    /// Чип, который нужно показать сейчас.
    #[must_use]
    pub fn chip(&self) -> Option<&Chip> {
        self.shown.as_ref()
    }

    /// Нажата клавиша: чип не появится в ближайшие [`TYPING_GRACE_MS`].
    pub fn note_key(&mut self, now: i64) {
        self.last_key_ms = Some(now);
    }

    /// Пользовательская перезагрузка `url` (сайт `site`).
    pub fn on_reload(&mut self, url: &str, site: &str, now: i64) {
        self.reloads.retain(|_, ts| {
            ts.retain(|&t| now - t <= RELOAD_WINDOW_MS);
            !ts.is_empty()
        });
        let ts = self.reloads.entry(url.to_owned()).or_default();
        ts.push(now);
        if ts.len() >= RELOAD_COUNT {
            self.reloads.remove(url);
            self.raise(site, Trigger::RepeatedReload, now);
        }
    }

    /// Страница сайта `site` установлена: через [`BLANK_DELAY_MS`] проверить белый экран.
    pub fn on_page_installed(&mut self, site: &str, now: i64) {
        self.blank_due = Some((site.to_owned(), now + BLANK_DELAY_MS));
    }

    /// Антибот-вызов сайта повторился ≥3 раз за минуту.
    pub fn on_antibot_loop(&mut self, site: &str, now: i64) {
        self.raise(site, Trigger::AntibotLoop, now);
    }

    /// Страница сайта упала; второе падение за [`CRASH_WINDOW_MS`] даёт чип.
    pub fn on_crash(&mut self, site: &str, now: i64) {
        self.crashes.retain(|_, &mut at| now - at <= CRASH_WINDOW_MS);
        if self.crashes.insert(site.to_owned(), now).is_some() {
            self.crashes.remove(site);
            self.raise(site, Trigger::RepeatedCrash, now);
        }
    }

    /// Пользователь сам открыл панель или выполнил её шаг: чип больше не нужен,
    /// счётчики сайта начинаются заново.
    pub fn on_panel_used(&mut self, site: &str) {
        self.reloads.retain(|url, _| crate::compat_signals::site_of(url) != site);
        self.crashes.remove(site);
        self.pending = None;
        if self.blank_due.as_ref().is_some_and(|(s, _)| s == site) {
            self.blank_due = None;
        }
    }

    fn raise(&mut self, site: &str, trigger: Trigger, now: i64) {
        if self.shown.is_some() || self.pending.is_some() || !self.limiter.allows(site, now) {
            return;
        }
        self.pending = Some((Chip { site: site.to_owned(), trigger }, now));
    }

    /// Ход времени. `site` — сайт текущей страницы (пусто, если страница не
    /// http(s)); `is_blank` вычисляется только когда подошёл срок проверки.
    pub fn tick(&mut self, now: i64, site: &str, is_blank: impl FnOnce() -> bool) -> Tick {
        let mut changed = false;
        if let Some((due_site, due)) = self.blank_due.clone()
            && now >= due
        {
            self.blank_due = None;
            if due_site == site && is_blank() {
                self.raise(site, Trigger::BlankScreen, now);
            }
        }
        if self.shown.as_ref().is_some_and(|c| c.site != site) {
            self.shown = None;
            changed = true;
        }
        if let Some((chip, since)) = &self.pending {
            if chip.site != site || now - since > CANDIDATE_TTL_MS {
                self.pending = None;
            } else if !self.typing(now) {
                let chip = chip.clone();
                self.limiter.record_shown(&chip.site, now);
                self.shown = Some(chip);
                self.pending = None;
                changed = true;
            }
        }
        Tick { changed, next_wakeup_ms: self.next_wakeup() }
    }

    fn typing(&self, now: i64) -> bool {
        self.last_key_ms.is_some_and(|k| now - k < TYPING_GRACE_MS)
    }

    fn next_wakeup(&self) -> Option<i64> {
        let blank = self.blank_due.as_ref().map(|(_, due)| *due);
        let pending = self.pending.as_ref().map(|(_, since)| {
            let typing_end = self.last_key_ms.map_or(0, |k| k + TYPING_GRACE_MS);
            typing_end.min(since + CANDIDATE_TTL_MS).max(*since)
        });
        [blank, pending].into_iter().flatten().min()
    }

    /// Клик по чипу: открывается панель, серия отказов прерывается.
    pub fn accept(&mut self) {
        if self.shown.take().is_some() {
            self.limiter.record_accepted();
        }
    }

    /// «Не сейчас»: тишина на сайте 7 суток; третий отказ подряд выключает
    /// автоматический чип на 30 суток.
    pub fn dismiss(&mut self, now: i64) {
        if let Some(chip) = self.shown.take() {
            self.limiter.record_dismissed(&chip.site, now);
        }
    }

    /// «Очистить данные сайта».
    pub fn forget_site(&mut self, site: &str) {
        self.on_panel_used(site);
        self.limiter.forget_site(site);
        if self.shown.as_ref().is_some_and(|c| c.site == site) {
            self.shown = None;
        }
    }

    /// «Очистить историю».
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://example.com/a";
    const SITE: &str = "example.com";

    fn shown(f: &mut Frustration, now: i64) -> Option<Chip> {
        f.tick(now, SITE, || false);
        f.chip().cloned()
    }

    #[test]
    fn two_reloads_in_30s_raise_chip() {
        let mut f = Frustration::default();
        f.on_reload(URL, SITE, 0);
        assert!(shown(&mut f, 0).is_none());
        f.on_reload(URL, SITE, 29_000);
        let chip = shown(&mut f, 29_000).expect("chip");
        assert_eq!(chip.trigger, Trigger::RepeatedReload);
        assert_eq!(chip.site, SITE);
    }

    #[test]
    fn slow_reloads_are_not_frustration() {
        let mut f = Frustration::default();
        for i in 0..10 {
            f.on_reload(URL, SITE, i * 31_000);
            assert!(shown(&mut f, i * 31_000).is_none());
        }
    }

    #[test]
    fn reloads_of_different_urls_do_not_add_up() {
        let mut f = Frustration::default();
        f.on_reload("https://example.com/a", SITE, 0);
        f.on_reload("https://example.com/b", SITE, 1_000);
        assert!(shown(&mut f, 1_000).is_none());
    }

    #[test]
    fn blank_screen_checked_three_seconds_after_load() {
        let mut f = Frustration::default();
        f.on_page_installed(SITE, 10_000);
        let t = f.tick(12_999, SITE, || panic!("too early"));
        assert_eq!(t.next_wakeup_ms, Some(13_000));
        assert!(f.chip().is_none());
        let t = f.tick(13_000, SITE, || true);
        assert!(t.changed);
        assert_eq!(f.chip().map(|c| c.trigger), Some(Trigger::BlankScreen));
    }

    #[test]
    fn healthy_or_foreign_page_gives_no_blank_chip() {
        let mut f = Frustration::default();
        f.on_page_installed(SITE, 0);
        f.tick(3_000, SITE, || false);
        assert!(f.chip().is_none());
        // Ушли на другой сайт раньше срока: предикат даже не вызывается.
        f.on_page_installed(SITE, 0);
        f.tick(3_000, "other.org", || panic!("other page"));
        assert!(f.chip().is_none());
    }

    #[test]
    fn blank_predicate() {
        assert!(is_blank(20, 0));
        assert!(!is_blank(19, 0));
        assert!(!is_blank(500, 1));
    }

    #[test]
    fn repeated_crash_within_10s() {
        let mut f = Frustration::default();
        f.on_crash(SITE, 0);
        assert!(shown(&mut f, 0).is_none());
        f.on_crash(SITE, 10_000);
        assert_eq!(shown(&mut f, 10_000).map(|c| c.trigger), Some(Trigger::RepeatedCrash));
    }

    #[test]
    fn crashes_far_apart_are_independent() {
        let mut f = Frustration::default();
        f.on_crash(SITE, 0);
        f.on_crash(SITE, 10_001);
        assert!(shown(&mut f, 10_001).is_none());
    }

    #[test]
    fn antibot_loop_raises_chip() {
        let mut f = Frustration::default();
        f.on_antibot_loop(SITE, 5);
        assert_eq!(shown(&mut f, 5).map(|c| c.trigger), Some(Trigger::AntibotLoop));
    }

    #[test]
    fn chip_waits_for_typing_pause() {
        let mut f = Frustration::default();
        f.note_key(1_000);
        f.on_antibot_loop(SITE, 1_500);
        let t = f.tick(1_500, SITE, || false);
        assert!(f.chip().is_none());
        assert_eq!(t.next_wakeup_ms, Some(3_000));
        let t = f.tick(3_000, SITE, || false);
        assert!(t.changed);
        assert!(f.chip().is_some());
    }

    #[test]
    fn stale_candidate_is_dropped() {
        let mut f = Frustration::default();
        f.on_antibot_loop(SITE, 0);
        f.note_key(CANDIDATE_TTL_MS);
        f.tick(CANDIDATE_TTL_MS + 1, SITE, || false);
        assert!(f.chip().is_none());
        assert_eq!(f.tick(CANDIDATE_TTL_MS + 5_000, SITE, || false).next_wakeup_ms, None);
    }

    #[test]
    fn chip_disappears_on_other_site() {
        let mut f = Frustration::default();
        f.on_antibot_loop(SITE, 0);
        f.tick(0, SITE, || false);
        let t = f.tick(1, "other.org", || false);
        assert!(t.changed);
        assert!(f.chip().is_none());
    }

    #[test]
    fn one_chip_per_site_per_day() {
        let mut f = Frustration::default();
        f.on_antibot_loop(SITE, 0);
        f.tick(0, SITE, || false);
        f.accept();
        f.on_antibot_loop(SITE, 1_000);
        assert!(shown(&mut f, 1_000).is_none());
        f.on_antibot_loop(SITE, DAY_MS);
        assert!(shown(&mut f, DAY_MS).is_some());
    }

    #[test]
    fn at_most_three_chips_a_day() {
        let mut f = Frustration::default();
        for (i, site) in ["a.com", "b.com", "c.com", "d.com"].iter().enumerate() {
            let now = i as i64 * 1_000;
            f.on_antibot_loop(site, now);
            f.tick(now, site, || false);
            assert_eq!(f.chip().is_some(), i < 3, "chip #{i}");
            f.accept();
        }
        f.on_antibot_loop("e.com", DAY_MS + 1_000);
        f.tick(DAY_MS + 1_000, "e.com", || false);
        assert!(f.chip().is_some());
    }

    #[test]
    fn not_now_silences_site_for_a_week() {
        let mut f = Frustration::default();
        f.on_antibot_loop(SITE, 0);
        f.tick(0, SITE, || false);
        f.dismiss(10);
        assert!(f.chip().is_none());
        f.on_antibot_loop(SITE, 6 * DAY_MS);
        assert!(shown(&mut f, 6 * DAY_MS).is_none());
        f.on_antibot_loop(SITE, 7 * DAY_MS + 10);
        assert!(shown(&mut f, 7 * DAY_MS + 10).is_some());
    }

    #[test]
    fn three_dismissals_in_a_row_switch_auto_chip_off_for_30_days() {
        let mut f = Frustration::default();
        for (i, site) in ["a.com", "b.com", "c.com"].iter().enumerate() {
            let now = i as i64 * 1_000;
            f.on_antibot_loop(site, now);
            f.tick(now, site, || false);
            f.dismiss(now);
        }
        // Три отказа за сутки: ещё и лимит дня, поэтому проверяем после суток.
        let later = DAY_MS + 5_000;
        f.on_antibot_loop("d.com", later);
        assert!(!f.tick(later, "d.com", || false).changed);
        assert!(f.chip().is_none());
        let after = 3 * 1_000 + AUTO_OFF_MS;
        f.on_antibot_loop("d.com", after);
        f.tick(after, "d.com", || false);
        assert!(f.chip().is_some());
    }

    #[test]
    fn accepting_resets_dismiss_streak() {
        let mut f = Frustration::default();
        for (i, site) in ["a.com", "b.com"].iter().enumerate() {
            let now = i as i64;
            f.on_antibot_loop(site, now);
            f.tick(now, site, || false);
            f.dismiss(now);
        }
        f.on_antibot_loop("c.com", 5);
        f.tick(5, "c.com", || false);
        f.accept();
        f.on_antibot_loop("d.com", DAY_MS);
        f.tick(DAY_MS, "d.com", || false);
        f.dismiss(DAY_MS);
        let later = 2 * DAY_MS;
        f.on_antibot_loop("e.com", later);
        f.tick(later, "e.com", || false);
        assert!(f.chip().is_some(), "streak was 1, not 3");
    }

    #[test]
    fn panel_use_clears_counters() {
        let mut f = Frustration::default();
        f.on_reload(URL, SITE, 0);
        f.on_panel_used(SITE);
        f.on_reload(URL, SITE, 1_000);
        assert!(shown(&mut f, 1_000).is_none());
    }

    #[test]
    fn limiter_text_roundtrip() {
        let mut f = Frustration::default();
        f.on_antibot_loop(SITE, 100);
        f.tick(100, SITE, || false);
        f.dismiss(200);
        let text = f.limiter().to_text();
        let back = Limiter::from_text(&text, 300);
        assert_eq!(&back, f.limiter());
        assert!(!back.allows(SITE, 300));
        assert!(Limiter::from_text("мусор\n\nday\tне число", 0).allows(SITE, 0));
    }

    #[test]
    fn normal_browsing_session_stays_quiet() {
        // Типичная сессия: загрузки, единичные обновления, нормальные страницы.
        let mut f = Frustration::default();
        for i in 0..50 {
            let now = i * 20_000;
            f.on_page_installed(SITE, now);
            if i % 5 == 0 {
                f.on_reload(URL, SITE, now + 500);
            }
            f.tick(now + BLANK_DELAY_MS, SITE, || is_blank(300, 40));
            assert!(f.chip().is_none(), "false positive at {i}");
        }
    }
}
