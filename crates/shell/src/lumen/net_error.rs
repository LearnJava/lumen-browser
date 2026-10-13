//! COMPAT-ERRORS: сбой загрузки главного документа показывается страницей
//! ошибки, а не прежней страницей, оставшейся на экране.
//!
//! Механика та же, что у экрана падения (`crash.rs`): источник вкладки
//! подменяется статической страницей, а исходный запоминается в
//! [`Lumen::crashed`] — F5 ([`Lumen::reload`]) возвращает его и повторяет
//! загрузку, а не перерисовывает ошибку.

use lumen_core::net_error::NetErrorCode;

use crate::*;

impl Lumen {
    /// Заменяет страницу, которую не удалось загрузить, страницей ошибки.
    /// `load_failed`/`load_error_message` остаются выставленными — на них
    /// опираются `wait{document_ready}` и автоматизация (BUG-308, BUG-438).
    pub(crate) fn show_net_error(&mut self, msg: &str) {
        let showing_error = self.crashed.as_ref().is_some_and(|(html, _)| {
            matches!(&self.source, PageSource::Static { html: h, .. } if h == html)
        });
        if showing_error {
            return;
        }
        let code = NetErrorCode::classify(msg);
        let original = self.source.clone();
        let retry_url = original.url_str().unwrap_or("").to_owned();
        let page = net_error_page::net_error_source(&retry_url, code, msg);
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
}
