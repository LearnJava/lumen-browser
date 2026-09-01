//! Writing and reading back `last_session.lsession` - the whole window's
//! tab set as it stood when the browser was last closed.
//!
//! The file format and the SQLite side live in `crate::session_persist`; what
//! is here is the shell's own reading of "what counts as the current session":
//! which tabs are worth saving, which page of a split view is the live one and
//! what has to be reopened before the first frame is drawn. Writes are silent
//! by design - a failure to save must not break window close.

use crate::*;

impl Lumen {
    /// Сохранить текущую вкладку в `last_session.lsession` при закрытии окна.
    ///
    /// Silent — ошибки записи не ломают выход. Не сохраняет Empty-страницу.
    pub(crate) fn save_session_on_close(&self) {
        let url = match &self.source {
            PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => return,
            PageSource::File(p) => p.display().to_string(),
            PageSource::Url(u) => u.clone(),
            PageSource::Snapshot { base_url, .. } => base_url.clone(),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let file = SessionFile {
            version: 1,
            name: format!("auto-save {now}"),
            created_at: now,
            tabs: vec![ExportedTab {
                url,
                title: self.title.clone().unwrap_or_default(),
                scroll_x: self.scroll_x,
                scroll_y: self.scroll_y,
                is_active: true,
            }],
        };
        let json = session_export::to_json(&file);
        let _ = std::fs::write("last_session.lsession", json.as_bytes());
    }

    /// Persist every open tab (URL + title + scroll + serialised DOM) to the
    /// SQLite session store on window close (§10I).
    ///
    /// Walks the tab strip in left-to-right order, pulling each tab's state from
    /// whichever slot holds it: the active tab from `self`, background tabs from
    /// `bg_tabs`, hibernated tabs from `tab_snapshots`. Tabs without a real URL
    /// (blank, never-loaded) are skipped. Silent — write errors do not block exit.
    pub(crate) fn save_full_session(&self) {
        let mut tabs: Vec<lumen_storage::PersistedTab> = Vec::new();
        let active_idx = self.tab_strip.active;
        for (idx, entry) in self.tab_strip.tabs.iter().enumerate() {
            let persisted = if idx == active_idx {
                source_url_string(&self.source).map(|url| lumen_storage::PersistedTab {
                    url,
                    title: self.title.clone().unwrap_or_default(),
                    scroll_x: self.scroll_x,
                    scroll_y: self.scroll_y,
                    is_active: true,
                    dom_blob: dom_blob_of(self.layout_source.as_ref()),
                })
            } else if let Some(snap) = self.bg_tabs.get(&entry.id) {
                source_url_string(&snap.source).map(|url| lumen_storage::PersistedTab {
                    url,
                    title: snap.title.clone().unwrap_or_default(),
                    scroll_x: snap.scroll_x,
                    scroll_y: snap.scroll_y,
                    is_active: false,
                    dom_blob: dom_blob_of(snap.layout_source.as_ref()),
                })
            } else if self.hibernated_tabs.contains_key(&entry.id) {
                // DOM blob already on disk in tab_snapshots — copy it over.
                match self.tab_snapshots.fetch(entry.id as i64) {
                    Ok(Some(data)) if !data.url.is_empty() => Some(lumen_storage::PersistedTab {
                        url: data.url,
                        title: data.title,
                        scroll_x: data.scroll_x,
                        scroll_y: data.scroll_y,
                        is_active: false,
                        dom_blob: data.dom_blob,
                    }),
                    _ => None,
                }
            } else {
                None // Blank / never-loaded tab.
            };
            if let Some(t) = persisted {
                tabs.push(t);
            }
        }

        if let Err(e) = self.session_store.save(&tabs) {
            eprintln!("session: не удалось сохранить сессию: {e}");
        }
    }

    /// Reopen the tabs saved by [`Self::save_full_session`] (§10I).
    ///
    /// Called once at launch only when the user started the browser with no
    /// explicit page (so we do not clobber an `argv`-requested page). The
    /// previously-active tab's source + scroll are installed into `self` so the
    /// normal load pipeline renders it; each background tab is parked via the
    /// hibernation machinery (`hibernated_tabs` + `tab_snapshots`) so switching
    /// to it reconstructs it from its DOM blob without a network round-trip.
    pub(crate) fn restore_session(&mut self) {
        let tabs = match self.session_store.load() {
            Ok(t) if !t.is_empty() => t,
            Ok(_) => return,
            Err(e) => {
                eprintln!("session: не удалось прочитать сессию: {e}");
                return;
            }
        };
        let active_idx = session_persist::active_index(&tabs);

        // Rebuild the tab strip from scratch — one entry per restored tab, in
        // saved order. The strip starts with a single blank tab (id 0); reuse it.
        self.tab_strip.tabs.clear();
        self.tab_strip.next_id = 0;

        for (idx, tab) in tabs.into_iter().enumerate() {
            let id = self.tab_strip.next_id;
            self.tab_strip.next_id += 1;
            self.tab_strip.tabs.push(tabs::strip::TabEntry {
                id,
                title: if tab.title.is_empty() {
                    "Восстановленная вкладка".to_owned()
                } else {
                    tab.title.clone()
                },
                tab_state: TabState::Active,
                opener_id: None,
                container: tabs::containers::ContainerKind::None,
                last_activated_ms: 0.0,
                pinned: false,
                group_id: None,
                adblock: false,
            });
            self.lifecycle_mgr.open_tab(id as u64);

            if idx == active_idx {
                // Active tab: load fresh through the normal pipeline.
                self.source = PageSource::from_arg(Some(&tab.url));
                self.scroll_x = tab.scroll_x;
                self.scroll_y = tab.scroll_y;
                self.title = Some(tab.title);
            } else {
                // Background tab: park as hibernated so switch_tab restores it
                // from the DOM blob on demand.
                let data = lumen_storage::HibernatedTabData {
                    dom_blob: tab.dom_blob,
                    css_source: String::new(),
                    url: tab.url.clone(),
                    title: tab.title.clone(),
                    scroll_x: tab.scroll_x,
                    scroll_y: tab.scroll_y,
                };
                if self.tab_snapshots.store(id as i64, &data).is_ok() {
                    self.hibernated_tabs.insert(
                        id,
                        tab_lifecycle::TabMetadata { url: tab.url, title: tab.title },
                    );
                    let last = self.tab_strip.tabs.len() - 1;
                    self.tab_strip.set_tab_state(last, TabState::Hibernated);
                }
            }
        }

        self.tab_strip.active = active_idx.min(self.tab_strip.tabs.len().saturating_sub(1));
    }

    // ── Tab lifecycle: hibernation and restore ─────────────────────────────────
}
