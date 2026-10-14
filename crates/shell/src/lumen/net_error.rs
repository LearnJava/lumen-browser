//! COMPAT-ERRORS: сбой загрузки главного документа показывается страницей
//! ошибки, а не прежней страницей, оставшейся на экране.
//!
//! Механика та же, что у экрана падения (`crash.rs`): источник вкладки
//! подменяется статической страницей, а исходный запоминается в
//! [`Lumen::crashed`] — F5 ([`Lumen::reload`]) возвращает его и повторяет
//! загрузку, а не перерисовывает ошибку.

use lumen_core::net_error::NetErrorCode;

use crate::net_error_page::AutoRetry;
use crate::*;

/// Дольше этой паузы между двумя страницами ошибки одного адреса счёт
/// автоповторов начинается заново: самая длинная пауза повтора — 45 с.
const RETRY_STREAK_WINDOW: std::time::Duration = std::time::Duration::from_secs(120);

/// Номер попытки (с нуля) для показа страницы ошибки на `url`.
fn retry_attempt(prev: Option<&(String, usize, std::time::Instant)>, url: &str, now: std::time::Instant) -> usize {
    match prev {
        Some((u, n, at)) if u == url && now.saturating_duration_since(*at) <= RETRY_STREAK_WINDOW => *n,
        _ => 0,
    }
}

impl Lumen {
    /// Заменяет страницу, которую не удалось загрузить, страницей ошибки.
    /// `load_failed`/`load_error_message` остаются выставленными — на них
    /// опираются `wait{document_ready}` и автоматизация (BUG-308, BUG-438).
    pub(crate) fn show_net_error(&mut self, msg: &str) {
        self.show_error_page(msg, NetErrorCode::classify(msg));
    }

    /// Сбой финального pipeline (`RenderDone(Err)`): сеть тут ни при чём, а
    /// сообщение — внутреннее (шрифт, CSP, разбор), поэтому код всегда
    /// `ERR_FAILED`, без угадывания по словам вроде «not found».
    pub(crate) fn show_render_error(&mut self, msg: &str) {
        self.show_error_page(msg, NetErrorCode::Failed);
    }

    fn show_error_page(&mut self, msg: &str, code: NetErrorCode) {
        if self.showing_crashed_page().is_some() {
            return;
        }
        let original = self.source.clone();
        let retry_url = original.url_str().unwrap_or("").to_owned();
        let auto_retry = self.next_auto_retry(&retry_url, code);
        let page = net_error_page::net_error_source(&retry_url, code, msg, auto_retry);
        let PageSource::Static { html, url } = page else { return };
        let html_copy = html.clone();
        // Адрес в строке — несостоявшаяся навигация; у файла кнопки «Повторить»
        // нет (относительная ссылка на путь не разрешилась бы), но адрес виден.
        let shown_url = if url.is_empty() { original.describe() } else { url };
        self.source = PageSource::Static { html, url: shown_url };
        let reloaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.reload()));
        if reloaded.is_err() {
            eprintln!("[net-error] показать страницу ошибки не удалось");
        }
        self.crashed = Some((html_copy, original));
        self.load_failed = true;
        self.load_error_message = Some(msg.to_owned());
    }

    /// Автоповтор для этого показа страницы ошибки: номер попытки считается по
    /// подряд идущим показам для одного адреса (пауза между показами больше
    /// [`RETRY_STREAK_WINDOW`] обнуляет счёт — это уже другой сбой).
    fn next_auto_retry(&mut self, url: &str, code: NetErrorCode) -> AutoRetry {
        let now = std::time::Instant::now();
        let attempt = retry_attempt(self.net_error_retries.as_ref(), url, now);
        let auto = AutoRetry::for_attempt(code, attempt);
        self.net_error_retries = (auto != AutoRetry::Off).then(|| (url.to_owned(), attempt + 1, now));
        auto
    }

    /// Исходный источник, если на экране сохранённая в [`Lumen::crashed`]
    /// страница (ошибки или падения), а не страница, ушедшая из-под неё.
    fn showing_crashed_page(&self) -> Option<(&str, &PageSource)> {
        let (html, original) = self.crashed.as_ref()?;
        matches!(&self.source, PageSource::Static { html: h, .. } if h == html)
            .then_some((html.as_str(), original))
    }

    /// Клик по ссылке кнопки «Открыть в другом браузере» страницы ошибки:
    /// передать адрес несостоявшейся навигации и вернуть `true`. Ссылка
    /// принимается только пока на экране сама страница ошибки — чужая страница
    /// не может запустить внешний браузер.
    pub(crate) fn apply_net_error_link(&mut self, resolved: &str) -> bool {
        let Some(reason) = net_error_page::parse_handoff_link(resolved) else {
            return false;
        };
        let Some((html, original)) = self.showing_crashed_page() else {
            return false;
        };
        if !html.contains(resolved) {
            return false;
        }
        let Some(url) = original.url_str().map(str::to_owned) else {
            return false;
        };
        self.open_url_in_other_browser(&url, reason, false);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn attempts_count_up_for_the_same_url_and_reset_otherwise() {
        let t0 = Instant::now();
        let prev = ("https://a.example/".to_owned(), 2, t0);
        assert_eq!(retry_attempt(None, "https://a.example/", t0), 0);
        assert_eq!(retry_attempt(Some(&prev), "https://a.example/", t0 + Duration::from_secs(46)), 2);
        assert_eq!(retry_attempt(Some(&prev), "https://b.example/", t0), 0);
        assert_eq!(retry_attempt(Some(&prev), "https://a.example/", t0 + RETRY_STREAK_WINDOW + Duration::from_secs(1)), 0);
    }
}
