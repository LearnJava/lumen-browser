//! Адресная строка (Ctrl+L): состояние overlay-бара и его сборка в display list.
//!
//! Паттерн идентичен `find.rs`: stateless рендер — `build_bar_overlay` каждый
//! кадр, stateful ввод — `AddressBarState` мутируется из event-handler-а.
//!
//! Commit-семантика: Enter → `take_commit()` возвращает URL и сбрасывает
//! состояние. Caller обязан вызвать `navigate_to(PageSource::from_arg(...))`.

use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{DisplayCommand, DisplayList};
use lumen_core::geom::Rect;

// ── Визуальные константы ──────────────────────────────────────────────────────

const BAR_BG: Color = Color { r: 32, g: 33, b: 36, a: 240 };
const BAR_BORDER: Color = Color { r: 60, g: 120, b: 220, a: 255 };
const BAR_FG: Color = Color { r: 232, g: 232, b: 236, a: 255 };
const BAR_DIM: Color = Color { r: 140, g: 140, b: 148, a: 255 };
const INPUT_BG: Color = Color { r: 18, g: 18, b: 22, a: 255 };
const CURSOR: Color = Color { r: 100, g: 160, b: 255, a: 220 };

const BAR_W: f32 = 560.0;
const BAR_H: f32 = 52.0;
const PAD: f32 = 10.0;
const FONT: f32 = 16.0;
/// Максимальная длина строки ввода. Защита от случайной paste-атаки.
const MAX_INPUT_LEN: usize = 2048;

// ── Состояние ─────────────────────────────────────────────────────────────────

/// Состояние адресной строки. Хранится в `Lumen` struct наряду с `FindState`.
#[derive(Debug, Default, Clone)]
pub struct AddressBarState {
    open: bool,
    input: String,
    /// Если `Some`, caller должен навигироваться на это значение и вызвать
    /// `clear_commit()`. Устанавливается в `commit()`.
    pending_commit: Option<String>,
}

impl AddressBarState {
    /// Открыть бар, предзаполнив поле текущим URL страницы.
    pub fn open(&mut self, current_url: &str) {
        self.open = true;
        self.input = current_url.to_owned();
        self.pending_commit = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input.clear();
        self.pending_commit = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    /// Добавить непечатаемые символы (printable chars из keyboard event).
    pub fn append_str(&mut self, s: &str) {
        if !self.open {
            return;
        }
        for c in s.chars() {
            if !c.is_control() && self.input.len() < MAX_INPUT_LEN {
                self.input.push(c);
            }
        }
    }

    /// Backspace — удалить последний Unicode-символ.
    pub fn backspace(&mut self) {
        if self.open {
            self.input.pop();
        }
    }

    /// Зафиксировать текущий ввод: закрыть бар и, если ввод непуст, выставить
    /// pending_commit. Caller получает URL через `take_commit()`.
    pub fn commit(&mut self) {
        if !self.open {
            return;
        }
        let url = if self.input.is_empty() { None } else { Some(self.input.clone()) };
        self.close(); // сбрасывает input и open, pending_commit = None
        self.pending_commit = url; // устанавливаем после close
    }

    /// Вернуть зафиксированный URL (если есть) и сбросить его.
    /// Caller обязан обработать результат в этом же кадре.
    pub fn take_commit(&mut self) -> Option<String> {
        self.pending_commit.take()
    }
}

// ── Рендер ────────────────────────────────────────────────────────────────────

/// Параметры для сборки overlay display list.
pub struct BarOverlay {
    /// Размер окна в физических пикселях (для позиционирования по центру).
    pub window_size: (u32, u32),
}

/// Собирает display list адресной строки. Вызывается каждый кадр, пока
/// `state.is_open()`. Возвращаемый список рисуется поверх страницы без
/// scroll-смещения (viewport-locked).
pub fn build_bar_overlay(state: &AddressBarState, bar: BarOverlay) -> DisplayList {
    let (ww, _wh) = bar.window_size;
    let x = ((ww as f32 - BAR_W) * 0.5).max(PAD);
    let y = PAD;

    let mut out = DisplayList::with_capacity(6);

    // Рамка (на 1px шире с каждой стороны) — синий accent.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x - 1.0, y - 1.0, BAR_W + 2.0, BAR_H + 2.0),
        color: BAR_BORDER,
    });
    // Фон бара.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, BAR_W, BAR_H),
        color: BAR_BG,
    });

    // Поле ввода.
    let input_x = x + PAD;
    let input_w = BAR_W - PAD * 2.0;
    let input_h = BAR_H - PAD * 2.0;
    let input_y = y + PAD;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(input_x, input_y, input_w, input_h),
        color: INPUT_BG,
    });

    // Текст или placeholder.
    let (display_text, text_color) = if state.input().is_empty() {
        ("Введите URL или поисковый запрос…", BAR_DIM)
    } else {
        (state.input(), BAR_FG)
    };
    let text_margin = 6.0;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(
            input_x + text_margin,
            input_y + (input_h - FONT * 1.2) * 0.5,
            input_w - text_margin * 2.0 - 10.0, // запас под cursor
            FONT * 1.2,
        ),
        text: display_text.to_string(),
        font_size: FONT,
        color: text_color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    // Курсор — вертикальная линия справа от текста (упрощённо: 2px блок).
    // Phase 0: ширина текста не считается точно — cursor фиксирован у правого
    // края поля ввода, что достаточно как визуальный индикатор режима ввода.
    if !state.input().is_empty() {
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(
                input_x + input_w - text_margin - 2.0,
                input_y + (input_h - FONT * 1.2) * 0.5,
                2.0,
                FONT * 1.2,
            ),
            color: CURSOR,
        });
    }

    out
}

// ── Тесты ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_prefills_url() {
        let mut s = AddressBarState::default();
        s.open("https://example.com");
        assert!(s.is_open());
        assert_eq!(s.input(), "https://example.com");
    }

    #[test]
    fn close_resets_state() {
        let mut s = AddressBarState::default();
        s.open("https://example.com");
        s.close();
        assert!(!s.is_open());
        assert_eq!(s.input(), "");
    }

    #[test]
    fn append_adds_chars() {
        let mut s = AddressBarState::default();
        s.open("");
        s.append_str("https://");
        s.append_str("rust-lang.org");
        assert_eq!(s.input(), "https://rust-lang.org");
    }

    #[test]
    fn append_ignores_control_chars() {
        let mut s = AddressBarState::default();
        s.open("");
        s.append_str("abc\n\t\x08");
        assert_eq!(s.input(), "abc");
    }

    #[test]
    fn append_ignored_when_closed() {
        let mut s = AddressBarState::default();
        s.append_str("abc");
        assert_eq!(s.input(), "");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut s = AddressBarState::default();
        s.open("abc");
        s.backspace();
        assert_eq!(s.input(), "ab");
        s.backspace();
        s.backspace();
        s.backspace(); // no panic on empty
        assert_eq!(s.input(), "");
    }

    #[test]
    fn commit_takes_url_and_closes() {
        let mut s = AddressBarState::default();
        s.open("https://example.com");
        s.append_str("/page");
        s.commit();
        assert!(!s.is_open());
        assert_eq!(s.take_commit(), Some("https://example.com/page".to_owned()));
        // second take returns None
        assert_eq!(s.take_commit(), None);
    }

    #[test]
    fn commit_empty_input_is_noop() {
        let mut s = AddressBarState::default();
        s.open("");
        s.commit();
        // stays closed (opened then committed with empty)
        assert!(!s.is_open());
        assert_eq!(s.take_commit(), None);
    }

    #[test]
    fn max_len_enforced() {
        let mut s = AddressBarState::default();
        s.open("");
        let big = "a".repeat(MAX_INPUT_LEN + 100);
        s.append_str(&big);
        assert!(s.input().len() <= MAX_INPUT_LEN);
    }

    #[test]
    fn overlay_has_rect_and_text_when_open() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("https://example.com");
            x
        };
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) });
        let has_text = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("example.com"))
        });
        assert!(has_text);
    }

    #[test]
    fn overlay_shows_placeholder_when_empty() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("");
            x
        };
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) });
        let has_placeholder = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("URL"))
        });
        assert!(has_placeholder);
    }

    #[test]
    fn overlay_has_border_rect_as_first_cmd() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("x");
            x
        };
        let dl = build_bar_overlay(&s, BarOverlay { window_size: (1024, 720) });
        assert!(matches!(dl[0], DisplayCommand::FillRect { color, .. } if color.b == BAR_BORDER.b));
    }
}
