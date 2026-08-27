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

impl Lumen {
    pub(crate) fn handle_address_bar_key(
        &mut self,
        code: KeyCode,
        key_event: &KeyEvent,
        event_loop: &ActiveEventLoop,
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
        // `RedrawRequested` вЂ” every branch above mutates `self.address_bar`
        // (text, selection, or open/closed), so without this call the
        // on-screen field would keep showing stale text while the user
        // types. No-op off the flag (`Self::relayout_chrome_host` early-
        // returns when `chrome_doc` is `None`).
        self.relayout_chrome_host();
    }

    /// Process a committed omnibox value: resolve aliases, then navigate or act.
    ///
    /// Order: `sidebar:` prefix в†’ bang aliases (`!g`) в†’ `@notes` / `@read-later`
    /// в†’ record in search_history в†’ plain navigate.
    pub(crate) fn handle_omnibox_commit(&mut self, value: String) {
        // `view-source:<url>` вЂ” fetch and display syntax-highlighted source (В§D-2).
        if let Some(target_url) = value.trim().strip_prefix("view-source:") {
            let target_url = target_url.trim().to_owned();
            self.show_view_source_for_url(&target_url);
            return;
        }

        // `note-viewer:<id>` вЂ” open the note viewer overlay (В§12.2, GG-2).
        if let Some(id_str) = value.trim().strip_prefix("note-viewer:") {
            if let Ok(id) = id_str.parse::<i64>()
                && let Ok(Some(note)) = self.notes_store.get(id)
            {
                self.note_viewer.open(id, &note.url, &note.selection, &note.comment);
                self.request_redraw();
            }
            return;
        }

        // `switch-tab:<id>` вЂ” switch to an open tab by its stable id (В§12.4,
        // `@tabs` omnibox prefix). Resolve id в†’ current index (tabs reorder).
        if let Some(id_str) = value.trim().strip_prefix("switch-tab:") {
            if let Ok(id) = id_str.parse::<usize>()
                && let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == id)
            {
                self.switch_tab(idx);
            }
            return;
        }

        // `ai-answer:noop` вЂ” committing an `@ai` answer row is a no-op (В§12.5):
        // the RAG answer is already fully shown in the dropdown row itself,
        // there is no URL to navigate to.
        if value.trim() == "ai-answer:noop" {
            return;
        }

        // `about:settings` вЂ” open the browser settings overlay (task D-7).
        if value.trim() == "about:settings" {
            self.open_settings_panel();
            self.request_redraw();
            return;
        }

        // `about:newtab?...` вЂ” pin/unpin/"+"/restore-closed special links
        // (DS-11), committed e.g. by pasting a copied tile link.
        if let Some(action) = newtab::parse_action(value.trim()) {
            self.apply_newtab_action(action);
            return;
        }

        // `about:newtab` вЂ” internal start page with a speed dial of pinned +
        // most-visited sites (task CC-5, DS-11).
        if value.trim() == newtab::NEWTAB_URL {
            self.navigate_to(self.build_newtab_source());
            return;
        }

        // `about:chrome-preview` вЂ” CC-1 render-smoke for the engine-drawn
        // chrome asset (docs/tasks/p1-css-chrome.md).
        if value.trim() == chrome_preview::URL {
            self.navigate_to(PageSource::Static {
                html: chrome_preview::HTML.to_owned(),
                url: chrome_preview::URL.to_owned(),
            });
            return;
        }

        // `sidebar:<url>` вЂ” load the URL into the right-docked sidebar panel (7D.3).
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
                        eprintln!("sidebar: РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РіСЂСѓР·РёС‚СЊ {sidebar_url}: {err}");
                        // Open panel with placeholder so user sees feedback.
                        self.sidebar.open(sidebar_url);
                        // ADR-016 M2.2b-8: the sidebar becoming visible narrows the
                        // main page's content viewport вЂ” the same async-safe
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
                        // Р§РµСЂРµР· apply_http, Р° РЅРµ РіРѕР»С‹Рј HttpClient::new(): РёРЅР°С‡Рµ
                        // В«СЃРѕС…СЂР°РЅРёС‚СЊ РЅР° РїРѕС‚РѕРјВ» С…РѕРґРёС‚ РјРёРјРѕ HSTS, РїСЂРѕРєСЃРё, DoH Рё
                        // РєСЌС€Р° вЂ” СЃРІРѕРёРј, РЅРёС‡РµРј РЅРµ РЅР°СЃС‚СЂРѕРµРЅРЅС‹Рј РєР»РёРµРЅС‚РѕРј (BUG-402).
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

        // No alias matched вЂ” plain URL or search query.
        if !value.contains("://") && !value.starts_with('@') {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let _ = self.search_history.record(&value, now);
        }
        self.navigate_to(PageSource::from_arg(Some(&value)));
    }

    /// Р—Р°РїСЂР°С€РёРІР°РµС‚ РїРѕРґСЃРєР°Р·РєРё РґР»СЏ С‚РµРєСѓС‰РµРіРѕ РІРІРѕРґР° РІ Р°РґСЂРµСЃРЅРѕР№ СЃС‚СЂРѕРєРµ.
    ///
    /// `@history <query>` в†’ FTS5-РїРѕРёСЃРє РїРѕ РёСЃС‚РѕСЂРёРё СЃС‚СЂР°РЅРёС†.
    /// `@notes <query>` в†’ FTS5-РїРѕРёСЃРє РїРѕ Р·Р°РјРµС‚РєР°Рј (В§12.2).
    /// РћР±С‹С‡РЅС‹Р№ РІРІРѕРґ в†’ prefix-match РїРѕ search_history + FTS5.
    fn query_omnibox_suggestions(&self) -> Vec<address_bar::OmniboxSuggestion> {
        use address_bar::{OmniboxPrefix, OmniboxSuggestion, parse_omnibox_prefix};

        let input = self.address_bar.input();
        if input.is_empty() {
            return Vec::new();
        }

        let (prefix, query) = parse_omnibox_prefix(input);
        let mut suggestions = Vec::new();

        match prefix {
            OmniboxPrefix::History => {
                // @history <query> вЂ” С‚РѕР»СЊРєРѕ FTS.
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
                // @notes <query> вЂ” FTS5-РїРѕРёСЃРє РїРѕ Р·Р°РјРµС‚РєР°Рј В§12.2 (РґРѕ 5 СЂРµР·СѓР»СЊС‚Р°С‚РѕРІ).
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
                // @read-later <query> вЂ” FTS5-РїРѕРёСЃРє РїРѕ СЃРѕС…СЂР°РЅС‘РЅРЅС‹Рј СЃС‚СЂР°РЅРёС†Р°Рј В§12.3
                // (РґРѕ 7 СЂРµР·СѓР»СЊС‚Р°С‚РѕРІ). Р’С‹Р±РѕСЂ РїРѕРґСЃРєР°Р·РєРё в†’ РЅР°РІРёРіР°С†РёСЏ РЅР° URL.
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
                // @tabs <query> вЂ” РїРѕРґСЃС‚СЂРѕС‡РЅС‹Р№ РїРѕРёСЃРє РїРѕ РѕС‚РєСЂС‹С‚С‹Рј РІРєР»Р°РґРєР°Рј В§12.4
                // (Р·Р°РіРѕР»РѕРІРѕРє + URL), case-insensitive. РџСѓСЃС‚РѕР№ Р·Р°РїСЂРѕСЃ в†’ РІСЃРµ
                // РІРєР»Р°РґРєРё. Р’С‹Р±РѕСЂ РїРѕРґСЃРєР°Р·РєРё в†’ РїРµСЂРµРєР»СЋС‡РµРЅРёРµ РїРѕ СЃС‚Р°Р±РёР»СЊРЅРѕРјСѓ id.
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
                // @bookmarks <query> вЂ” РїРѕРґСЃС‚СЂРѕС‡РЅС‹Р№ РїРѕРёСЃРє РїРѕ Р·Р°РєР»Р°РґРєР°Рј В§12.8
                // (title/url/С‚РµРіРё), case-insensitive. РџСЂРё РЅР°Р»РёС‡РёРё AI-СЌРјР±РµРґРґРёРЅРіР°
                // Р·Р°РїСЂРѕСЃР° СЂРµР·СѓР»СЊС‚Р°С‚ РґРѕРїРѕР»РЅСЏРµС‚СЃСЏ cosine-similarity СЂР°РЅР¶РёСЂРѕРІР°РЅРёРµРј
                // РїРѕРІРµСЂС… С‚РµРєСЃС‚РѕРІС‹С… СЃРѕРІРїР°РґРµРЅРёР№ (РЅРµ Р·Р°РјРµРЅСЏРµС‚ РёС… вЂ” closes the loop
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
                // @ai <query> вЂ” РµРґРёРЅСЃС‚РІРµРЅРЅР°СЏ СЃС‚СЂРѕРєР°: RAG-РѕС‚РІРµС‚ (В§12.5) РїРѕРґ
                // `--features ai`, Р»РёР±Рѕ СЃС‚Р°С‚РёС‡РЅС‹Р№ hint РїРѕРґ РµС‘ РѕС‚СЃСѓС‚СЃС‚РІРёРµ
                // (СЃРј. `Self::ai_answer_for`, РѕР±Рµ РІРµС‚РєРё cfg-gated). РџСѓСЃС‚РѕР№
                // Р·Р°РїСЂРѕСЃ вЂ” РЅРё РѕРґРЅРѕР№ СЃС‚СЂРѕРєРё, РєР°Рє Сѓ РѕСЃС‚Р°Р»СЊРЅС‹С… РїСЂРµС„РёРєСЃРѕРІ.
                if !query.is_empty() {
                    suggestions.push(OmniboxSuggestion::Ai { answer: self.ai_answer_for(query) });
                }
            }
            OmniboxPrefix::Plain => {
                // prefix-match РїРѕ search_history (РґРѕ 4 СЃС‚СЂРѕРє).
                if let Ok(queries) = self.search_history.prefix_match(query, 4) {
                    for q in queries {
                        suggestions.push(OmniboxSuggestion::SearchQuery {
                            query: q.query,
                            frequency: q.frequency,
                        });
                    }
                }
                // URL/title substring match РїРѕ history_store (РґРѕ 5 СЃС‚СЂРѕРє).
                // Р”Р°С‘С‚ СЂРµР·СѓР»СЊС‚Р°С‚С‹ РїРѕ URL-С„СЂР°РіРјРµРЅС‚Сѓ РґР°Р¶Рµ Р±РµР· FTS5-РёРЅРґРµРєСЃР°.
                if let Ok(hits) = self.history_store.search_prefix(query, 5) {
                    for hit in hits {
                        suggestions.push(OmniboxSuggestion::HistoryFts {
                            url: hit.url,
                            title: hit.title,
                            snippet: String::new(),
                        });
                    }
                }
                // FTS5 РїРѕ РёСЃС‚РѕСЂРёРё СЃС‚СЂР°РЅРёС† (РґРѕ 4 СЃС‚СЂРѕРє, РёС‚РѕРіРѕ в‰¤ 8).
                if let Ok(hits) = self.history_fts.search(query, 4) {
                    for hit in hits {
                        // Р”РµРґСѓРїР»РёРєР°С†РёСЏ: FTS5 РјРѕР¶РµС‚ РїРѕРІС‚РѕСЂРёС‚СЊ URL РёР· search_prefix РІС‹С€Рµ.
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
