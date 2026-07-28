//! Адресная строка: состояние омнибокса.
//!
//! DS-10 — омнибокс постоянно виден как инлайн-поле в центре тулбара, читаемое
//! движковым хромом (CC-7/CC-9: `chrome_omnibox_value` и
//! `OmniboxSuggestion::{commit_value,label,sub_label,tag_color}`). Этот модуль
//! хранит только состояние (`AddressBarState`) — своей отрисовки у него больше
//! нет (CC-15-3: легаси-инлайн-поле/dropdown были единственным вызывающим
//! кодом, удалённым вместе с `toolbar::build_toolbar`).
//!
//! Commit-семантика: Enter → `take_commit()` возвращает URL/запрос и сбрасывает
//! состояние. Caller обязан обработать навигацию или запрос.
//!
//! Omnibox-интеграция: при изменении ввода caller вызывает `set_suggestions()`,
//! передавая результаты из `HistoryFts` + `SearchHistory`. Стрелки Up/Down
//! перемещают выделение; Enter коммитит выделенную строку или raw input.
//!
//! `@history <query>` — FTS-поиск по истории; `@notes <query>` — поиск по
//! пользовательским заметкам (§12.2); `@read-later <query>` — поиск по списку
//! «прочитать позже» (§12.3); `@tabs <query>` — поиск по открытым вкладкам
//! (§12.4); `@bookmarks <query>` — поиск по закладкам, с cosine-similarity
//! ранжированием при наличии AI-эмбеддинга (§12.8); `@ai <query>` — RAG-ответ
//! через `lumen-ai` `RagEngine` (§12.5), либо hint-строка если модуль не
//! собран (`--features ai`); без префикса — prefix-match по search_history +
//! FTS по умолчанию.
//!
//! DS-6 — IDN homograph-spoof guard: любой URL-текст, попадающий на экран
//! (поле ввода, label/sub_label подсказок) или в `commit()`, проходит через
//! `guard_display_text()` → `lumen_core::idn::display_host` (DS-5). Если
//! хост признан спуф-риском, отображается/коммитится его Punycode-форма.

use lumen_layout::Color;
use lumen_core::idn::{HostDisplay, SpoofReason, display_host};
use lumen_core::url::Url;

// ── Визуальные константы ──────────────────────────────────────────────────────
//
// CC-15-3: the inline field/dropdown painters (`build_inline_field`,
// `build_dropdown`) were removed once the engine-drawn chrome (CC-4) made
// their sole caller (`toolbar::build_toolbar` and the legacy tab-bar paint
// block in `main.rs`) dead code. `chrome_omnibox_value`/`commit_value`/
// `label`/`sub_label`/`tag_color` stay — CC-7/CC-9 read them for the
// engine-chrome `#omniInput`/`#omniDropdown` equivalents.

/// Tag accent for FTS-history omnibox results — read by `tag_color()` (CC-9:
/// also used by `Lumen::chrome_model_snapshot` for `#omniDropdown`'s
/// `.dd-icon` swatch color).
const HISTORY_TAG: Color = Color { r: 60, g: 120, b: 220, a: 255 };
/// Green tag accent for search-query omnibox results.
const ITEM_TAG: Color = Color { r: 72, g: 150, b: 90, a: 255 };

/// Максимум строк в dropdown. Also the cap `Lumen::chrome_model_snapshot`
/// applies to `#omniDropdown`'s rebuilt row list (CC-9).
pub(crate) const MAX_VISIBLE: usize = 7;
/// Максимальная длина строки ввода. Защита от случайной paste-атаки.
const MAX_INPUT_LEN: usize = 2048;

// ── IDN spoof guard (DS-6) ──────────────────────────────────────────────────

/// Прогоняет `text` (полный URL) через детектор омоглифов/mixed-script
/// (`lumen_core::idn::display_host`, DS-5). Если хост признан спуф-риском,
/// возвращает `text` с хостом, замененным на его Punycode ASCII-форму, и
/// причину. Иначе возвращает `text` без изменений и `None`.
///
/// Вход без схемы (поисковый запрос, `@`-команда, внутренние sentinel-ы
/// вроде `switch-tab:<id>`) не парсится `Url::parse` или даёт пустой host —
/// в обоих случаях возвращается как есть: детектор действует только на
/// реальный URL-хост.
fn guard_display_text(text: &str) -> (String, Option<SpoofReason>) {
    let Ok(url) = Url::parse(text) else {
        return (text.to_owned(), None);
    };
    let host = url.host();
    if host.is_empty() {
        return (text.to_owned(), None);
    }
    match display_host(host) {
        HostDisplay::Punycode { ascii, reason } => (text.replacen(host, &ascii, 1), Some(reason)),
        HostDisplay::Unicode(_) => (text.to_owned(), None),
    }
}

/// Текст красной строки-предупреждения под полем ввода для причины спуфинга.
fn spoof_warning_message(reason: SpoofReason) -> &'static str {
    match reason {
        SpoofReason::MixedScript => {
            "Домен смешивает алфавиты — возможна подмена, показан Punycode"
        }
        SpoofReason::ConfusableLabel => {
            "Буквы домена похожи на латиницу — возможна подмена, показан Punycode"
        }
    }
}

/// CC-7 (`docs/tasks/p1-css-chrome.md`): chrome-DOM equivalent of the value
/// the removed `build_inline_field` drew — the not-focused/focused branching
/// (current URL vs. live input or selected suggestion), IDN-guarded, minus
/// glyph-rect placement. Returned `value` is written into `#omniInput`'s `value`
/// attribute (`lumen_chrome::OmniboxModel::value`); empty lets the asset's
/// own `placeholder` attribute show through, so unlike the legacy overlay
/// this never needs an "about:blank"/"Введите URL…" text fallback.
pub(crate) fn chrome_omnibox_value(state: &AddressBarState, current_url: &str) -> (String, Option<&'static str>) {
    if !state.is_open() {
        let (guarded, _) = guard_display_text(current_url);
        return (guarded, None);
    }
    let display_input = match state.selected_idx() {
        Some(idx) => state.suggestions().get(idx).map(|s| s.commit_value()).unwrap_or(state.input()),
        None => state.input(),
    };
    let (guarded, reason) = guard_display_text(display_input);
    (guarded, reason.map(spoof_warning_message))
}

/// DS-6 guard for one `#omniDropdown` row (CC-9): returns the IDN-guarded
/// `(label, sub_label)` pair to write into `ChromeSuggestionModel`.
///
/// The removed legacy `build_dropdown` ran both strings through
/// `guard_display_text` before drawing them; the engine-chrome snapshot read
/// `label()`/`sub_label()` raw, so a homograph host in a history/bookmark hit
/// reached the screen in its Unicode form. Routing the snapshot through this
/// helper restores the invariant stated at the top of this module — every
/// URL-bearing string that reaches the screen is punycode-guarded.
pub(crate) fn chrome_suggestion_text(s: &OmniboxSuggestion) -> (String, String) {
    (guard_display_text(s.label()).0, guard_display_text(s.sub_label()).0)
}

// ── Omnibox prefix ────────────────────────────────────────────────────────────

/// Префикс @-команды, распознанный в строке ввода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmniboxPrefix {
    /// `@history <query>` — поиск по FTS5-индексу истории посещённых страниц.
    History,
    /// `@notes <query>` — поиск по пользовательским заметкам (§12.2).
    Notes,
    /// `@read-later <query>` — поиск по сохранённым «прочитать позже» (§12.3).
    ///
    /// Поиск во время ввода; commit без выделения подсказки сохраняет ввод
    /// как URL (см. `omnibox::resolve` → `SaveReadLater`).
    ReadLater,
    /// `@tabs <query>` — поиск по открытым вкладкам (§12.4, заголовок + URL).
    Tabs,
    /// `@bookmarks <query>` — поиск по закладкам (§12.8): подстрочное
    /// совпадение title/url/тегов, при наличии AI-эмбеддинга результат
    /// дополнительно ранжируется по cosine-similarity к запросу.
    Bookmarks,
    /// `@ai <query>` — RAG-ответ через `lumen-ai` `RagEngine` (§12.5), либо
    /// hint-строка «AI module not enabled» если крейт не собран (`--features ai`).
    Ai,
    /// Обычный ввод: URL или поисковый запрос.
    Plain,
}

/// Разбирает raw ввод → `(OmniboxPrefix, query_str)`.
///
/// `@history foo bar` → `(History, "foo bar")`.
/// `@notes foo bar` → `(Notes, "foo bar")`.
/// `@read-later foo` → `(ReadLater, "foo")`.
/// `@tabs foo` → `(Tabs, "foo")`.
/// Всё остальное → `(Plain, trimmed_input)`.
pub fn parse_omnibox_prefix(input: &str) -> (OmniboxPrefix, &str) {
    let s = input.trim_start();
    if let Some(rest) = s.strip_prefix("@history") {
        (OmniboxPrefix::History, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@notes") {
        (OmniboxPrefix::Notes, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@read-later") {
        (OmniboxPrefix::ReadLater, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@tabs") {
        (OmniboxPrefix::Tabs, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@bookmarks") {
        (OmniboxPrefix::Bookmarks, rest.trim_start())
    } else if let Some(rest) = s.strip_prefix("@ai") {
        (OmniboxPrefix::Ai, rest.trim_start())
    } else {
        (OmniboxPrefix::Plain, s)
    }
}

// ── OmniboxSuggestion ─────────────────────────────────────────────────────────

/// Одна строка autocomplete в dropdown omnibox.
#[derive(Debug, Clone)]
pub enum OmniboxSuggestion {
    /// Результат FTS5-поиска по истории (`HistoryFts::search`).
    HistoryFts {
        /// URL посещённой страницы.
        url: String,
        /// Заголовок страницы (может быть пустым).
        title: String,
        /// Сниппет совпадения из текста страницы.
        snippet: String,
    },
    /// Результат FTS5-поиска по заметкам (§12.2, `@notes <query>`).
    ///
    /// При выборе пользователем `commit_value()` возвращает `viewer_url`
    /// (`note-viewer:<id>`), который перехватывается в `handle_omnibox_commit`
    /// для открытия `NoteViewerPanel`. Данные заметки (comment и проч.)
    /// запрашиваются напрямую из `notes_store` по id, поэтому хранить их
    /// здесь не нужно.
    Note {
        /// URL, к которому привязана заметка (отображается в dropdown label).
        url: String,
        /// Выделенный текст (selection) заметки.
        selection: String,
        /// BM25 сниппет вокруг совпадения.
        snippet: String,
        /// `note-viewer:<id>` — committed value, opens the note viewer.
        viewer_url: String,
    },
    /// Ранее введённый поисковый запрос (`SearchHistory::prefix_match`).
    SearchQuery {
        /// Исходная строка запроса (case-preserved).
        query: String,
        /// Частота использования — показывалась легаси-dropdown'ом как тег
        /// `×N`.
        ///
        /// BUG-410: `#omniDropdown` (CC-9) переносит из подсказки только
        /// `label`/`sub_label`/`tag_color`, но не текстовый тег, поэтому с
        /// удалением легаси-рендера (CC-15-3) поле осталось без читателя.
        /// Данные сохранены — их потребит миграция тега в движковый хром.
        #[allow(dead_code, reason = "BUG-410: тег строки dropdown ещё не перенесён в движковый хром")]
        frequency: i64,
    },
    /// Результат FTS5-поиска по списку «прочитать позже» (§12.3, `@read-later`).
    ///
    /// При выборе `commit_value()` возвращает `url` → обычная навигация на
    /// сохранённую страницу.
    ReadLater {
        /// URL сохранённой страницы — committed value (навигация).
        url: String,
        /// Заголовок страницы (может быть пустым → показываем URL).
        title: String,
        /// BM25 сниппет вокруг совпадения.
        snippet: String,
    },
    /// Открытая вкладка, совпавшая с `@tabs <query>` (§12.4).
    ///
    /// При выборе `commit_value()` возвращает `switch_value`
    /// (`switch-tab:<id>`), перехватываемый в `handle_omnibox_commit` для
    /// переключения на вкладку по её стабильному id.
    Tab {
        /// Заголовок вкладки (может быть пустым → показываем URL).
        title: String,
        /// URL открытой во вкладке страницы (для sub_label).
        url: String,
        /// `switch-tab:<id>` — committed value, переключает на вкладку.
        switch_value: String,
    },
    /// Результат поиска по закладкам (§12.8, `@bookmarks <query>`).
    ///
    /// При выборе `commit_value()` возвращает `url` → обычная навигация.
    Bookmark {
        /// Заголовок закладки (может быть пустым → показываем URL).
        title: String,
        /// URL закладки.
        url: String,
        /// AI-саммари страницы, если вычислено (см. `Bookmarks::set_semantic`),
        /// иначе пустая строка — `sub_label()` подставит URL.
        snippet: String,
    },
    /// Единственная строка ответа на `@ai <query>` (§12.5).
    ///
    /// При выборе `commit_value()` возвращает sentinel `"ai-answer:noop"`
    /// (перехватывается в `handle_omnibox_commit` — навигация не нужна, весь
    /// ответ уже показан в самой строке dropdown).
    Ai {
        /// RAG-ответ (`RagEngine::answer`), fallback-текст `NullAiBackend`
        /// если Ollama недоступен (ADR-019), либо hint «AI module not
        /// enabled» под `#[cfg(not(feature = "ai"))]`.
        answer: String,
    },
}

impl OmniboxSuggestion {
    /// Строка, которая будет зафиксирована при выборе этой подсказки.
    /// HistoryFts → URL навигации. Note → `note-viewer:<id>` (перехват в shell).
    /// SearchQuery → текст запроса. ReadLater → URL навигации.
    /// Tab → `switch-tab:<id>` (перехват в shell).
    pub fn commit_value(&self) -> &str {
        match self {
            OmniboxSuggestion::HistoryFts { url, .. } => url,
            OmniboxSuggestion::Note { viewer_url, .. } => viewer_url,
            OmniboxSuggestion::SearchQuery { query, .. } => query,
            OmniboxSuggestion::ReadLater { url, .. } => url,
            OmniboxSuggestion::Tab { switch_value, .. } => switch_value,
            OmniboxSuggestion::Bookmark { url, .. } => url,
            OmniboxSuggestion::Ai { .. } => "ai-answer:noop",
        }
    }

    /// Основной текст строки dropdown.
    pub fn label(&self) -> &str {
        match self {
            OmniboxSuggestion::HistoryFts { title, url, .. } => {
                if title.is_empty() { url } else { title }
            }
            OmniboxSuggestion::Note { selection, .. } => selection,
            OmniboxSuggestion::SearchQuery { query, .. } => query,
            OmniboxSuggestion::ReadLater { title, url, .. }
            | OmniboxSuggestion::Tab { title, url, .. }
            | OmniboxSuggestion::Bookmark { title, url, .. } => {
                if title.is_empty() { url } else { title }
            }
            OmniboxSuggestion::Ai { answer } => answer,
        }
    }

    /// Дополнительный текст под основным label.
    /// HistoryFts: сниппет если непуст, иначе URL.
    /// Note: сниппет вокруг совпадения (или URL если сниппет пуст).
    /// SearchQuery: пустая строка (вся информация в label).
    /// ReadLater: сниппет если непуст, иначе URL.
    /// Tab: URL открытой страницы.
    /// Bookmark: AI-саммари если вычислено, иначе URL.
    /// Ai: пустая строка (весь ответ уже в label).
    pub fn sub_label(&self) -> &str {
        match self {
            OmniboxSuggestion::HistoryFts { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::Note { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::SearchQuery { .. } => "",
            OmniboxSuggestion::ReadLater { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::Tab { url, .. } => url,
            OmniboxSuggestion::Bookmark { snippet, url, .. } => {
                if !snippet.is_empty() { snippet } else { url }
            }
            OmniboxSuggestion::Ai { .. } => "",
        }
    }

    /// CC-9: also read by `Lumen::chrome_model_snapshot` for `#omniDropdown`'s
    /// `.dd-icon` swatch color, mirroring the legacy overlay's own tag color.
    pub(crate) fn tag_color(&self) -> Color {
        match self {
            OmniboxSuggestion::HistoryFts { .. } => HISTORY_TAG,
            OmniboxSuggestion::Note { .. } => Color { r: 180, g: 120, b: 60, a: 255 },
            OmniboxSuggestion::SearchQuery { .. } => ITEM_TAG,
            OmniboxSuggestion::ReadLater { .. } => Color { r: 120, g: 90, b: 180, a: 255 },
            OmniboxSuggestion::Tab { .. } => Color { r: 60, g: 150, b: 170, a: 255 },
            OmniboxSuggestion::Bookmark { .. } => Color { r: 200, g: 160, b: 40, a: 255 },
            OmniboxSuggestion::Ai { .. } => Color { r: 150, g: 70, b: 200, a: 255 },
        }
    }
}

// ── Состояние ─────────────────────────────────────────────────────────────────

/// Состояние адресной строки. Хранится в `Lumen` struct наряду с `FindState`.
#[derive(Debug, Default, Clone)]
pub struct AddressBarState {
    open: bool,
    input: String,
    /// Если `Some`, caller должен навигироваться на это значение и вызвать
    /// `clear_commit()`. Устанавливается в `commit()`.
    pending_commit: Option<String>,
    /// Текущий список подсказок. Обновляется caller-ом через `set_suggestions()`
    /// после каждого изменения ввода.
    suggestions: Vec<OmniboxSuggestion>,
    /// Индекс выделенной подсказки в dropdown. `None` — курсор в поле ввода.
    selected_idx: Option<usize>,
}

impl AddressBarState {
    /// Открыть бар, предзаполнив поле текущим URL страницы.
    pub fn open(&mut self, current_url: &str) {
        self.open = true;
        self.input = current_url.to_owned();
        self.pending_commit = None;
        self.suggestions.clear();
        self.selected_idx = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input.clear();
        self.pending_commit = None;
        self.suggestions.clear();
        self.selected_idx = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    /// Текущий список подсказок (для рендера и клавиатурной навигации).
    pub fn suggestions(&self) -> &[OmniboxSuggestion] {
        &self.suggestions
    }

    /// Индекс выделенной подсказки. `None` — ни одна не выделена.
    pub fn selected_idx(&self) -> Option<usize> {
        self.selected_idx
    }

    /// Установить новый список подсказок и сбросить выделение.
    /// Вызывается caller-ом после каждого изменения ввода.
    pub fn set_suggestions(&mut self, suggestions: Vec<OmniboxSuggestion>) {
        self.suggestions = suggestions;
        self.selected_idx = None;
    }

    /// Перейти к следующей (вниз) подсказке.
    pub fn select_next(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }
        self.selected_idx = Some(match self.selected_idx {
            None => 0,
            Some(i) => (i + 1).min(self.suggestions.len() - 1),
        });
    }

    /// Перейти к предыдущей (вверх) подсказке. `None` если уже на первой.
    pub fn select_prev(&mut self) {
        self.selected_idx = match self.selected_idx {
            None | Some(0) => None,
            Some(i) => Some(i - 1),
        };
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
        // Сбросить выделение при ручном вводе.
        self.selected_idx = None;
    }

    /// Backspace — удалить последний Unicode-символ.
    pub fn backspace(&mut self) {
        if self.open {
            self.input.pop();
            self.selected_idx = None;
        }
    }

    /// Зафиксировать текущий ввод или выделенную подсказку: закрыть бар и,
    /// если значение непусто, выставить pending_commit. Caller получает
    /// значение через `take_commit()`.
    pub fn commit(&mut self) {
        if !self.open {
            return;
        }
        let value = if let Some(idx) = self.selected_idx {
            self.suggestions.get(idx).map(|s| s.commit_value().to_owned())
        } else if !self.input.is_empty() {
            Some(self.input.clone())
        } else {
            None
        };
        // DS-6: если хост коммитимого значения — спуф-риск, навигируем на
        // его Punycode-форму, а не на визуально подделываемый Unicode.
        let value = value.map(|v| guard_display_text(&v).0);
        self.close(); // сбрасывает input, open, suggestions, selected_idx, pending_commit
        self.pending_commit = value; // восстанавливаем после close
    }

    /// Вернуть зафиксированный URL/запрос (если есть) и сбросить его.
    /// Caller обязан обработать результат в этом же кадре.
    pub fn take_commit(&mut self) -> Option<String> {
        self.pending_commit.take()
    }

    /// Фиксирует подсказку `idx` напрямую (CC-9: клик по движковому
    /// `#omniDropdown` не проходит через `selected_idx`, который отслеживает
    /// только клавиатурную навигацию) — та же spoof-guard и
    /// close-затем-pending_commit последовательность, что и в [`Self::commit`].
    pub fn commit_suggestion(&mut self, idx: usize) {
        if !self.open {
            return;
        }
        let value = self.suggestions.get(idx).map(|s| s.commit_value().to_owned());
        let value = value.map(|v| guard_display_text(&v).0);
        self.close();
        self.pending_commit = value;
    }
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
        assert!(s.suggestions().is_empty());
        assert_eq!(s.selected_idx(), None);
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
        assert_eq!(s.take_commit(), None);
    }

    #[test]
    fn commit_empty_input_is_noop() {
        let mut s = AddressBarState::default();
        s.open("");
        s.commit();
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
    fn chrome_omnibox_value_shows_current_url_when_not_focused() {
        let s = AddressBarState::default();
        let (value, warning) = chrome_omnibox_value(&s, "https://example.com/page");
        assert_eq!(value, "https://example.com/page");
        assert_eq!(warning, None);
    }

    #[test]
    fn chrome_omnibox_value_shows_live_input_when_focused() {
        let mut s = AddressBarState::default();
        s.open("https://example.com");
        s.append_str("/more");
        let (value, warning) = chrome_omnibox_value(&s, "https://example.com");
        assert_eq!(value, "https://example.com/more");
        assert_eq!(warning, None);
    }

    #[test]
    fn chrome_omnibox_value_shows_selected_suggestion() {
        let mut s = AddressBarState::default();
        s.open("");
        s.set_suggestions(vec![OmniboxSuggestion::SearchQuery { query: "rust book".to_owned(), frequency: 3 }]);
        s.select_next();
        let (value, _) = chrome_omnibox_value(&s, "");
        assert_eq!(value, "rust book");
    }

    #[test]
    fn chrome_omnibox_value_flags_spoof_risk_host_when_focused() {
        let mut s = AddressBarState::default();
        s.open("https://аpple.com/login");
        let (value, warning) = chrome_omnibox_value(&s, "https://аpple.com/login");
        assert!(value.contains("xn--"), "spoof-risk host must be shown as punycode: {value}");
        assert!(warning.is_some());
    }

    /// DS-6 for the engine-chrome dropdown: replaces the deleted
    /// `dropdown_suggestion_url_is_punycode_guarded`, which asserted the same
    /// invariant against the legacy `build_dropdown` display list.
    #[test]
    fn chrome_suggestion_text_is_punycode_guarded() {
        let s = OmniboxSuggestion::HistoryFts {
            url: "https://аpple.com/".into(),
            title: String::new(),
            snippet: String::new(),
        };
        let (label, sub_label) = chrome_suggestion_text(&s);
        assert!(label.contains("xn--pple-43d.com"), "label must be punycode-guarded: {label}");
        assert!(
            sub_label.contains("xn--pple-43d.com"),
            "sub_label must be punycode-guarded: {sub_label}"
        );
    }

    /// A safe host must survive the guard unchanged — the negative half of
    /// the deleted `field_has_no_warning_strip_for_safe_input`.
    #[test]
    fn chrome_suggestion_text_leaves_safe_host_untouched() {
        let s = OmniboxSuggestion::HistoryFts {
            url: "https://google.com/search".into(),
            title: "Google".into(),
            snippet: String::new(),
        };
        let (label, sub_label) = chrome_suggestion_text(&s);
        assert_eq!(label, "Google");
        assert_eq!(sub_label, "https://google.com/search");
    }

    // ── Omnibox prefix ────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_history() {
        let (prefix, q) = parse_omnibox_prefix("@history rust async");
        assert_eq!(prefix, OmniboxPrefix::History);
        assert_eq!(q, "rust async");
    }

    #[test]
    fn parse_prefix_plain() {
        let (prefix, q) = parse_omnibox_prefix("rust async");
        assert_eq!(prefix, OmniboxPrefix::Plain);
        assert_eq!(q, "rust async");
    }

    #[test]
    fn parse_prefix_history_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@history ");
        assert_eq!(prefix, OmniboxPrefix::History);
        assert_eq!(q, "");
    }

    #[test]
    fn parse_prefix_leading_space() {
        let (prefix, _q) = parse_omnibox_prefix("  @history foo");
        assert_eq!(prefix, OmniboxPrefix::History);
    }

    // ── Suggestion selection ──────────────────────────────────────────────────

    #[test]
    fn select_next_cycles() {
        let mut s = AddressBarState::default();
        s.open("r");
        s.set_suggestions(vec![
            OmniboxSuggestion::SearchQuery { query: "rust".into(), frequency: 3 },
            OmniboxSuggestion::SearchQuery { query: "rayon".into(), frequency: 1 },
        ]);
        assert_eq!(s.selected_idx(), None);
        s.select_next();
        assert_eq!(s.selected_idx(), Some(0));
        s.select_next();
        assert_eq!(s.selected_idx(), Some(1));
        s.select_next(); // clamp at last
        assert_eq!(s.selected_idx(), Some(1));
    }

    #[test]
    fn select_prev_goes_to_none() {
        let mut s = AddressBarState::default();
        s.open("r");
        s.set_suggestions(vec![
            OmniboxSuggestion::SearchQuery { query: "rust".into(), frequency: 3 },
        ]);
        s.select_next();
        assert_eq!(s.selected_idx(), Some(0));
        s.select_prev();
        assert_eq!(s.selected_idx(), None);
    }

    #[test]
    fn commit_uses_selected_suggestion() {
        let mut s = AddressBarState::default();
        s.open("ru");
        s.set_suggestions(vec![
            OmniboxSuggestion::HistoryFts {
                url: "https://rust-lang.org".into(),
                title: "Rust".into(),
                snippet: String::new(),
            },
        ]);
        s.select_next(); // selects index 0
        s.commit();
        assert_eq!(s.take_commit(), Some("https://rust-lang.org".to_owned()));
    }

    #[test]
    fn commit_falls_back_to_input_when_none_selected() {
        let mut s = AddressBarState::default();
        s.open("https://crates.io");
        s.set_suggestions(vec![
            OmniboxSuggestion::SearchQuery { query: "crates".into(), frequency: 2 },
        ]);
        // Don't select any suggestion.
        s.commit();
        assert_eq!(s.take_commit(), Some("https://crates.io".to_owned()));
    }

    #[test]
    fn append_resets_selection() {
        let mut s = AddressBarState::default();
        s.open("r");
        s.set_suggestions(vec![
            OmniboxSuggestion::SearchQuery { query: "rust".into(), frequency: 1 },
        ]);
        s.select_next();
        assert_eq!(s.selected_idx(), Some(0));
        s.append_str("u");
        assert_eq!(s.selected_idx(), None);
    }

    // ── @notes prefix ─────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_notes() {
        let (prefix, q) = parse_omnibox_prefix("@notes rust ownership");
        assert_eq!(prefix, OmniboxPrefix::Notes);
        assert_eq!(q, "rust ownership");
    }

    #[test]
    fn parse_prefix_notes_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@notes ");
        assert_eq!(prefix, OmniboxPrefix::Notes);
        assert_eq!(q, "");
    }

    #[test]
    fn parse_prefix_notes_no_match_for_plain() {
        let (prefix, _) = parse_omnibox_prefix("notes something");
        assert_eq!(prefix, OmniboxPrefix::Plain);
    }

    // ── @read-later prefix ──────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_read_later() {
        let (prefix, q) = parse_omnibox_prefix("@read-later rust book");
        assert_eq!(prefix, OmniboxPrefix::ReadLater);
        assert_eq!(q, "rust book");
    }

    #[test]
    fn parse_prefix_read_later_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@read-later ");
        assert_eq!(prefix, OmniboxPrefix::ReadLater);
        assert_eq!(q, "");
    }

    #[test]
    fn read_later_suggestion_commit_value_is_url() {
        let s = OmniboxSuggestion::ReadLater {
            url: "https://example.com/article".into(),
            title: "Article".into(),
            snippet: "an **article** snippet".into(),
        };
        assert_eq!(s.commit_value(), "https://example.com/article");
        assert_eq!(s.label(), "Article");
        assert_eq!(s.sub_label(), "an **article** snippet");
    }

    #[test]
    fn read_later_suggestion_label_falls_back_to_url() {
        let s = OmniboxSuggestion::ReadLater {
            url: "https://example.com/x".into(),
            title: String::new(),
            snippet: String::new(),
        };
        assert_eq!(s.label(), "https://example.com/x");
        assert_eq!(s.sub_label(), "https://example.com/x");
    }

    // ── @tabs prefix ────────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_tabs() {
        let (prefix, q) = parse_omnibox_prefix("@tabs github");
        assert_eq!(prefix, OmniboxPrefix::Tabs);
        assert_eq!(q, "github");
    }

    #[test]
    fn parse_prefix_tabs_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@tabs");
        assert_eq!(prefix, OmniboxPrefix::Tabs);
        assert_eq!(q, "");
    }

    #[test]
    fn tab_suggestion_commit_value_is_switch_sentinel() {
        let s = OmniboxSuggestion::Tab {
            title: "GitHub".into(),
            url: "https://github.com/".into(),
            switch_value: "switch-tab:42".into(),
        };
        assert_eq!(s.commit_value(), "switch-tab:42");
        assert_eq!(s.label(), "GitHub");
        assert_eq!(s.sub_label(), "https://github.com/");
    }

    // ── @bookmarks prefix ────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_bookmarks() {
        let (prefix, q) = parse_omnibox_prefix("@bookmarks rust");
        assert_eq!(prefix, OmniboxPrefix::Bookmarks);
        assert_eq!(q, "rust");
    }

    #[test]
    fn parse_prefix_bookmarks_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@bookmarks");
        assert_eq!(prefix, OmniboxPrefix::Bookmarks);
        assert_eq!(q, "");
    }

    #[test]
    fn bookmark_suggestion_commit_value_is_url() {
        let s = OmniboxSuggestion::Bookmark {
            title: "Rust".into(),
            url: "https://rust-lang.org/".into(),
            snippet: "a systems programming language".into(),
        };
        assert_eq!(s.commit_value(), "https://rust-lang.org/");
        assert_eq!(s.label(), "Rust");
        assert_eq!(s.sub_label(), "a systems programming language");
    }

    #[test]
    fn bookmark_suggestion_label_and_sub_label_fall_back_to_url() {
        let s = OmniboxSuggestion::Bookmark {
            title: String::new(),
            url: "https://example.com/x".into(),
            snippet: String::new(),
        };
        assert_eq!(s.label(), "https://example.com/x");
        assert_eq!(s.sub_label(), "https://example.com/x");
    }

    // ── @ai prefix ───────────────────────────────────────────────────────────────

    #[test]
    fn parse_prefix_ai() {
        let (prefix, q) = parse_omnibox_prefix("@ai what did I read about rust?");
        assert_eq!(prefix, OmniboxPrefix::Ai);
        assert_eq!(q, "what did I read about rust?");
    }

    #[test]
    fn parse_prefix_ai_empty_query() {
        let (prefix, q) = parse_omnibox_prefix("@ai");
        assert_eq!(prefix, OmniboxPrefix::Ai);
        assert_eq!(q, "");
    }

    #[test]
    fn ai_suggestion_commit_value_is_noop_sentinel() {
        let s = OmniboxSuggestion::Ai { answer: "Rust is a systems language.".into() };
        assert_eq!(s.commit_value(), "ai-answer:noop");
        assert_eq!(s.label(), "Rust is a systems language.");
        assert_eq!(s.sub_label(), "");
    }

    fn make_note_suggestion(note_id: i64) -> OmniboxSuggestion {
        OmniboxSuggestion::Note {
            url: "https://example.com/".into(),
            selection: "interesting text".into(),
            snippet: "interesting **text** here".into(),
            viewer_url: format!("note-viewer:{note_id}"),
        }
    }

    #[test]
    fn note_suggestion_commit_value_is_viewer_url() {
        let s = make_note_suggestion(7);
        assert_eq!(s.commit_value(), "note-viewer:7");
    }

    #[test]
    fn note_suggestion_label_is_selection() {
        let s = make_note_suggestion(1);
        assert_eq!(s.label(), "interesting text");
    }

    #[test]
    fn note_suggestion_sub_label_is_snippet() {
        let s = make_note_suggestion(2);
        assert_eq!(s.sub_label(), "interesting **text** here");
    }

    #[test]
    fn note_suggestion_sub_label_falls_back_to_url_when_snippet_empty() {
        let s = OmniboxSuggestion::Note {
            url: "https://example.com/".into(),
            selection: "sel".into(),
            snippet: String::new(),
            viewer_url: "note-viewer:3".into(),
        };
        assert_eq!(s.sub_label(), "https://example.com/");
    }

    // ── DS-6: IDN spoof guard ──────────────────────────────────────────────────

    #[test]
    fn guard_swaps_spoofed_host_to_punycode() {
        let (text, reason) = guard_display_text("https://аpple.com/login");
        assert_eq!(text, "https://xn--pple-43d.com/login");
        assert_eq!(reason, Some(SpoofReason::MixedScript));
    }

    #[test]
    fn guard_leaves_safe_host_unchanged() {
        let (text, reason) = guard_display_text("https://google.com/search");
        assert_eq!(text, "https://google.com/search");
        assert_eq!(reason, None);
    }

    #[test]
    fn guard_leaves_pure_cyrillic_rf_domain_unchanged() {
        let (text, reason) = guard_display_text("https://яндекс.рф/news");
        assert_eq!(text, "https://яндекс.рф/news");
        assert_eq!(reason, None);
    }

    #[test]
    fn guard_ignores_schemeless_and_sentinel_text() {
        assert_eq!(guard_display_text("rust async").0, "rust async");
        assert_eq!(guard_display_text("switch-tab:42").0, "switch-tab:42");
        assert_eq!(guard_display_text("").0, "");
    }

    #[test]
    fn commit_normalizes_spoofed_raw_input_to_punycode() {
        let mut s = AddressBarState::default();
        s.open("https://аpple.com/login");
        s.commit();
        assert_eq!(
            s.take_commit(),
            Some("https://xn--pple-43d.com/login".to_owned())
        );
    }

    #[test]
    fn commit_normalizes_spoofed_selected_suggestion_to_punycode() {
        let mut s = AddressBarState::default();
        s.open("a");
        s.set_suggestions(vec![OmniboxSuggestion::HistoryFts {
            url: "https://аpple.com/".into(),
            title: "Apple".into(),
            snippet: String::new(),
        }]);
        s.select_next();
        s.commit();
        assert_eq!(s.take_commit(), Some("https://xn--pple-43d.com/".to_owned()));
    }
}
