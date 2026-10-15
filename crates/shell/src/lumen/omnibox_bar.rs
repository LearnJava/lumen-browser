//! The address bar and its omnibox dropdown, shell side: what happens to the
//! text the user types there.
//!
//! Three questions, one per method - which key does what while the field has
//! focus, what a committed value means (internal `view-source:` /
//! `note-viewer:` / `switch-tab:` / `about:*` targets, then the user's own
//! `omnibox_aliases`, then a plain URL or search), and what the dropdown
//! offers while typing (`@history` / `@notes` / `@tabs` / `@ai` prefixes and
//! the prefix-match over `search_history`).
//!
//! The *widget* and its parsing helpers live in `crate::address_bar`, alias
//! resolution in `crate::omnibox`; what is here is the shell side that reaches
//! into the stores and drives navigation.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Behaviour, order
//! of operations and method bodies are unchanged; only the module path and
//! visibility (`fn` -> `pub(crate) fn`, required for callers in other
//! modules) differ.

use crate::*;

use super::text_input::write_clipboard_nonempty;

impl Lumen {
    pub(crate) fn handle_address_bar_key(
        &mut self,
        code: KeyCode,
        key_event: &KeyEvent,
        event_loop: &MainHandle<'_>,
    ) {
        let _ = event_loop;
        match code {
            KeyCode::Escape if !key_event.repeat => {
                self.address_bar.close();
                self.request_redraw();
            }
            KeyCode::Enter if !key_event.repeat => {
                self.address_bar.commit();
                if let Some(value) = self.address_bar.take_commit() {
                    self.handle_omnibox_commit(value);
                }
            }
            KeyCode::ArrowDown if !key_event.repeat => {
                self.address_bar.select_next();
                self.request_redraw();
            }
            KeyCode::ArrowUp if !key_event.repeat => {
                self.address_bar.select_prev();
                self.request_redraw();
            }
            KeyCode::Backspace => {
                self.address_bar.backspace();
                let sugg = self.query_omnibox_suggestions();
                self.address_bar.set_suggestions(sugg);
                self.request_redraw();
            }
            KeyCode::Delete => {
                self.address_bar.delete_forward();
                let sugg = self.query_omnibox_suggestions();
                self.address_bar.set_suggestions(sugg);
                self.request_redraw();
            }
            // UX-OMNI-EDIT: caret and selection. Shift extends the selection,
            // Ctrl jumps by word; the dropdown is not re-queried (the text
            // is unchanged), moving the caret just drops the highlighted row.
            KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::Home | KeyCode::End
                if !self.modifiers.alt_key() =>
            {
                let extend = self.modifiers.shift_key();
                let word = self.modifiers.control_key();
                match code {
                    KeyCode::ArrowLeft if word => self.address_bar.move_word_left(extend),
                    KeyCode::ArrowLeft => self.address_bar.move_left(extend),
                    KeyCode::ArrowRight if word => self.address_bar.move_word_right(extend),
                    KeyCode::ArrowRight => self.address_bar.move_right(extend),
                    KeyCode::Home => self.address_bar.move_home(extend),
                    _ => self.address_bar.move_end(extend),
                }
                self.request_redraw();
            }
            // «Выделить всё»: Ctrl+A, and the focus shortcuts again while the
            // field is already focused (Ctrl+L / F6 / Alt+D).
            KeyCode::KeyA | KeyCode::KeyL if self.modifiers == ModifiersState::CONTROL => {
                self.address_bar.select_all();
                self.request_redraw();
            }
            KeyCode::F6 if self.modifiers.is_empty() => {
                self.address_bar.select_all();
                self.request_redraw();
            }
            KeyCode::KeyD if self.modifiers == ModifiersState::ALT => {
                self.address_bar.select_all();
                self.request_redraw();
            }
            // UX-URL-COPY: Ctrl+C / Ctrl+X put the selection on the OS clipboard
            // (whole address → full readable URL, part → the visible substring);
            // Ctrl+V inserts clipboard text over the selection (line breaks dropped).
            KeyCode::KeyC if self.modifiers == ModifiersState::CONTROL => {
                if let Some(text) = self.address_bar.selected_text() {
                    write_clipboard_nonempty(&text);
                }
            }
            KeyCode::KeyX if self.modifiers == ModifiersState::CONTROL => {
                if let Some(text) = self.address_bar.cut_selection() {
                    write_clipboard_nonempty(&text);
                    let sugg = self.query_omnibox_suggestions();
                    self.address_bar.set_suggestions(sugg);
                    self.request_redraw();
                }
            }
            KeyCode::KeyV
                if self.modifiers == ModifiersState::CONTROL
                    || self.modifiers == (ModifiersState::CONTROL | ModifiersState::SHIFT) =>
            {
                use lumen_core::ext::ClipboardProvider;
                let text = platform::clipboard::PlatformClipboard.read_text();
                if !text.is_empty() {
                    self.address_bar.append_str(&text);
                    let sugg = self.query_omnibox_suggestions();
                    self.address_bar.set_suggestions(sugg);
                    self.request_redraw();
                }
            }
            // Ctrl+T while typing still opens a new tab (with a fresh, empty bar).
            KeyCode::KeyT if self.modifiers == ModifiersState::CONTROL && !key_event.repeat => {
                self.open_new_tab_for_typing();
            }
            _ => {
                if let Some(text) = key_event.text.as_ref()
                    && !text.is_empty()
                {
                    self.address_bar.append_str(text);
                    let sugg = self.query_omnibox_suggestions();
                    self.address_bar.set_suggestions(sugg);
                    self.request_redraw();
                }
            }
        }
        // CC-7 (docs/tasks/p1-css-chrome.md): `#omniInput`'s engine-rendered
        // value/warning/caret (`Self::chrome_model_snapshot`,
        // `Self::chrome_omni_input_rect`) is baked into `self.chrome_layout`
        // at `relayout_chrome_host` time, not recomputed every
        // `RedrawRequested` — every branch above mutates `self.address_bar`
        // (text, selection, or open/closed), so without this call the
        // on-screen field would keep showing stale text while the user
        // types. No-op off the flag (`Self::relayout_chrome_host` early-
        // returns when `chrome_doc` is `None`).
        self.relayout_chrome_host();
    }

    /// Process a committed omnibox value: resolve aliases, then navigate or act.
    ///
    /// Order: `sidebar:` prefix → bang aliases (`!g`) → `@notes` / `@read-later`
    /// → record in search_history → plain navigate.
    pub(crate) fn handle_omnibox_commit(&mut self, value: String) {
        // `view-source:<url>` — fetch and display syntax-highlighted source (§D-2).
        if let Some(target_url) = value.trim().strip_prefix("view-source:") {
            let target_url = target_url.trim().to_owned();
            self.show_view_source_for_url(&target_url);
            return;
        }

        // `note-viewer:<id>` — open the note viewer overlay (§12.2, GG-2).
        if let Some(id_str) = value.trim().strip_prefix("note-viewer:") {
            if let Ok(id) = id_str.parse::<i64>()
                && let Ok(Some(note)) = self.notes_store.get(id)
            {
                self.note_viewer.open(id, &note.url, &note.selection, &note.comment);
                self.relayout_chrome_host();
                self.request_redraw();
            }
            return;
        }

        // `switch-tab:<id>` — switch to an open tab by its stable id (§12.4,
        // `@tabs` omnibox prefix). Resolve id → current index (tabs reorder).
        if let Some(id_str) = value.trim().strip_prefix("switch-tab:") {
            if let Ok(id) = id_str.parse::<usize>()
                && let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == id)
            {
                self.switch_tab(idx);
            }
            return;
        }

        // `ai-answer:noop` — committing an `@ai` answer row is a no-op (§12.5):
        // the RAG answer is already fully shown in the dropdown row itself,
        // there is no URL to navigate to.
        if value.trim() == "ai-answer:noop" {
            return;
        }

        // `about:settings` — open the browser settings overlay (task D-7).
        if value.trim() == "about:settings" {
            self.open_settings_panel();
            self.request_redraw();
            return;
        }

        // `about:newtab?...` — pin/unpin/"+"/restore-closed special links
        // (DS-11), committed e.g. by pasting a copied tile link.
        if let Some(action) = newtab::parse_action(value.trim()) {
            self.apply_newtab_action(action);
            return;
        }

        // `about:handoff` — sites handed to another browser (COMPAT-HANDOFF).
        // Its `?remove=`/`?recheck=`/`?keep=` action links are only honoured
        // by a click on that page itself (`apply_handoff_link`), never here.
        if value.trim() == handoff_page::HANDOFF_URL {
            self.show_handoff_sites();
            return;
        }

        // `about:compat` — the compat pack: every record, per-record off switch
        // (COMPAT-PACK-PAGE). Its `?off=`/`?on=`/`?pack=` action links are only
        // honoured by a click on that page itself (`apply_compat_link`).
        if value.trim() == compat_page::COMPAT_URL {
            self.show_compat_page();
            return;
        }

        // `about:newtab` — internal start page with a speed dial of pinned +
        // most-visited sites (task CC-5, DS-11).
        if value.trim() == newtab::NEWTAB_URL {
            self.navigate_to(self.build_newtab_source());
            return;
        }

        // `about:chrome-preview` — CC-1 render-smoke for the engine-drawn
        // chrome asset (docs/tasks/p1-css-chrome.md).
        if value.trim() == chrome_preview::URL {
            self.navigate_to(PageSource::Static {
                html: chrome_preview::HTML.to_owned(),
                url: chrome_preview::URL.to_owned(),
            });
            return;
        }

        // `sidebar:<url>` — load the URL into the right-docked sidebar panel (7D.3).
        if let Some(sidebar_url) = value.strip_prefix("sidebar:") {
            let sidebar_url = sidebar_url.trim().to_owned();
            if !sidebar_url.is_empty() {
                let sink = Arc::clone(&self.event_sink);
                let src = PageSource::from_arg(Some(&sidebar_url));
                match src.load_bytes(sink, Some(self.active_cookie_jar())) {
                    Ok(raw) => {
                        self.open_sidebar_page(sidebar_url, &raw.bytes, String::new());
                    }
                    Err(err) => {
                        eprintln!("sidebar: не удалось загрузить {sidebar_url}: {err}");
                        // Open panel with placeholder so user sees feedback.
                        self.sidebar.open(sidebar_url);
                        // ADR-016 M2.2b-8: the sidebar becoming visible narrows the
                        // main page's content viewport — the same async-safe
                        // chrome-inset relayout the success path already routes off
                        // the UI thread (`open_sidebar_page`, M2.2b-3).
                        self.relayout_chrome();
                        self.request_redraw();
                    }
                }
            }
            return;
        }

        let aliases = self.omnibox_aliases.list_all().unwrap_or_default();
        if let Some(action) = omnibox::resolve(&value, &aliases) {
            match action {
                omnibox::AliasAction::Navigate(url) => {
                    self.navigate_to(PageSource::from_arg(Some(&url)));
                }
                omnibox::AliasAction::CreateNote(text) => {
                    self.notes.push(text);
                }
                omnibox::AliasAction::SaveReadLater(url) => {
                    // Spawn a background thread to fetch the page HTML and title.
                    // The result is sent back through `read_later_tx` and processed
                    // in `about_to_wait` via `read_later_rx`.
                    let tx = self.read_later_tx.clone();
                    let url_clone = url.clone();
                    std::thread::spawn(move || {
                        use lumen_core::ext::NetworkTransport;
                        use lumen_core::url::Url;
                        use lumen_network::HttpClient;
                        let Ok(parsed) = Url::parse(&url_clone) else { return };
                        // Через apply_http, а не голым HttpClient::new(): иначе
                        // «сохранить на потом» ходит мимо HSTS, прокси, DoH и
                        // кэша — своим, ничем не настроенным клиентом (BUG-402).
                        let client = crate::config::global().apply_http(HttpClient::new());
                        let Ok(html) = client.fetch(&parsed) else { return };
                        let title = panels::read_later_panel::extract_title_from_html(&html);
                        let title = if title.is_empty() { url_clone.clone() } else { title };
                        let _ = tx.send((url_clone, title, html));
                    });
                    // Also persist into the bookmark store under a dedicated
                    // folder so the bookmark manager panel shows it.
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = self.bookmarks.add(
                        &url,
                        &url,
                        "/Read Later",
                        &["read-later".to_owned()],
                        "",
                        now,
                    );
                    if self.bookmark_panel.visible {
                        self.refresh_bookmarks();
                    }
                }
            }
            return;
        }

        // No alias matched — plain URL or search query.
        if !value.contains("://") && !value.starts_with('@') {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let _ = self.search_history.record(&value, now);
        }
        self.navigate_to(PageSource::from_arg(Some(&value)));
    }

    /// Запрашивает подсказки для текущего ввода в адресной строке.
    ///
    /// `@history <query>` → FTS5-поиск по истории страниц.
    /// `@notes <query>` → FTS5-поиск по заметкам (§12.2).
    /// Обычный ввод → prefix-match по search_history + FTS5.
    pub(crate) fn query_omnibox_suggestions(&self) -> Vec<address_bar::OmniboxSuggestion> {
        use address_bar::{OmniboxPrefix, OmniboxSuggestion, parse_omnibox_prefix};

        let input = self.address_bar.input();
        if input.is_empty() {
            return Vec::new();
        }

        let (prefix, query) = parse_omnibox_prefix(input);
        let mut suggestions = Vec::new();

        match prefix {
            OmniboxPrefix::History => {
                // @history <query> — только FTS.
                if !query.is_empty() && let Ok(hits) = self.history_fts.search(query, 7) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::HistoryFts {
                            url: hit.url,
                            title: hit.title,
                            snippet: hit.snippet,
                        });
                    }
                }
            }
            OmniboxPrefix::Notes => {
                // @notes <query> — FTS5-поиск по заметкам §12.2 (до 5 результатов).
                if !query.is_empty() && let Ok(hits) = self.notes_store.search(query, 5) {
                    for hit in hits {
                        let viewer_url = format!("note-viewer:{}", hit.note.id);
                        suggestions.push(OmniboxSuggestion::Note {
                            url: hit.note.url,
                            selection: hit.note.selection,
                            snippet: hit.snippet,
                            viewer_url,
                        });
                    }
                }
            }
            OmniboxPrefix::ReadLater => {
                // @read-later <query> — FTS5-поиск по сохранённым страницам §12.3
                // (до 7 результатов). Выбор подсказки → навигация на URL.
                if !query.is_empty() && let Ok(hits) = self.read_later_store.search(query, 7) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::ReadLater {
                            url: hit.entry.url,
                            title: hit.entry.title,
                            snippet: hit.snippet,
                        });
                    }
                }
            }
            OmniboxPrefix::Tabs => {
                // @tabs <query> — подстрочный поиск по открытым вкладкам §12.4
                // (заголовок + URL), case-insensitive. Пустой запрос → все
                // вкладки. Выбор подсказки → переключение по стабильному id.
                let needle = query.to_lowercase();
                let active = self.tab_strip.active;
                for (idx, tab) in self.tab_strip.tabs.iter().enumerate() {
                    let url = if idx == active {
                        self.source.url_str().unwrap_or("").to_owned()
                    } else {
                        self.bg_tabs
                            .get(&tab.id)
                            .and_then(|s| s.source.url_str().map(str::to_owned))
                            .unwrap_or_default()
                    };
                    if needle.is_empty()
                        || tab.title.to_lowercase().contains(&needle)
                        || url.to_lowercase().contains(&needle)
                    {
                        suggestions.push(OmniboxSuggestion::Tab {
                            title: tab.title.clone(),
                            url,
                            switch_value: format!("switch-tab:{}", tab.id),
                        });
                    }
                    if suggestions.len() >= 8 {
                        break;
                    }
                }
            }
            OmniboxPrefix::Bookmarks => {
                // @bookmarks <query> — подстрочный поиск по закладкам §12.8
                // (title/url/теги), case-insensitive. При наличии AI-эмбеддинга
                // запроса результат дополняется cosine-similarity ранжированием
                // поверх текстовых совпадений (не заменяет их — closes the loop
                // for bookmarks that don't textually match but are related).
                if let Ok(bookmarks) = self.bookmarks.list_all() {
                    let needle = query.to_lowercase();
                    let query_embedding = if query.is_empty() {
                        Vec::new()
                    } else {
                        self.ai_backend.embed(query)
                    };
                    // Score: text matches always outrank pure-semantic ones (base
                    // 1.0 + similarity as tie-break); semantic-only matches keep
                    // their raw similarity so they still sort by relevance.
                    let mut scored: Vec<(f32, &lumen_storage::bookmarks::Bookmark)> = bookmarks
                        .iter()
                        .filter_map(|b| {
                            let text_match = needle.is_empty()
                                || b.title.to_lowercase().contains(&needle)
                                || b.url.to_lowercase().contains(&needle)
                                || b.tags.iter().any(|t| t.to_lowercase().contains(&needle));
                            let similarity = if !query_embedding.is_empty()
                                && let Some(emb) = &b.embedding
                            {
                                lumen_storage::bookmarks::cosine_similarity(
                                    &query_embedding,
                                    &lumen_storage::bookmarks::embedding_from_bytes(emb),
                                )
                            } else {
                                0.0
                            };
                            if !text_match && similarity <= 0.5 {
                                return None;
                            }
                            let score = if text_match { 1.0 + similarity } else { similarity };
                            Some((score, b))
                        })
                        .collect();
                    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                    for (_, b) in scored.into_iter().take(7) {
                        suggestions.push(OmniboxSuggestion::Bookmark {
                            title: b.title.clone(),
                            url: b.url.clone(),
                            snippet: b.summary.clone().unwrap_or_default(),
                        });
                    }
                }
            }
            OmniboxPrefix::Ai => {
                // @ai <query> — единственная строка: RAG-ответ (§12.5) под
                // `--features ai`, либо статичный hint под её отсутствие
                // (см. `Self::ai_answer_for`, обе ветки cfg-gated). Пустой
                // запрос — ни одной строки, как у остальных префиксов.
                if !query.is_empty() {
                    suggestions.push(OmniboxSuggestion::Ai { answer: self.ai_answer_for(query) });
                }
            }
            OmniboxPrefix::Plain => {
                // prefix-match по search_history (до 4 строк).
                if let Ok(queries) = self.search_history.prefix_match(query, 4) {
                    for q in queries {
                        suggestions.push(OmniboxSuggestion::SearchQuery {
                            query: q.query,
                            frequency: q.frequency,
                        });
                    }
                }
                // URL/title substring match по history_store (до 5 строк).
                // Даёт результаты по URL-фрагменту даже без FTS5-индекса.
                if let Ok(hits) = self.history_store.search_prefix(query, 5) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::HistoryFts {
                            url: hit.url,
                            title: hit.title,
                            snippet: String::new(),
                        });
                    }
                }
                // FTS5 по истории страниц (до 4 строк, итого ≤ 8).
                if let Ok(hits) = self.history_fts.search(query, 4) {
                    for hit in hits {
                        // Дедупликация: FTS5 может повторить URL из search_prefix выше.
                        if !suggestions.iter().any(|s| {
                            matches!(s, OmniboxSuggestion::HistoryFts { url, .. } if url == &hit.url)
                        }) {
                            suggestions.push(OmniboxSuggestion::HistoryFts {
                                url: hit.url,
                                title: hit.title,
                                snippet: hit.snippet,
                            });
                        }
                    }
                }
            }
        }

        suggestions
    }
}

/// UX-OMNI-EDIT: left inset of the text inside `#omniInput` (the UA's 2px
/// horizontal padding; the asset sets `border:none`) — the same constant
/// `forms::input_caret_rect` uses.
const OMNI_TEXT_INSET: f32 = 2.0;

/// The field's rect and the x (window CSS px) of every glyph edge of `input`
/// at `font_size`: `edges[i]` is the gap before the `i`-th character,
/// `edges[len]` the end of the text. Measured with the bundled Inter, so it
/// is per-glyph (not a fixed advance) but ignores kerning.
fn omnibox_glyph_edges(field: Rect, input: &str, font_size: f32) -> Option<Vec<f32>> {
    use lumen_layout::TextMeasurer;
    let font = lumen_font::Font::parse(INTER_FONT).ok()?;
    let m = lumen_paint::FontMeasurer::new(&font).ok()?;
    let mut x = field.x + OMNI_TEXT_INSET;
    let mut edges = Vec::with_capacity(input.len() + 1);
    edges.push(x);
    for c in input.chars() {
        x += m.char_width(c, font_size);
        edges.push(x);
    }
    Some(edges)
}

/// Index of the glyph gap nearest to `x` (what a click at `x` puts the caret on).
fn nearest_edge(edges: &[f32], x: f32) -> usize {
    edges
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
        .map_or(0, |(i, _)| i)
}

impl Lumen {
    /// Caret bar of the open address bar at the glyph gap under
    /// `AddressBarState::cursor`, in window CSS px. Unlike the old flush-right
    /// placement it also exists in an empty field. `None` while a dropdown row
    /// is selected or there is no chrome layout yet; the painter additionally
    /// hides it over a live selection (the highlight stands in for the caret),
    /// the IME anchor keeps using it there.
    pub(crate) fn omnibox_caret_rect(&self) -> Option<Rect> {
        if !self.address_bar.is_open() || self.address_bar.selected_idx().is_some() {
            return None;
        }
        let field = self.chrome_omni_input_rect?;
        let edges = omnibox_glyph_edges(field, self.address_bar.input(), self.chrome_omni_font_size)?;
        let x = edges.get(self.address_bar.cursor()).copied()?;
        // Text wider than the field scrolls in the real input; keep the bar inside it.
        let x = x.min(field.x + field.width - 4.0);
        Some(Rect::new(x, field.y + 4.0, 2.0, (field.height - 8.0).max(0.0)))
    }

    /// Translucent highlight over the selected part of the address bar text,
    /// clipped to the field. `None` without a selection.
    pub(crate) fn omnibox_selection_overlay(&self, pal: &crate::panels::themes::Palette) -> Option<lumen_paint::DisplayList> {
        if !self.address_bar.is_open() || self.address_bar.selected_idx().is_some() {
            return None;
        }
        let (start, end) = self.address_bar.selection()?;
        let field = self.chrome_omni_input_rect?;
        let edges = omnibox_glyph_edges(field, self.address_bar.input(), self.chrome_omni_font_size)?;
        let (x0, x1) = (*edges.get(start)?, *edges.get(end)?);
        let rect = Rect::new(x0, field.y + 3.0, (x1 - x0).max(0.0), (field.height - 6.0).max(0.0));
        Some(vec![
            lumen_paint::DisplayCommand::PushClipRect { rect: field },
            lumen_paint::DisplayCommand::FillRect { rect, color: lumen_layout::Color { a: 90, ..pal.accent } },
            lumen_paint::DisplayCommand::PopClip,
        ])
    }

    /// Click inside the already-focused address bar field: put the caret on
    /// the glyph gap nearest to `x` (window CSS px), collapsing any selection.
    pub(crate) fn omnibox_place_caret_at(&mut self, x: f32) {
        let Some(field) = self.chrome_omni_input_rect else { return };
        let Some(edges) = omnibox_glyph_edges(field, self.address_bar.input(), self.chrome_omni_font_size) else {
            return;
        };
        self.address_bar.set_cursor(nearest_edge(&edges, x), false);
    }
}

#[cfg(test)]
mod omnibox_geometry_tests {
    use super::*;

    #[test]
    fn nearest_edge_picks_closest_gap() {
        let edges = [10.0, 18.0, 30.0];
        assert_eq!(nearest_edge(&edges, 0.0), 0);
        assert_eq!(nearest_edge(&edges, 13.9), 0);
        assert_eq!(nearest_edge(&edges, 14.1), 1);
        assert_eq!(nearest_edge(&edges, 25.0), 2);
        assert_eq!(nearest_edge(&edges, 999.0), 2);
    }

    #[test]
    fn glyph_edges_grow_per_glyph_and_start_at_the_inset() {
        let field = Rect::new(100.0, 10.0, 400.0, 28.0);
        let edges = omnibox_glyph_edges(field, "il W", 12.5).expect("bundled font parses");
        assert_eq!(edges.len(), 5);
        assert_eq!(edges[0], 102.0);
        assert!(edges.windows(2).all(|w| w[1] > w[0]));
        // Per-glyph, not a fixed advance: narrow «i» is thinner than wide «W».
        assert!(edges[1] - edges[0] < edges[4] - edges[3]);
        assert_eq!(omnibox_glyph_edges(field, "", 12.5).expect("parses"), vec![102.0]);
    }
}
