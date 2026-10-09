//! UX-CRASH: вкладка переживает панику своего конвейера.
//!
//! `guard_event` оборачивает обработчик события окна, `poll_engine_crash`
//! забирает падения движкового потока; оба приводят к
//! [`Lumen::on_page_crashed`] — экрану «Страница упала» вместо смерти процесса.

use crate::*;

impl Lumen {
    /// Исполняет `f` под `catch_unwind`; паника превращается в экран падения.
    pub(crate) fn guard_event(&mut self, what: &str, f: impl FnOnce(&mut Self)) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self)));
        if let Err(payload) = result {
            let detail = format!("{what}: {}", crash_page::panic_text(payload.as_ref()));
            self.on_page_crashed(&detail);
        }
    }

    /// Забирает падение задания движкового потока (если было).
    pub(crate) fn poll_engine_crash(&mut self) {
        if let Some(detail) = self.engine_thread.as_ref().and_then(|e| e.take_crash()) {
            self.on_page_crashed(&detail);
        }
    }

    /// Заменяет упавшую страницу экраном падения (без записи в историю);
    /// F5 и кнопка возвращают исходный источник ([`Self::restore_crashed_source`]).
    pub(crate) fn on_page_crashed(&mut self, detail: &str) {
        eprintln!("[crash] страница упала: {detail}");
        let showing_crash = self.crashed.as_ref().is_some_and(|(html, _)| {
            matches!(&self.source, PageSource::Static { html: h, .. } if h == html)
        });
        if showing_crash {
            // Падает уже сам экран падения — второй раз его не строим.
            return;
        }
        let url = self.current_display_url().to_owned();
        let original = self.source.clone();
        let crash = crash_page::crash_source(&url, detail);
        let PageSource::Static { html, .. } = &crash else { return };
        let html = html.clone();
        self.source = crash;
        let reloaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.reload()));
        if reloaded.is_err() {
            eprintln!("[crash] показать экран падения не удалось");
        }
        self.crashed = Some((html, original));
    }

    /// Если сейчас показан экран падения — возвращает в `source` исходную
    /// страницу, чтобы перезагрузка загрузила её, а не экран снова.
    pub(crate) fn restore_crashed_source(&mut self) {
        if let Some((html, original)) = self.crashed.take() {
            if matches!(&self.source, PageSource::Static { html: h, .. } if *h == html) {
                self.source = original;
            } else {
                self.crashed = Some((html, original));
            }
        }
    }
}
