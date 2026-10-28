use crate::Lumen;
use std::time::{Duration, Instant};

/// LOAD-2: сколько страница ждёт таблицы стилей из `<head>` до первого
/// черновика. Chrome тоже блокирует отрисовку на CSS, но один медленный
/// файл не должен держать экран пустым до `FETCH_READ_TIMEOUT` (60 с).
pub(crate) const STREAM_CSS_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct CssPending {
    generation: u64,
    count: usize,
    /// Момент первого `add` навигации — от него отсчитывается дедлайн.
    first_add: Option<Instant>,
    /// Дедлайн уже отдан `stream_css_pending_expired` (черновик нарисован).
    expired_taken: bool,
}

/// BUG-1225: число render-blocking таблиц стилей из `<head>`, для которых
/// preload-сканер уже запустил загрузку, а `CssLoaded` ещё не обработан
/// event loop-ом. Пока оно ненулевое, streaming-кадр не публикуется — иначе
/// на экране виден сырой HTML без CSS. Каждая посчитанная таблица шлёт
/// `CssLoaded` ровно один раз (при сбое — пустой таблицей), поэтому счётчик
/// не зависает; а если файл долго качается, через [`STREAM_CSS_DEADLINE`]
/// после первого `add` ожидание снимается (LOAD-2).
static STREAM_CSS_PENDING: std::sync::Mutex<CssPending> = std::sync::Mutex::new(CssPending {
    generation: 0,
    count: 0,
    first_add: None,
    expired_taken: false,
});

fn lock() -> std::sync::MutexGuard<'static, CssPending> {
    STREAM_CSS_PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn stream_css_pending_add(generation: u64) {
    stream_css_pending_add_at(generation, Instant::now());
}

fn stream_css_pending_add_at(generation: u64, now: Instant) {
    let mut g = lock();
    if g.generation != generation {
        *g = CssPending { generation, count: 0, first_add: None, expired_taken: false };
    }
    g.first_add.get_or_insert(now);
    g.count += 1;
}

pub(crate) fn stream_css_pending_done(generation: u64) {
    let mut g = lock();
    if g.generation == generation {
        g.count = g.count.saturating_sub(1);
    }
}

/// `true`, пока для навигации `generation` ждут таблицы стилей (см. выше).
pub(crate) fn stream_css_pending(generation: u64) -> bool {
    stream_css_pending_at(generation, Instant::now())
}

pub(crate) fn stream_css_pending_at(generation: u64, now: Instant) -> bool {
    let g = lock();
    g.generation == generation && g.count > 0 && !deadline_passed(&g, now)
}

fn deadline_passed(g: &CssPending, now: Instant) -> bool {
    g.first_add.is_some_and(|t| now >= t + STREAM_CSS_DEADLINE)
}

/// Когда истекает ожидание стилей навигации `generation`; `None`, если
/// ждать уже нечего (всё пришло, дедлайн отдан или навигация чужая).
pub(crate) fn stream_css_pending_deadline(generation: u64) -> Option<Instant> {
    let g = lock();
    if g.generation != generation || g.count == 0 || g.expired_taken {
        return None;
    }
    g.first_add.map(|t| t + STREAM_CSS_DEADLINE)
}

/// `true` ровно один раз, когда дедлайн ожидания стилей `generation`
/// наступил, а таблицы ещё в пути: пора рисовать черновик без них.
pub(crate) fn stream_css_pending_expired(generation: u64) -> bool {
    stream_css_pending_expired_at(generation, Instant::now())
}

fn stream_css_pending_expired_at(generation: u64, now: Instant) -> bool {
    let mut g = lock();
    if g.generation != generation || g.count == 0 || g.expired_taken || !deadline_passed(&g, now) {
        return false;
    }
    g.expired_taken = true;
    true
}

impl Lumen {
    /// LOAD-2: нарисовать черновик из накопленного streaming-DOM. `false`,
    /// если рисовать не из чего (HTML ещё не пришёл) или стили ещё ждут.
    pub(crate) fn paint_stream_draft(&mut self, generation: u64) -> bool {
        if stream_css_pending(generation) {
            return false;
        }
        let Some(builder) = self.stream_builder.as_ref() else { return false };
        // Клонируем снапшот для layout — builder остаётся живым.
        let doc_snap = builder.as_doc().clone();
        self.paint_partial_dom(&doc_snap);
        self.stream_last_paint = Instant::now();
        true
    }

    /// LOAD-2: вызывается из `about_to_wait`. По наступлении дедлайна рисует
    /// черновик без ещё не пришедших таблиц; возвращает момент, до которого
    /// нужно разбудить цикл (если дедлайн ещё впереди).
    pub(crate) fn stream_css_deadline_tick(&mut self) -> Option<Instant> {
        let generation = self.load_generation;
        if stream_css_pending_expired(generation) {
            self.paint_stream_draft(generation);
            return None;
        }
        stream_css_pending_deadline(generation)
    }
}

/// Счётчик глобальный (один слот на процесс), поэтому тесты, которые его
/// трогают, идут строго по очереди.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_releases_pending_once() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let g = 9_100_001;
        let t0 = Instant::now();
        stream_css_pending_add_at(g, t0);
        stream_css_pending_add_at(g, t0 + Duration::from_millis(500));
        assert!(stream_css_pending_at(g, t0 + Duration::from_millis(1999)));
        assert_eq!(stream_css_pending_deadline(g), Some(t0 + STREAM_CSS_DEADLINE));
        assert!(!stream_css_pending_expired_at(g, t0 + Duration::from_millis(1999)));
        // Дедлайн отсчитывается от первого add, а не от последнего.
        let late = t0 + STREAM_CSS_DEADLINE;
        assert!(!stream_css_pending_at(g, late), "после дедлайна кадры идут");
        assert!(stream_css_pending_expired_at(g, late), "черновик по дедлайну — один раз");
        assert!(!stream_css_pending_expired_at(g, late), "повторно не отдаётся");
        assert_eq!(stream_css_pending_deadline(g), None);
        stream_css_pending_done(g);
        stream_css_pending_done(g);
    }

    #[test]
    fn deadline_not_fired_when_all_sheets_arrived() {
        let _serial = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let g = 9_100_002;
        let t0 = Instant::now();
        stream_css_pending_add_at(g, t0);
        stream_css_pending_done(g);
        assert_eq!(stream_css_pending_deadline(g), None);
        assert!(!stream_css_pending_expired_at(g, t0 + Duration::from_secs(5)));
        // Чужая навигация не видит дедлайн.
        assert_eq!(stream_css_pending_deadline(g + 1), None);
    }
}
