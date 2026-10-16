//! Секция «Сайт работает не так?» во всплывающем окне сайта (COMPAT-PANEL,
//! `docs/research/site-compat/report.md` §8.2): лесенка шагов по возрастанию
//! цены и вопрос «Заработало?» через 3 с после шага.
//!
//! Здесь только состояние и правила (без окна и сети); действия — в
//! `crate::lumen::fix_panel`. Модель для chrome собирает [`CompatPanel::model`].
//!
//! Шаги v1: «без кэша», «сбросить данные сайта» (с предупреждением), «открыть
//! в другом браузере». Шаги «без защит на вкладку» (COMPAT-PANEL-TAB-SHIELDS) и
//! «как Chrome» (COMPAT-ID) появятся в лесенке вместе со своими механизмами —
//! кнопок-заглушек нет.

use std::time::{Duration, Instant};

use lumen_chrome::ChromeFixPanelModel;

/// Пауза между окончанием загрузки и вопросом «Заработало?».
pub const ASK_DELAY: Duration = Duration::from_secs(3);

/// Шаг лесенки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Перезагрузка в обход HTTP-кэша.
    NoCache,
    /// Сброс кэша, cookie, хранилищ, service worker и разрешений сайта.
    ResetSite,
    /// Одноразовая передача страницы во внешний браузер.
    OtherBrowser,
}

impl Step {
    /// Шаги по возрастанию цены; порядок задаёт `ChromeFixPanelModel::tried`.
    pub const LADDER: [Step; 3] = [Step::NoCache, Step::ResetSite, Step::OtherBrowser];

    /// Стабильный код: поле `fixed_by` журнала сигналов и будущего отчёта.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Step::NoCache => "nocache",
            Step::ResetSite => "reset_site",
            Step::OtherBrowser => "other_browser",
        }
    }

    /// Название шага в вопросе пользователю.
    fn title(self) -> &'static str {
        match self {
            Step::NoCache => "Обновить без кэша",
            Step::ResetSite => "Сбросить данные сайта",
            Step::OtherBrowser => "Открыть в другом браузере",
        }
    }

    fn index(self) -> usize {
        Self::LADDER.iter().position(|&s| s == self).unwrap_or(0)
    }
}

/// Шаг выполнен, ждём окончания загрузки и паузы, чтобы спросить.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Watch {
    step: Step,
    /// Сайт (eTLD+1), на котором шаг сделан: на другом вопрос не задаётся.
    site: String,
    /// Идентификатор браузера, куда передана страница (только `OtherBrowser`).
    browser_id: Option<String>,
    /// Когда спрашивать; `None` — ждём конца загрузки.
    ask_at: Option<Instant>,
}

/// Вопрос «Заработало?», ожидающий ответа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// Шаг, после которого задан вопрос.
    pub step: Step,
    /// Браузер, куда ушла страница (для правила «открывать там 30 дней»).
    pub browser_id: Option<String>,
}

/// Состояние секции.
#[derive(Debug, Default)]
pub struct CompatPanel {
    /// Показано предупреждение о сбросе вместо лесенки.
    confirm_reset: bool,
    /// Сайт, для которого ведётся `tried`.
    tried_site: String,
    /// Шаги, уже испробованные на `tried_site`.
    tried: Vec<Step>,
    watch: Option<Watch>,
    asking: Option<Asked>,
    /// Что показано под секцией (результат шага).
    note: String,
}

impl CompatPanel {
    /// Окно сайта открыто для `site`: на другом сайте счёт попыток начинается заново.
    pub fn open_for(&mut self, site: &str) {
        self.confirm_reset = false;
        if self.tried_site != site {
            self.tried_site = site.to_owned();
            self.tried.clear();
            self.watch = None;
            self.asking = None;
            self.note.clear();
        }
    }

    /// Окно закрыто: предупреждение и неотвеченный вопрос снимаются.
    pub fn close(&mut self) {
        self.confirm_reset = false;
        self.asking = None;
    }

    /// Первое нажатие «Сбросить данные сайта…» — показать предупреждение.
    pub fn ask_reset_confirmation(&mut self) {
        self.confirm_reset = true;
        self.note.clear();
    }

    /// «Отмена» в предупреждении.
    pub fn cancel_reset(&mut self) {
        self.confirm_reset = false;
    }

    /// Предупреждение показано?
    #[must_use]
    pub fn is_confirming_reset(&self) -> bool {
        self.confirm_reset
    }

    /// Шаг выполнен. `after_load` — шаг перезагружает страницу, вопрос ждёт
    /// конца загрузки (иначе — только паузы, как после передачи в другой браузер).
    pub fn step_done(
        &mut self,
        step: Step,
        site: &str,
        after_load: bool,
        browser_id: Option<String>,
        now: Instant,
    ) {
        self.confirm_reset = false;
        self.asking = None;
        if self.tried_site != site {
            self.tried_site = site.to_owned();
            self.tried.clear();
        }
        if !self.tried.contains(&step) {
            self.tried.push(step);
        }
        self.note.clear();
        let ask_at = (!after_load).then(|| now + ASK_DELAY);
        self.watch = Some(Watch { step, site: site.to_owned(), browser_id, ask_at });
    }

    /// Текст под секцией (результат шага, отказ, пояснение).
    pub fn set_note(&mut self, note: &str) {
        self.note = note.to_owned();
    }

    /// Страница `site` загружена: если ждали именно её, запускаем отсчёт 3 с.
    pub fn load_done(&mut self, site: &str, now: Instant) {
        if let Some(w) = self.watch.as_mut()
            && w.ask_at.is_none()
            && w.site == site
        {
            w.ask_at = Some(now + ASK_DELAY);
        }
    }

    /// Ход времени. Вернёт `true`, когда вопрос только что появился (окно надо
    /// открыть). `current_site` — сайт открытой вкладки: на чужом вопрос
    /// снимается молча.
    pub fn tick(&mut self, now: Instant, current_site: &str) -> bool {
        let due = self.watch.as_ref().is_some_and(|w| w.ask_at.is_some_and(|at| now >= at));
        if !due {
            return false;
        }
        let Some(w) = self.watch.take() else { return false };
        if w.site != current_site {
            return false;
        }
        self.asking = Some(Asked { step: w.step, browser_id: w.browser_id });
        true
    }

    /// Ближайший момент, когда `tick` что-то изменит.
    #[must_use]
    pub fn next_wakeup(&self) -> Option<Instant> {
        self.watch.as_ref().and_then(|w| w.ask_at)
    }

    /// Ответ на вопрос: вернёт шаг, если он был задан.
    pub fn answer(&mut self) -> Option<Asked> {
        self.asking.take()
    }

    /// Вопрос ждёт ответа?
    #[cfg(test)]
    #[must_use]
    pub fn is_asking(&self) -> bool {
        self.asking.is_some()
    }

    /// Снимок для chrome. `steps_enabled` — на странице шаги применимы.
    #[must_use]
    pub fn model(&self, steps_enabled: bool) -> ChromeFixPanelModel {
        let mut tried = [false; 3];
        for s in &self.tried {
            tried[s.index()] = true;
        }
        ChromeFixPanelModel {
            steps_enabled,
            confirm_reset: self.confirm_reset,
            asking: self.asking.as_ref().map(|a| format!("Заработало после шага «{}»?", a.step.title())),
            tried,
            note: if steps_enabled {
                self.note.clone()
            } else {
                "Для этой страницы шагов нет: они работают на http(s)-сайтах.".to_owned()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn ladder_codes_are_distinct_and_ordered() {
        let codes: Vec<_> = Step::LADDER.iter().map(|s| s.code()).collect();
        assert_eq!(codes, ["nocache", "reset_site", "other_browser"]);
        for (i, s) in Step::LADDER.iter().enumerate() {
            assert_eq!(s.index(), i);
        }
    }

    #[test]
    fn question_comes_three_seconds_after_load() {
        let mut p = CompatPanel::default();
        let now = t0();
        p.open_for("example.com");
        p.step_done(Step::NoCache, "example.com", true, None, now);
        assert_eq!(p.next_wakeup(), None, "пока страница грузится, срока нет");
        assert!(!p.tick(now + Duration::from_secs(60), "example.com"));
        p.load_done("example.com", now);
        assert_eq!(p.next_wakeup(), Some(now + ASK_DELAY));
        assert!(!p.tick(now + Duration::from_millis(2999), "example.com"));
        assert!(p.tick(now + ASK_DELAY, "example.com"));
        assert!(p.is_asking());
        assert_eq!(p.next_wakeup(), None);
        let asked = p.answer().expect("question was asked");
        assert_eq!(asked.step, Step::NoCache);
        assert!(!p.is_asking());
    }

    #[test]
    fn other_browser_asks_without_waiting_for_a_load() {
        let mut p = CompatPanel::default();
        let now = t0();
        p.step_done(Step::OtherBrowser, "example.com", false, Some("edge".into()), now);
        assert!(p.tick(now + ASK_DELAY, "example.com"));
        assert_eq!(p.answer().and_then(|a| a.browser_id).as_deref(), Some("edge"));
    }

    #[test]
    fn other_site_gets_no_question() {
        let mut p = CompatPanel::default();
        let now = t0();
        p.step_done(Step::NoCache, "a.example", true, None, now);
        p.load_done("b.example", now);
        assert_eq!(p.next_wakeup(), None, "загрузка чужого сайта не запускает отсчёт");
        p.load_done("a.example", now);
        assert!(!p.tick(now + ASK_DELAY, "b.example"), "пользователь ушёл на другой сайт");
        assert!(!p.is_asking());
        assert_eq!(p.next_wakeup(), None);
    }

    #[test]
    fn closing_drops_a_pending_question_and_confirmation() {
        let mut p = CompatPanel::default();
        let now = t0();
        p.step_done(Step::OtherBrowser, "a.example", false, None, now);
        assert!(p.tick(now + ASK_DELAY, "a.example"));
        p.ask_reset_confirmation();
        p.close();
        assert!(!p.is_asking());
        assert!(!p.is_confirming_reset());
    }

    #[test]
    fn tried_steps_reset_on_another_site() {
        let mut p = CompatPanel::default();
        p.open_for("a.example");
        p.step_done(Step::NoCache, "a.example", true, None, t0());
        assert_eq!(p.model(true).tried, [true, false, false]);
        p.open_for("a.example");
        assert_eq!(p.model(true).tried, [true, false, false], "повторное открытие на том же сайте помнит");
        p.open_for("b.example");
        assert_eq!(p.model(true).tried, [false, false, false]);
    }

    #[test]
    fn model_explains_pages_without_steps() {
        let p = CompatPanel::default();
        let m = p.model(false);
        assert!(!m.steps_enabled);
        assert!(m.note.contains("http"));
        assert!(p.model(true).note.is_empty());
    }

    #[test]
    fn question_names_the_step() {
        let mut p = CompatPanel::default();
        let now = t0();
        p.step_done(Step::ResetSite, "a.example", false, None, now);
        assert!(p.tick(now + ASK_DELAY, "a.example"));
        let q = p.model(true).asking.expect("asking");
        assert!(q.contains("Сбросить данные сайта"), "{q}");
    }
}
