//! Адресная строка: состояние омнибокса и его сборка в display list.
//!
//! DS-10 — омнибокс постоянно виден как инлайн-поле в центре тулбара
//! (`toolbar.rs`), а не как Ctrl+L-оверлей поверх всего окна. Этот модуль не
//! знает о геометрии тулбара (кластеры кнопок, `CHROME_H`) — `toolbar.rs`
//! считает раскладку (`omnibox_rects`) и передаёт сюда уже готовые `Rect`-ы;
//! здесь только состояние (`AddressBarState`) и чистый рендер по этим rect-ам:
//! `build_inline_field` — всегда видимое поле (не в фокусе — текущий URL,
//! в фокусе — редактируемый ввод), `build_dropdown` — список подсказок,
//! рисуется отдельно (anchored под тулбаром) только пока бар открыт.
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
//! хост признан спуф-риском, отображается/коммитится его Punycode-форма,
//! а под полем ввода рисуется красная строка-предупреждение.

use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};
use lumen_core::geom::Rect;
use lumen_core::idn::{HostDisplay, SpoofReason, display_host};
use lumen_core::url::Url;

use crate::panels::themes::Palette;
use crate::theme_tokens::radius;

// ── Визуальные константы ──────────────────────────────────────────────────────
//
// Surface/text colours are theme-driven via [`Palette`] (passed into
// `build_inline_field`/`build_dropdown`). The focus ring and caret follow
// `pal.accent` (design-system prototype: `.omnibox:focus-within{
// border-color:var(--accent) }`), so both track the active profile's accent
// colour. The result-tag legend below stays theme-invariant by design: it is
// a fixed per-category colour code (history/notes/tabs/…), same rationale as
// the lifecycle/container colours excluded from `Palette` in `tabs/strip.rs`.

/// Tag accent for FTS-history omnibox results — historically shared the same
/// blue as the focus ring; kept as its own constant now that the ring reads
/// `pal.accent` instead.
const HISTORY_TAG: Color = Color { r: 60, g: 120, b: 220, a: 255 };
/// Green tag accent for search-query omnibox results.
const ITEM_TAG: Color = Color { r: 72, g: 150, b: 90, a: 255 };

/// Font size of the field's own text (URL / editable input) — the design
/// reference's `.omnibox input{ font-size:12.5px }`, rounded to a value the
/// bundled fonts render cleanly at.
const FONT: f32 = 13.0;
/// Высота одной строки в dropdown.
const ITEM_H: f32 = 36.0;
const ITEM_LABEL_SZ: f32 = 13.0;
const ITEM_SUB_SZ: f32 = 11.0;
const ITEM_PAD: f32 = 8.0;
/// Максимум строк в dropdown.
const MAX_VISIBLE: usize = 7;
/// Максимальная длина строки ввода. Защита от случайной paste-атаки.
const MAX_INPUT_LEN: usize = 2048;
/// Высота красной строки-предупреждения о спуфинге (DS-6), рисуется под
/// полем ввода перед dropdown, если он есть.
const WARN_H: f32 = 22.0;

/// Фон строки-предупреждения о спуфинге — фиксированный красный вне
/// зависимости от темы (сигнал безопасности должен быть читаем в обеих
/// палитрах, та же логика что у `HISTORY_TAG`/`ITEM_TAG` выше).
const SPOOF_WARNING_BG: Color = Color { r: 130, g: 24, b: 24, a: 235 };
/// Цвет текста поверх `SPOOF_WARNING_BG`.
const SPOOF_WARNING_TEXT: Color = Color { r: 255, g: 220, b: 220, a: 255 };

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
        /// Частота использования — отображается как подсказка.
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

    /// Короткий тег-маркер типа для правой части строки.
    /// Для `SearchQuery` включает счётчик использований если > 1.
    fn tag(&self) -> String {
        match self {
            OmniboxSuggestion::HistoryFts { .. } => "история".to_string(),
            OmniboxSuggestion::Note { .. } => "заметка".to_string(),
            OmniboxSuggestion::SearchQuery { frequency, .. } if *frequency > 1 => {
                format!("×{frequency}")
            }
            OmniboxSuggestion::SearchQuery { .. } => "запрос".to_string(),
            OmniboxSuggestion::ReadLater { .. } => "позже".to_string(),
            OmniboxSuggestion::Tab { .. } => "вкладка".to_string(),
            OmniboxSuggestion::Bookmark { .. } => "закладка".to_string(),
            OmniboxSuggestion::Ai { .. } => "ai".to_string(),
        }
    }

    fn tag_color(&self) -> Color {
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
}

// ── Рендер ────────────────────────────────────────────────────────────────────

/// Rects for the always-visible chrome of the inline omnibox field. Computed
/// by `toolbar::omnibox_rects` (which owns the toolbar's cluster layout) —
/// this module only draws into them, it has no knowledge of button-cluster
/// geometry.
#[derive(Debug, Clone, Copy)]
pub struct FieldRects {
    /// The field's own background/border box.
    pub field: Rect,
    /// TLS padlock icon-button, left-aligned inside `field`.
    pub lock: Rect,
    /// Text area between `lock` and `star` — host / editable input + caret.
    pub text: Rect,
    /// Bookmark ("star") icon-button.
    pub star: Rect,
    /// Shields icon-button, right-aligned inside `field`.
    pub shield: Rect,
}

/// Uniform corner radii helper (mirrors the identically-named private helper
/// in other chrome modules, e.g. `toolbar.rs`, `page_context_menu.rs`).
fn corners(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r }
}

/// Push one icon glyph centered in `rect`, no background (lock/star/shield
/// inside the omnibox — unlike toolbar's nav/action buttons, these never
/// render a highlighted "on" background).
fn push_icon(out: &mut DisplayList, rect: Rect, glyph: &str, color: Color) {
    out.push(DisplayCommand::DrawText {
        rect,
        text: glyph.to_owned(),
        font_size: rect.height.min(rect.width),
        color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: None,
    });
}

/// Собирает display list всегда-видимого поля омнибокса: фон+рамка (accent
/// кольцо в фокусе — design-system `.omnibox:focus-within{
/// border-color:var(--accent) }`), замок/звезда/щит и текст. Не в фокусе —
/// `current_url` (IDN-guarded); в фокусе — живой ввод/выделенная подсказка +
/// курсор, как раньше в overlay, и (если хост спуф-риск) красная строка
/// предупреждения под полем.
///
/// Не рисует dropdown — см. `build_dropdown`.
pub fn build_inline_field(
    state: &AddressBarState,
    current_url: &str,
    rects: &FieldRects,
    pal: &Palette,
) -> DisplayList {
    let focused = state.is_open();
    let mut out = DisplayList::with_capacity(8);

    // Рамка (на 1px шире с каждой стороны, тот же приём что был у overlay-бара).
    let border_color = if focused { pal.accent } else { pal.divider };
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(
            rects.field.x - 1.0,
            rects.field.y - 1.0,
            rects.field.width + 2.0,
            rects.field.height + 2.0,
        ),
        radii: corners(radius::MD + 1.0),
        color: border_color,
    });
    out.push(DisplayCommand::FillRoundedRect {
        rect: rects.field,
        radii: corners(radius::MD),
        color: if focused { pal.overlay_bg } else { pal.input_bg },
    });

    push_icon(&mut out, rects.lock, "\u{1F512}", pal.text_dim);
    push_icon(&mut out, rects.star, "\u{2606}", pal.text_dim);
    push_icon(&mut out, rects.shield, "\u{1F6E1}", pal.text_dim);

    // Отображаем строку выделенной подсказки в input field если она выбрана.
    let sugg = state.suggestions();
    let display_input = if let Some(idx) = state.selected_idx() {
        sugg.get(idx).map(|s| s.commit_value()).unwrap_or(state.input())
    } else {
        state.input()
    };
    let is_empty = focused && display_input.is_empty();
    let (display_text, text_color, spoof_reason) = if !focused {
        let (guarded, reason) = guard_display_text(current_url);
        let text = if guarded.is_empty() { "about:blank".to_string() } else { guarded };
        (text, pal.text, reason)
    } else if is_empty {
        ("Введите URL или поисковый запрос…".to_string(), pal.text_dim, None)
    } else {
        let (guarded, reason) = guard_display_text(display_input);
        (guarded, pal.text, reason)
    };

    let text_margin = 4.0;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(
            rects.text.x + text_margin,
            rects.text.y + (rects.text.height - FONT * 1.2) * 0.5,
            (rects.text.width - text_margin * 2.0).max(0.0),
            FONT * 1.2,
        ),
        text: display_text,
        font_size: FONT,
        color: text_color,
        // DS-4: omnibox URL text is monospace (bundled JetBrains Mono) while
        // focused; not focused it stays the default chrome UI font (Golos
        // Text, resolved from empty font_family) matching the rest of the row.
        font_family: if focused { vec!["JetBrains Mono".to_string()] } else { Vec::new() },
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: None,
    });

    // Курсор — вертикальная линия. Только в фокусе, не рисуется если выбрана подсказка.
    if focused && !is_empty && state.selected_idx().is_none() {
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(
                rects.text.x + rects.text.width - text_margin - 2.0,
                rects.text.y + (rects.text.height - FONT * 1.2) * 0.5,
                2.0,
                FONT * 1.2,
            ),
            color: Color { a: 220, ..pal.accent },
        });
    }

    // ─ Спуфинг-предупреждение (DS-6) ────────────────────────────────────────

    if focused && let Some(reason) = spoof_reason {
        let warn_y = rects.field.y + rects.field.height + 4.0;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(rects.field.x, warn_y, rects.field.width, WARN_H),
            color: SPOOF_WARNING_BG,
        });
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(
                rects.field.x + 8.0,
                warn_y + (WARN_H - ITEM_SUB_SZ * 1.3) * 0.5,
                (rects.field.width - 16.0).max(0.0),
                ITEM_SUB_SZ * 1.3,
            ),
            text: spoof_warning_message(reason).to_string(),
            font_size: ITEM_SUB_SZ,
            color: SPOOF_WARNING_TEXT,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        });
    }

    out
}

/// Собирает display list dropdown-подсказок. Вызывается только пока
/// `state.is_open()`; anchored под тулбаром (`dropdown_top` = `toolbar::
/// CHROME_H`, а не сразу под полем — DS-10 шаг 3), шириной поля `field`.
/// Пустой список, если подсказок нет.
pub fn build_dropdown(
    state: &AddressBarState,
    field: Rect,
    dropdown_top: f32,
    pal: &Palette,
) -> DisplayList {
    let sugg = state.suggestions();
    let n_visible = sugg.len().min(MAX_VISIBLE);
    if n_visible == 0 {
        return DisplayList::new();
    }
    let drop_h = n_visible as f32 * ITEM_H;
    let drop_x = field.x;
    let drop_w = field.width;
    let drop_y = dropdown_top;

    let mut out = DisplayList::with_capacity(2 + n_visible * 4);

    // Граница dropdown.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(drop_x - 1.0, drop_y, drop_w + 2.0, drop_h + 1.0),
        color: pal.overlay_border,
    });
    // Фон dropdown.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(drop_x, drop_y, drop_w, drop_h),
        color: pal.item_bg,
    });

    for (i, s) in sugg.iter().take(n_visible).enumerate() {
        let iy = drop_y + i as f32 * ITEM_H;
        let selected = state.selected_idx() == Some(i);

        if selected {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(drop_x, iy, drop_w, ITEM_H),
                color: pal.item_selected_bg,
            });
        }

        // DS-6: подсказки часто несут URL в label/sub_label (история,
        // вкладки, закладки) — прогоняем их через тот же спуф-guard, что и
        // основное поле ввода, чтобы дропдаун не подсвечивал спуф-домен.
        let label = guard_display_text(s.label()).0;
        let sub = guard_display_text(s.sub_label()).0;
        let tag = s.tag(); // String

        let has_sub = !sub.is_empty();
        if has_sub {
            // Двухстрочный layout: label сверху, sub снизу.
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(
                    drop_x + ITEM_PAD,
                    iy + 4.0,
                    drop_w - ITEM_PAD * 3.0 - 60.0,
                    ITEM_LABEL_SZ * 1.3,
                ),
                text: label,
                font_size: ITEM_LABEL_SZ,
                color: pal.text,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: None,
            });
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(
                    drop_x + ITEM_PAD,
                    iy + 4.0 + ITEM_LABEL_SZ * 1.3 + 1.0,
                    drop_w - ITEM_PAD * 3.0 - 60.0,
                    ITEM_SUB_SZ * 1.3,
                ),
                text: sub,
                font_size: ITEM_SUB_SZ,
                color: pal.text_dim,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: None,
            });
        } else {
            // Одна строка по центру высоты строки.
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(
                    drop_x + ITEM_PAD,
                    iy + (ITEM_H - ITEM_LABEL_SZ * 1.3) * 0.5,
                    drop_w - ITEM_PAD * 3.0 - 60.0,
                    ITEM_LABEL_SZ * 1.3,
                ),
                text: label,
                font_size: ITEM_LABEL_SZ,
                color: pal.text,
                font_family: Vec::new(),
                font_weight: FontWeight::NORMAL,
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                font_features: Vec::new(),
                font_palette: None,
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: None,
            });
        }

        // Тег справа.
        out.push(DisplayCommand::DrawText {
            rect: Rect::new(
                drop_x + drop_w - 58.0,
                iy + (ITEM_H - ITEM_SUB_SZ * 1.3) * 0.5,
                54.0,
                ITEM_SUB_SZ * 1.3,
            ),
            text: tag,
            font_size: ITEM_SUB_SZ,
            color: s.tag_color(),
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
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

    /// Fixed field geometry for tests — mirrors what `toolbar::omnibox_rects`
    /// would produce, but without depending on that module.
    fn test_rects() -> FieldRects {
        FieldRects {
            field: Rect::new(100.0, 2.0, 680.0, 32.0),
            lock: Rect::new(108.0, 7.0, 22.0, 22.0),
            text: Rect::new(138.0, 2.0, 550.0, 32.0),
            star: Rect::new(720.0, 7.0, 22.0, 22.0),
            shield: Rect::new(750.0, 7.0, 22.0, 22.0),
        }
    }

    #[test]
    fn field_shows_input_text_when_open() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("https://example.com");
            x
        };
        let dl = build_inline_field(&s, "https://example.com", &test_rects(), &Palette::DARK);
        let has_text = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("example.com"))
        });
        assert!(has_text);
    }

    #[test]
    fn field_shows_placeholder_when_open_and_empty() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("");
            x
        };
        let dl = build_inline_field(&s, "", &test_rects(), &Palette::DARK);
        let has_placeholder = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("URL"))
        });
        assert!(has_placeholder);
    }

    #[test]
    fn field_shows_current_url_when_not_focused() {
        let s = AddressBarState::default();
        let dl = build_inline_field(&s, "https://example.com/page", &test_rects(), &Palette::DARK);
        let has_text = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("example.com/page"))
        });
        assert!(has_text);
    }

    #[test]
    fn field_has_border_rect_as_first_cmd_when_focused() {
        let s = {
            let mut x = AddressBarState::default();
            x.open("x");
            x
        };
        let dl = build_inline_field(&s, "x", &test_rects(), &Palette::DARK);
        assert!(matches!(dl[0], DisplayCommand::FillRoundedRect { color, .. } if color == Palette::DARK.accent));
    }

    #[test]
    fn field_border_is_not_accent_when_not_focused() {
        let s = AddressBarState::default();
        let dl = build_inline_field(&s, "https://example.com", &test_rects(), &Palette::DARK);
        assert!(matches!(dl[0], DisplayCommand::FillRoundedRect { color, .. } if color == Palette::DARK.divider));
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

    #[test]
    fn dropdown_rendered_for_suggestions() {
        let mut s = AddressBarState::default();
        s.open("rust");
        s.set_suggestions(vec![
            OmniboxSuggestion::HistoryFts {
                url: "https://rust-lang.org".into(),
                title: "Rust Programming Language".into(),
                snippet: "Systems programming".into(),
            },
            OmniboxSuggestion::SearchQuery { query: "rust async".into(), frequency: 5 },
        ]);
        let dl = build_dropdown(&s, test_rects().field, 40.0, &Palette::DARK);
        let text_count = dl.iter().filter(|c| matches!(c, DisplayCommand::DrawText { .. })).count();
        // label1 + sub1 + tag1 + label2 + tag2 >= 5
        assert!(text_count >= 5);
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

    #[test]
    fn field_shows_warning_strip_for_spoofed_input() {
        let mut s = AddressBarState::default();
        s.open("https://аpple.com/login");
        let dl = build_inline_field(&s, "https://аpple.com/login", &test_rects(), &Palette::DARK);
        let has_warning = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("Punycode"))
        });
        assert!(has_warning);
        let has_punycode_url = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("xn--pple-43d.com"))
        });
        assert!(has_punycode_url);
    }

    #[test]
    fn field_has_no_warning_strip_for_safe_input() {
        let mut s = AddressBarState::default();
        s.open("https://google.com");
        let dl = build_inline_field(&s, "https://google.com", &test_rects(), &Palette::DARK);
        let has_warning = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("Punycode"))
        });
        assert!(!has_warning);
    }

    #[test]
    fn dropdown_suggestion_url_is_punycode_guarded() {
        let mut s = AddressBarState::default();
        s.open("a");
        s.set_suggestions(vec![OmniboxSuggestion::HistoryFts {
            url: "https://аpple.com/".into(),
            title: String::new(),
            snippet: String::new(),
        }]);
        let dl = build_dropdown(&s, test_rects().field, 40.0, &Palette::DARK);
        let has_punycode_label = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("xn--pple-43d.com"))
        });
        assert!(has_punycode_label);
    }
}
