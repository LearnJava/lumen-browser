//! `#hangBar` — плашка «Страница не отвечает» (COMPAT-HANG).
//!
//! Сторож `lumen_core::script_stall` замечает JS-задачу, идущую дольше порога
//! (5 с, `LUMEN_SCRIPT_STALL_MS`), и будит цикл событий. Здесь оболочка решает,
//! чья это задача: рантайм активной вкладки (`Lumen::js_stall`) либо «ничей»
//! рантайм, которого вкладка ещё не взяла, — страница, чьи `<script>` крутятся
//! на загрузке, или фрейм. Все чтения — атомики, JS-потока не ждём: именно он и
//! завис.
//!
//! Кнопки: «Подождать» (плашка вернётся через 15 с работы того же скрипта),
//! «Остановить скрипт» (`TerminateExecution`, страница остаётся), «Перезагрузить»
//! (остановка и обычная перезагрузка). Рантаймы фоновых вкладок не учитываются.

use lumen_chrome::ChromeHangBarModel;
use lumen_core::script_stall::{StallHandle, unclaimed_stalled};

/// Состояние `#hangBar`.
#[derive(Default)]
pub(crate) struct ScriptHang {
    /// Рантайм, о котором показана плашка; `None` — панель скрыта.
    shown: Option<StallHandle>,
    /// Секунды зависания на момент показа (панель не тикает сама).
    secs: u64,
}

impl ScriptHang {
    /// Пересматривает, нужна ли плашка. `tab` — рантайм активной вкладки.
    /// `true`, если панель изменилась (нужна перерисовка хрома).
    pub(crate) fn refresh(&mut self, tab: Option<&StallHandle>, suppressed: bool) -> bool {
        let found = if suppressed {
            None
        } else {
            let tab_stall = tab.and_then(|h| h.stalled_ms().map(|ms| (h.clone(), ms)));
            tab_stall.or_else(|| unclaimed_stalled().and_then(|h| h.stalled_ms().map(|ms| (h, ms))))
        };
        match (&self.shown, found) {
            (None, None) => false,
            (Some(_), None) => {
                self.shown = None;
                true
            }
            (Some(cur), Some((h, _))) if cur.same_runtime(&h) => false,
            (_, Some((h, ms))) => {
                self.secs = ms / 1000;
                self.shown = Some(h);
                true
            }
        }
    }

    pub(crate) fn is_shown(&self) -> bool {
        self.shown.is_some()
    }

    pub(crate) fn secs(&self) -> u64 {
        self.secs
    }

    /// «Подождать».
    pub(crate) fn wait(&mut self) {
        if let Some(h) = self.shown.take() {
            h.wait_more();
        }
    }

    /// «Остановить скрипт»; `true`, если был что останавливать. Плашка уйдёт,
    /// когда сторож увидит конец задачи.
    pub(crate) fn stop(&self) -> bool {
        self.shown.as_ref().is_some_and(StallHandle::terminate)
    }

    /// Данные для `#hangBar`.
    pub(crate) fn model(&self) -> ChromeHangBarModel {
        if self.shown.is_none() {
            return ChromeHangBarModel::default();
        }
        ChromeHangBarModel {
            open: true,
            meta: format!(
                "Скрипт на странице не возвращает управление уже {} с. Остальные вкладки работают.",
                self.secs
            ),
        }
    }
}

impl crate::Lumen {
    /// Зовётся из `about_to_wait` (сторож будит цикл событий): открывает или
    /// убирает плашку. Читает только атомики — JS-поток может висеть.
    pub(crate) fn poll_script_hang(&mut self) {
        // Модальный `alert`/`confirm` держит JS-поток по замыслу страницы, а не
        // зависанием: плашку поверх диалога не показываем.
        let suppressed = self.page_dialog.is_some();
        let was_shown = self.script_hang.is_shown();
        let changed = self.script_hang.refresh(self.js_stall.as_ref(), suppressed);
        if !changed {
            return;
        }
        if !was_shown && self.script_hang.is_shown() {
            crate::health_log::log_script_stall(
                &self.source.describe(),
                &format!("{} с без возврата управления", self.script_hang.secs()),
            );
        }
        self.relayout_chrome_host();
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_is_closed_while_nothing_hangs() {
        let mut hang = ScriptHang::default();
        assert!(!hang.model().open);
        let idle = StallHandle::new();
        idle.claim();
        assert!(!hang.refresh(Some(&idle), false), "свободный рантайм плашку не открывает");
        assert!(!hang.stop());
    }

    #[test]
    fn wait_hides_the_bar_and_stop_needs_a_shown_script() {
        let mut hang = ScriptHang::default();
        let h = StallHandle::new();
        h.claim();
        hang.shown = Some(h.clone());
        hang.secs = 7;
        assert!(hang.model().open);
        assert!(hang.model().meta.contains("7 с"));
        // Рантайм свободен — останавливать нечего, но плашка закрывается по «Подождать».
        assert!(!hang.stop());
        hang.wait();
        assert!(!hang.model().open);
    }
}
