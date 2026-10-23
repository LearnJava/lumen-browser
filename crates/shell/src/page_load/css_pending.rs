/// BUG-1225: число render-blocking таблиц стилей из `<head>`, для которых
/// preload-сканер уже запустил загрузку, а `CssLoaded` ещё не обработан
/// event loop-ом: `(generation, счётчик)`. Пока оно ненулевое, streaming-кадр
/// не публикуется — иначе на экране виден сырой HTML без CSS. Каждая
/// посчитанная таблица шлёт `CssLoaded` ровно один раз (при сбое — пустой
/// таблицей), поэтому счётчик не зависает.
static STREAM_CSS_PENDING: std::sync::Mutex<(u64, usize)> = std::sync::Mutex::new((0, 0));

pub(crate) fn stream_css_pending_add(generation: u64) {
    let mut g = STREAM_CSS_PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if g.0 != generation {
        *g = (generation, 0);
    }
    g.1 += 1;
}

pub(crate) fn stream_css_pending_done(generation: u64) {
    let mut g = STREAM_CSS_PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if g.0 == generation {
        g.1 = g.1.saturating_sub(1);
    }
}

/// `true`, пока для навигации `generation` ждут таблицы стилей (см. выше).
pub(crate) fn stream_css_pending(generation: u64) -> bool {
    let g = STREAM_CSS_PENDING.lock().unwrap_or_else(|e| e.into_inner());
    g.0 == generation && g.1 > 0
}
