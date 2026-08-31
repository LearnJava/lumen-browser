//! Browser chrome (CC track): layout of the chrome host document, the model
//! snapshot handed to `lumen-chrome`, hit-testing, chrome action dispatch and
//! the synthetic chrome accessibility nodes.
//!
//! SPLIT-SH1 (2026-08-26): moved verbatim out of `main.rs`. Behaviour, order of
//! operations and method bodies are unchanged; only module path and visibility
//! (`fn` -> `pub(crate) fn`, required for a caller in the parent module) differ.

use crate::*;

impl Lumen {
    /// CC-4 (docs/tasks/p1-css-chrome.md): re-lays-out and re-paints the
    /// engine-drawn chrome document at the current window size. No-op when the
    /// renderer/window is not ready yet (mirrors the degenerate-size guard in
    /// [`Self::relayout_viewport`]).
    ///
    /// Called once the renderer has a first non-zero size and again on every
    /// `WindowEvent::Resized`. The chrome asset has no dynamic content yet
    /// (`ChromeModel` DOM mutation lands in CC-6), so a resize is the only
    /// thing that can currently change its layout.
    ///
    /// CC-5: hover/active state comes from [`Self::chrome_hovered_nid`]/
    /// [`Self::chrome_active_nid`]. CC-7 adds `:focus`/`:focus-within` for
    /// `#omniInput` вЂ” `Some` exactly while the legacy `address_bar` is open;
    /// its caret is still hand-painted (no native `<input>` caret exists,
    /// see [`Self::chrome_omni_input_rect`]), only editing state moved to
    /// chrome-DOM. The interactive thread-locals are process-wide (brief
    /// risk #6), so this pass explicitly sets them from chrome's own state
    /// rather than inheriting whatever the page's last [`Self::relayout`]
    /// left behind, and clears them again afterward so a subsequent page
    /// relayout does not inherit chrome's state either.
    ///
    /// The design reference's `#contentArea` вЂ” the container it reserves for
    /// live tab content, doubling as the brief's "`#page-host`" (no new id
    /// was introduced) вЂ” carries its own placeholder markup (new-tab tiles, a
    /// demo site page, вЂ¦) meant for standalone preview
    /// (`about:chrome-preview`, CC-1), not for stacking under the real tab
    /// content this host paints separately at that same rect
    /// ([`Self::chrome_page_host_rect`]). Since this pass's display list
    /// paints *above* the page in `overlay_buf`, leaving that placeholder in
    /// вЂ” or even just clearing its children but keeping its own box вЂ” would
    /// permanently hide the real page behind either the placeholder markup or
    /// `#contentArea`'s own `background:var(--surface-0)` fill. This pass
    /// therefore removes `#contentArea` from the tree entirely
    /// ([`take_content_area`]) right after layout, capturing its rect into
    /// `chrome_page_host_rect` first, and before [`paint_ordered`] вЂ” except
    /// `#findBar`/`#downloadsPanel` (CC-9), salvaged back into the tree at
    /// `#contentArea`'s former slot since they're real popovers, not preview
    /// placeholder content.
    #[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn relayout_chrome_host(&mut self) {
        if self.chrome_doc.is_none() {
            return;
        }
        let Some(r) = self.renderer.as_ref() else { return };
        let viewport = r.viewport_size();
        if viewport.width <= 0.0 || viewport.height <= 0.0 {
            return;
        }
        // CC-6: rebind ChromeModel from current shell state before every
        // layout pass вЂ” no separate dirty flag, `bind_model` is cheap (a
        // handful of attribute/text mutations + two small list rebuilds).
        let model = self.chrome_model_snapshot();
        let Some((doc, sheet)) = self.chrome_doc.as_mut() else { return };
        // BUG-341 S6: `bind_model_tracked` (not plain `bind_model`) вЂ” reports
        // every node whose selector-relevant attribute/class actually changed
        // value, or whose row-list container gained/lost a member, so a
        // content-mutating pass (typed omnibox text, a tab title, вЂ¦) can also
        // take the incremental path below instead of only a pure
        // interactive-state transition (S5's limit вЂ” see BUG-341 "S5" В§"Not
        // attempted").
        let touched = lumen_chrome::bind_model_tracked(doc, &model);
        let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
        let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
        // CC-7: `#omniInput` is focused (`:focus`/`:focus-within`, e.g. the
        // `.omnibox` accent ring) exactly while the legacy `address_bar` is
        // open вЂ” there is no other focusable element in the chrome document
        // yet, so a single node id is enough.
        let omni_input = doc.find_by_id(lumen_chrome::ids::OMNI_INPUT);
        let chrome_focus = if self.address_bar.is_open() { omni_input } else { None };
        lumen_layout::set_interactive_state(self.chrome_hovered_nid, chrome_focus, self.chrome_active_nid);
        // BUG-341 S5/S6: `layout_mutation_incremental` (plain graft_geometry,
        // no incremental cascade) was tried here early on and measured
        // *worse* than a plain full layout (`graft_geometry`'s then-O(depth)
        // redundant clone bug) вЂ” fixed since (BUG-341 "third session"), and
        // now combined with the S3 incremental cascade via
        // `layout_mutation_incremental_restyle`. Safe whenever a previous
        // pristine tree/cascade cache exists to graft/diff against and
        // viewport/Forced-Colors still match вЂ” a resize or Forced-Colors flip
        // invalidates geometry `bind_model_tracked` cannot see, so those still
        // force the full, known-correct `layout_measured_hyp_with_counters`
        // fallback (as does the very first pass).
        let new_interactive = (self.chrome_hovered_nid, chrome_focus, self.chrome_active_nid);
        let forced_colors = lumen_layout::forced_colors_active();
        let viewport_stable = self.chrome_prev_viewport == Some(viewport);
        let forced_colors_stable = self.chrome_prev_forced_colors == forced_colors;
        // BUG-405 срез 48 (диагностика, п.85): все четыре входа read-нутся
        // ДО того, как строки ниже перезапишут `chrome_prev_*` этим
        // проходом — `predict_same` называет, что было бы известно БЕЗ
        // хэширования `dl` (`touched` уже посчитан выше, до этой точки).
        let interactive_stable = new_interactive == self.chrome_prev_interactive;
        let predict_same =
            touched.is_empty() && interactive_stable && viewport_stable && forced_colors_stable;
        // BUG-341 S22: the previous pass's pristine tree is *reconstructed*
        // from the live one, not copied out of it while it was still pristine.
        // `chrome_layout` holds the pruned tree that pass painted, and
        // `chrome_content_area_detached` holds exactly what the pruning took
        // out вЂ” putting the second back into the first yields the pre-pruning
        // tree box for box, for the price of one insert per salvaged popover
        // instead of a whole-tree copy. Taking `chrome_layout` by value is
        // what makes it free: the incremental path below *moves* the reusable
        // subtrees out of the basis (S19), so the basis does not survive the
        // call either way. Both fields are reassigned from this pass's own
        // result further down, on both arms; the display list is dropped with
        // the tree because this pass builds a new one.
        let prev_pristine = match (self.chrome_layout.take(), self.chrome_content_area_detached.take()) {
            (Some((mut lb, _stale_dl)), Some(detached)) => restore_content_area(&mut lb, detached).then_some(lb),
            // No `#contentArea` in the chrome document (or it had no box):
            // nothing was pruned, so the live tree *is* the pristine one.
            (Some((lb, _stale_dl)), None) => Some(lb),
            (None, _) => None,
        };
        let (mut layout, cascade_styles) = match (
            viewport_stable && forced_colors_stable,
            prev_pristine,
        ) {
            (true, Some(prev)) => {
                let (prev_hover, prev_focus, prev_active) = self.chrome_prev_interactive;
                // BUG-341 S7: computed once per pass, not once per axis вЂ” the
                // stylesheet/shadow-DOM shape doesn't change between the three
                // hover/focus/active calls below.
                let state_index = lumen_layout::style::restyle_state_index(doc, sheet);
                let mut dirty_roots = std::collections::HashSet::new();
                dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                    doc,
                    prev_hover,
                    new_interactive.0,
                    &state_index,
                ));
                dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                    doc,
                    prev_focus,
                    new_interactive.1,
                    &state_index,
                ));
                dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                    doc,
                    prev_active,
                    new_interactive.2,
                    &state_index,
                ));
                // BUG-341 S6: DOM-mutation root-set, unioned with the
                // interactive-state one above вЂ” `touched` is empty on a pure
                // hover/focus/active-only pass (S5's original case), non-empty
                // whenever `bind_model` actually changed content this cycle.
                // BUG-341 S17: the report names the mutated attributes, so the
                // root-set can narrow each one to the node itself unless some
                // selector reaches a sibling from a compound matching it.
                let node_index = lumen_layout::style::restyle_node_index(doc, sheet);
                dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(
                    doc,
                    chrome_node_changes(&touched),
                    &node_index,
                ));
                let delta = lumen_layout::counters::RestyleDelta {
                    prev_styles: std::mem::take(&mut self.chrome_prev_cascade_styles),
                    dirty_roots,
                    // BUG-341 S16: `bind_model_tracked` enumerates every node
                    // whose content it mutated (see `ChromeMutations::content`
                    // and the source-level completeness gate behind it), so
                    // this cycle can name them instead of declaring the whole
                    // document unstable the way S4-S15 had to вЂ” one changed
                    // omnibox character no longer costs all 318 boxes.
                    content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
                };
                lumen_layout::counters::set_incremental_restyle(true);
                // BUG-341 S15: reuse whole box subtrees from `prev` too, not
                // just their geometry. Licensed by the `content_dirty` record
                // this call site establishes above вЂ” a subtree containing a
                // mutated node stays out of `clean_subtrees`.
                lumen_layout::box_tree::set_incremental_box_build(true);
                let result = lumen_layout::box_tree::layout_mutation_incremental_restyle(
                    doc,
                    sheet,
                    viewport,
                    &measurer,
                    &*self.hyp_provider,
                    self.dark_mode,
                    prev,
                    delta,
                );
                lumen_layout::box_tree::set_incremental_box_build(false);
                lumen_layout::counters::set_incremental_restyle(false);
                result
            }
            _ => lumen_layout::layout_measured_hyp_with_counters(
                doc,
                sheet,
                viewport,
                &measurer,
                &*self.hyp_provider,
                self.dark_mode,
            ),
        };
        lumen_layout::clear_interactive_state();
        // Persist this pass's cascade cache as the `prev` basis for the next
        // call's incremental path (BUG-341 S5). Its box-tree counterpart is no
        // longer copied here вЂ” S22 records `take_content_area`'s removals
        // below instead and undoes them at the top of the next pass.
        self.chrome_prev_cascade_styles = cascade_styles.into_styles();
        self.chrome_prev_viewport = Some(viewport);
        self.chrome_prev_interactive = new_interactive;
        self.chrome_prev_forced_colors = forced_colors;
        // CC-9: `#findBar`/`#downloadsPanel` are salvaged out of
        // `#contentArea` before the rest of it is discarded вЂ” see
        // `take_content_area`'s doc comment. CC-10 adds the command palette
        // and cert/print modals вЂ” all three are `position:absolute` direct
        // children of `#contentArea` too, same reasoning as `#findBar`/
        // `#downloadsPanel`.
        // BUG-341 S22: the detachment record is kept, not discarded вЂ” it is
        // what lets the next pass rebuild this tree's pristine form instead of
        // this pass copying one aside.
        let pruned = doc.find_by_id(lumen_chrome::ids::CONTENT_AREA).and_then(|id| {
            take_content_area(
                &mut layout,
                id,
                &[
                    lumen_chrome::ids::FIND_BAR,
                    lumen_chrome::ids::DOWNLOADS_PANEL,
                    lumen_chrome::ids::CP_OVERLAY,
                    lumen_chrome::ids::CERT_OVERLAY,
                    lumen_chrome::ids::PRINT_OVERLAY,
                ],
                doc,
            )
        });
        let (page_host_rect, detached) = match pruned {
            Some((rect, detached)) => (Some(rect), Some(detached)),
            None => (None, None),
        };
        self.chrome_page_host_rect = page_host_rect;
        self.chrome_content_area_detached = detached;
        // CC-7/CC-9: captured non-destructively (unlike `#contentArea`
        // above) вЂ” these nodes stay in the tree and paint normally.
        self.chrome_omni_input_rect = omni_input
            .and_then(|id| lumen_layout::find_box_by_node(&layout, id))
            .map(|b| b.rect);
        // CC-11: sync transitions вЂ” compare chrome_prev_styles with the fresh
        // layout before replacing it, mirroring apply_relayout_result's
        // page-side sync (main.rs's collect_box_styles + sync loop). No
        // @starting-style handling here: bind_model mutates existing
        // chrome_doc nodes in place rather than inserting/removing them, so
        // there are no "entering" nodes the way JS page mutation can produce.
        let now_s = self.epoch.elapsed().as_secs_f32();
        let mut new_styles = HashMap::new();
        collect_box_styles(&layout, &mut new_styles);
        for (node, new_style) in &new_styles {
            if let Some(old_style) = self.chrome_prev_styles.get(node) {
                self.chrome_transition_scheduler.sync(*node, old_style, new_style, now_s);
            }
        }
        self.chrome_prev_styles = new_styles;
        let dl = paint_ordered(&layout);
        // BUG-405 срез 48 (диагностика, п.85): не гейтит поведение — только
        // печать под `LUMEN_FRAME_LOG=2`. `hash_display_list` берёт `dl` как
        // overlay-лейн (content — пустой срез), тот же тотальный хэш, что уже
        // используют кадровый хэш и overlay-кэш (band_compose.rs), так что
        // «actual=true» здесь означает ровно то же самое «байты не
        // изменились», что видит overlay_cache_step дальше по кадру.
        if lumen_paint::frame_log_level() >= 2 {
            let actual_hash = lumen_paint::hash_display_list(&[], &dl, 0.0, 0.0, 0, 0);
            let actual_same = self.chrome_dl_content_hash == Some(actual_hash);
            eprintln!(
                "[frame] chrome-dl-repeat predict={predict_same} actual={actual_same}"
            );
            self.chrome_dl_content_hash = Some(actual_hash);
        }
        self.chrome_layout = Some((layout, dl));
    }

    /// CC-6 (docs/tasks/p1-css-chrome.md): snapshots tab strip, workspaces,
    /// theme, tab layout, and active profile into a [`lumen_chrome::ChromeModel`]
    /// вЂ” [`Self::relayout_chrome_host`] binds this before every chrome layout
    /// pass. Mirrors the same shell fields [`Self::chrome_snapshot`] (DS-17 a11y
    /// tree) reads, shaped for [`lumen_chrome::bind_model`] instead. CC-7 adds
    /// the omnibox value/spoof-warning, read from the legacy `address_bar`.
    pub(crate) fn chrome_model_snapshot(&self) -> lumen_chrome::ChromeModel {
        let active_id = self.tab_strip.tabs.get(self.tab_strip.active).map(|t| t.id);
        // BUG-409: iterate `visible_indices()` rather than `tabs` directly вЂ”
        // a collapsed group's non-leftmost members stay hidden behind the
        // leftmost (chip) row, mirroring the legacy strip's own collapse
        // behaviour. For a strip with no collapsed groups this is `0..len()`.
        let tabs = self
            .tab_strip
            .visible_indices()
            .into_iter()
            .map(|i| {
                let t = &self.tab_strip.tabs[i];
                lumen_chrome::ChromeTabModel {
                    id: t.id,
                    title: t.title.clone(),
                    active: Some(t.id) == active_id,
                    sleeping: t.tab_state == TabState::Hibernated,
                    // CC-8: tree-style tabs (7A.2) вЂ” a tab with an opener is
                    // rendered as a `.child` row with a `.tree-line` connector.
                    // The asset's CSS only indents one nesting level, so this
                    // collapses depth в‰Ґ1 to a single boolean rather than
                    // threading `tabs::tree::depth_of`'s full depth through.
                    is_child: t.opener_id.is_some(),
                    container_color: t.container.border_color().map(Self::chrome_hex_color),
                    group: t.group_id.and_then(|gid| {
                        let group = self.tab_strip.group(gid)?;
                        let color = self.tab_strip.group_color(gid)?.color();
                        Some(lumen_chrome::ChromeTabGroup {
                            color: Self::chrome_hex_color(color),
                            name: group.label.clone(),
                            collapsed: group.collapsed,
                        })
                    }),
                }
            })
            .collect();
        let workspaces = self
            .workspace_panel
            .workspaces
            .iter()
            .map(|w| lumen_chrome::ChromeWorkspaceModel {
                id: w.id,
                name: w.name.clone(),
                active: Some(w.id) == self.workspace_panel.active_id,
                color: Self::chrome_hex_color(w.accent),
            })
            .collect();
        let profile_slug = self
            .profile_menu
            .active_entry()
            .and_then(|e| panels::profile_menu::slug_for_profile(&e.name))
            .map(str::to_owned);
        // CC-7: same not-focused/focused branching `build_inline_field` uses
        // for the legacy overlay text, retargeted at `#omniInput`'s `value`.
        let (omnibox_value, omnibox_warning) =
            address_bar::chrome_omnibox_value(&self.address_bar, self.current_display_url());
        // CC-9: same `MAX_VISIBLE` cap the legacy `address_bar::build_dropdown`
        // applies вЂ” the asset's `.dropdown` isn't scroll-clipped, so an
        // uncapped list would grow the popover past its designed height.
        let dropdown_suggestions: Vec<lumen_chrome::ChromeSuggestionModel> = self
            .address_bar
            .suggestions()
            .iter()
            .take(address_bar::MAX_VISIBLE)
            .enumerate()
            .map(|(idx, s)| {
                // CC-15-3/DS-6: punycode-guard both strings, as the legacy
                // `build_dropdown` did вЂ” without this a homograph host in a
                // history/bookmark hit renders in its Unicode form.
                let (label, sub_label) = address_bar::chrome_suggestion_text(s);
                lumen_chrome::ChromeSuggestionModel {
                    idx,
                    label,
                    sub_label,
                    color: Self::chrome_hex_color(s.tag_color()),
                    tag: s.tag(),
                }
            })
            .collect();
        let dropdown = lumen_chrome::ChromeDropdownModel {
            open: self.address_bar.is_open() && !dropdown_suggestions.is_empty(),
            suggestions: dropdown_suggestions,
        };
        // CC-9: `current_matches()` re-scans the display list for the query вЂ”
        // cheap relative to a full relayout, and this snapshot only runs on
        // explicit chrome-relayout triggers (resize/click/key), not every
        // `RedrawRequested` frame (see `Self::relayout_chrome_host`'s doc).
        let find_matches_len = if self.find.is_open() { self.current_matches().len() } else { 0 };
        // CC-15-6/BUG-419: the "ERR" state is carried over from the deleted
        // legacy bar (`find::append_bar`) вЂ” without it an invalid regex is
        // indistinguishable from "no matches" (`0/0`). The legacy bar also
        // painted it red (`BAR_ERR`); `error` drives `#findCount`'s `.error`
        // class to restore that accent (see BUG-419).
        let find_is_error = self.find.is_regex_mode()
            && !self.find.query().is_empty()
            && !find::is_valid_regex_pattern(self.find.query());
        let find = lumen_chrome::ChromeFindModel {
            open: self.find.is_open(),
            value: self.find.query().to_owned(),
            count_label: if find_is_error {
                "ERR".to_owned()
            } else if find_matches_len == 0 {
                "0/0".to_owned()
            } else {
                format!("{}/{}", self.find.active_index() + 1, find_matches_len)
            },
            error: find_is_error,
        };
        let downloads: Vec<lumen_chrome::ChromeDownloadModel> = self
            .downloads
            .entries()
            .iter()
            .map(|d| {
                let (meta, progress_fraction) = match &d.status {
                    download::DownloadStatus::Pending => ("Р’ РѕС‡РµСЂРµРґРёвЂ¦".to_owned(), None),
                    download::DownloadStatus::InProgress => {
                        let text = match d.total {
                            Some(t) if t > 0 => format!(
                                "{} / {} вЂ” РёРґС‘С‚ Р·Р°РіСЂСѓР·РєР°вЂ¦",
                                download::human_bytes(d.received),
                                download::human_bytes(t)
                            ),
                            _ => "Р—Р°РіСЂСѓР·РєР°вЂ¦".to_owned(),
                        };
                        (text, Some(d.progress_fraction().unwrap_or(0.6)))
                    }
                    download::DownloadStatus::Done { bytes } => {
                        (format!("{} вЂ” РіРѕС‚РѕРІРѕ", download::human_bytes(*bytes)), None)
                    }
                    download::DownloadStatus::Failed(reason) => (format!("РћС€РёР±РєР°: {reason}"), None),
                    download::DownloadStatus::Cancelled => ("РћС‚РјРµРЅРµРЅРѕ".to_owned(), None),
                };
                lumen_chrome::ChromeDownloadModel {
                    id: d.id.raw(),
                    ext_label: download::extension_label(&d.filename),
                    name: d.filename.clone(),
                    meta,
                    progress_fraction,
                }
            })
            .collect();
        // BUG-408: mirrors `TabArchive`'s entries into `#archivePanel`'s
        // `.arc-list` вЂ” same shape `ChromeTabModel`'s favicon fallback and
        // `container_color` convention already use.
        let archive: Vec<lumen_chrome::ChromeArchiveEntryModel> = self
            .archive
            .entries
            .iter()
            .map(|e| lumen_chrome::ChromeArchiveEntryModel {
                id: e.id,
                fav_letter: e
                    .title
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_else(|| "\u{2022}".to_owned()),
                title: e.title.clone(),
                url: e.url.clone(),
                container_color: e.container.border_color().map(Self::chrome_hex_color),
            })
            .collect();
        // CC-9: the frozen design merges shields' blocked-count and the
        // permission rows into one `#permPopover` вЂ” no separate engine
        // control exists for `PermissionPanel::visible` (`Ctrl+Shift+P`), so
        // either legacy toggle shows it.
        let popover_open = self.shields.visible || self.permission.visible;
        // BUG-411: all four `PermissionKind::ALL` rows вЂ” the asset gained the
        // Notifications/Clipboard rows the frozen design was missing, so their
        // state is no longer unreachable from the UI.
        let permissions = panels::permission_panel::PermissionKind::ALL
        .map(|kind| match self.permission.state_for(kind) {
            panels::permission_panel::PermissionState::Allow => lumen_chrome::ChromePermState::Allow,
            panels::permission_panel::PermissionState::Deny => lumen_chrome::ChromePermState::Deny,
            panels::permission_panel::PermissionState::Ask => lumen_chrome::ChromePermState::Ask,
        });
        // CC-10: same `MAX_VISIBLE_ROWS`-windowed slice
        // `command_palette::build_panel` shows, mapped down to what `.cp-row`
        // can render вЂ” a command's keyboard shortcut, or a bookmark/history
        // item's URL, as the sub-label.
        let filtered = self.command_palette.filtered();
        let palette_results: Vec<lumen_chrome::ChromePaletteResultModel> = filtered
            .iter()
            .enumerate()
            .skip(self.command_palette.scroll_row)
            .take(panels::command_palette::MAX_VISIBLE_ROWS)
            .filter_map(|(rank, &item_idx)| {
                self.command_palette.items.get(item_idx).map(|item| {
                    let sub_label = match &item.kind {
                        panels::command_palette::PaletteKind::Command(action) => {
                            action.shortcut().to_owned()
                        }
                        panels::command_palette::PaletteKind::Bookmark
                        | panels::command_palette::PaletteKind::History => item.url.clone(),
                    };
                    lumen_chrome::ChromePaletteResultModel {
                        label: item.title.clone(),
                        sub_label,
                        selected: rank == self.command_palette.selected,
                    }
                })
            })
            .collect();
        let palette = lumen_chrome::ChromePaletteModel {
            open: self.command_palette.visible,
            query: self.command_palette.query.clone(),
            results: palette_results,
        };
        // CC-10: the design's 6 `.cert-row`s + `.cert-fp` cover a subset of
        // `PanelCertData`'s 9 fields (no TLS-version slot) вЂ” missing/empty
        // individual fields render as `"вЂ”"`, mirroring
        // `cert_panel::build_rows`'s own em-dash fallback.
        let dash = |s: &str| if s.is_empty() { "\u{2014}".to_owned() } else { s.to_owned() };
        let cert = match &self.cert_panel.cert {
            Some(c) if c.has_data() => {
                let san = if c.san_list.is_empty() { "\u{2014}".to_owned() } else { c.san_list.join(", ") };
                let issuer = if !c.issuer_org.is_empty() { c.issuer_org.clone() } else { dash(&c.issuer_cn) };
                lumen_chrome::ChromeCertModel {
                    open: self.cert_panel.visible,
                    title: format!("РЎРµСЂС‚РёС„РёРєР°С‚ вЂ” {}", dash(&c.subject_cn)),
                    rows: [
                        dash(&c.subject_cn),
                        dash(&c.subject_org),
                        san,
                        issuer,
                        dash(&c.not_before),
                        dash(&c.not_after),
                    ],
                    fingerprint: dash(&c.fingerprint_sha256),
                }
            }
            _ => lumen_chrome::ChromeCertModel {
                open: self.cert_panel.visible,
                title: "РЎРµСЂС‚РёС„РёРєР°С‚ РЅРµРґРѕСЃС‚СѓРїРµРЅ".to_owned(),
                rows: std::array::from_fn(|_| "\u{2014}".to_owned()),
                fingerprint: "\u{2014}".to_owned(),
            },
        };
        // CC-10b: which `#contentArea` view is shown вЂ” mirrors whichever of
        // the three legacy panel `visible` flags is set (kept mutually
        // exclusive by `dispatch_chrome_action`'s `ShowView` handler), same
        // "reuse the legacy flag as source of truth" approach CC-9/CC-10
        // already use for print/cert/palette `open` state.
        let content_view = if self.settings_panel.visible {
            lumen_chrome::ChromeContentView::Settings
        } else if self.history_panel.visible {
            lumen_chrome::ChromeContentView::History
        } else if self.bookmark_panel.visible {
            lumen_chrome::ChromeContentView::Bookmarks
        } else {
            lumen_chrome::ChromeContentView::Page
        };
        // CC-10b: DS-16's "history not saved" banner вЂ” same anonymous-profile
        // check the legacy `history_panel::build_panel` call site makes.
        let is_anon = self
            .profile_menu
            .active_entry()
            .is_some_and(|e| panels::profile_menu::is_anonymous(&e.name));
        let history = lumen_chrome::ChromeHistoryModel {
            banner: is_anon,
            rows: self
                .history_panel
                .rows
                .iter()
                .map(|r| match r {
                    panels::history_panel::HistoryRow::Group(label) => {
                        lumen_chrome::ChromeHistoryRow::Group(label.clone())
                    }
                    panels::history_panel::HistoryRow::Entry(item) => lumen_chrome::ChromeHistoryRow::Entry {
                        title: if item.title.is_empty() { item.url.clone() } else { item.title.clone() },
                        url: item.url.clone(),
                        time_label: panels::history_panel::format_time_hhmm(item.visit_date),
                    },
                })
                .collect(),
        };
        // CC-10b: `"Р’СЃРµ Р·Р°РєР»Р°РґРєРё"` (the `None`-filter entry) followed by the
        // real folder set вЂ” mirrors `bookmark_panel::hit_test`'s own "All"
        // row convention.
        let mut bookmark_folders = vec![lumen_chrome::ChromeBookmarkFolderModel {
            label: "Р’СЃРµ Р·Р°РєР»Р°РґРєРё".to_owned(),
            active: self.bookmark_panel.selected_folder.is_none(),
            filter: None,
        }];
        bookmark_folders.extend(self.bookmark_panel.folders.iter().map(|f| {
            lumen_chrome::ChromeBookmarkFolderModel {
                label: f.clone(),
                active: self.bookmark_panel.selected_folder.as_deref() == Some(f.as_str()),
                filter: Some(f.clone()),
            }
        }));
        let bookmarks = lumen_chrome::ChromeBookmarksModel {
            folders: bookmark_folders,
            title: self.bookmark_panel.selected_folder.clone().unwrap_or_else(|| "Р’СЃРµ Р·Р°РєР»Р°РґРєРё".to_owned()),
            cards: self
                .bookmark_panel
                .visible_entries()
                .iter()
                .map(|e| {
                    let title = if e.title.is_empty() { e.url.clone() } else { e.title.clone() };
                    lumen_chrome::ChromeBookmarkCardModel {
                        fav_letter: title
                            .chars()
                            .next()
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_else(|| "\u{2022}".to_owned()),
                        title,
                        url: e.url.clone(),
                    }
                })
                .collect(),
        };
        let settings = lumen_chrome::ChromeSettingsModel {
            active_section: self.chrome_settings_section.clone(),
            ad_block_on: self.settings_panel.draft.shields_enabled,
            fingerprint_on: self.settings_panel.draft.fingerprint_mode != "off",
        };
        // CC-10b: the design's single tabbed `#rightSidebar` merges the
        // legacy independently-dockable `ai_panel`/`sidebar` вЂ” kept mutually
        // exclusive by `dispatch_chrome_action`'s `OpenAiSidebar`/
        // `OpenWebSidebar`/`SetSidebarTab` handlers, so `sidebar.visible`
        // alone picks the tab.
        let right_sidebar = lumen_chrome::ChromeRightSidebarModel {
            open: self.ai_panel.visible || self.sidebar.visible,
            tab: if self.sidebar.visible {
                lumen_chrome::ChromeSidebarTab::Web
            } else {
                lumen_chrome::ChromeSidebarTab::Ai
            },
        };
        lumen_chrome::ChromeModel {
            dark_theme: self.dark_mode,
            layout_vertical: self.vertical_tabs.visible,
            profile_slug,
            tabs,
            workspaces,
            omnibox: lumen_chrome::OmniboxModel {
                value: omnibox_value,
                warning: omnibox_warning.map(str::to_owned),
            },
            sidebar_collapsed: self.chrome_sidebar_collapsed,
            dropdown,
            find,
            downloads_open: self.downloads.visible,
            downloads,
            archive_open: self.archive.visible,
            archive,
            popover_open,
            blocked_total: self.shields.blocked_total_count(),
            permissions,
            // BUG-411: the popover names the host it applies to and carries the
            // per-site shields switch вЂ” both lost when CC-15-4 removed the
            // legacy panels that used to show them.
            popover_domain: self.shields.current_domain.clone().unwrap_or_default(),
            site_shields_on: self.shields.enabled_for_current(),
            palette,
            cert,
            print: lumen_chrome::ChromePrintModel {
                open: self.print_panel.visible,
                landscape: self.print_panel.orientation == panels::print_panel::Orientation::Landscape,
                backgrounds: self.print_panel.print_backgrounds,
            },
            content_view,
            history,
            bookmarks,
            settings,
            right_sidebar,
        }
    }

    /// CC-8: renders a [`lumen_layout::Color`] as the `#RRGGBB` string
    /// `ChromeModel`'s container/workspace colour fields need (CSS custom
    /// properties and inline `style="background:вЂ¦"` are both plain text).
    /// Drops alpha вЂ” every caller (container accent, workspace accent) is
    /// opaque in practice.
    pub(crate) fn chrome_hex_color(c: lumen_layout::Color) -> String {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    }

    /// CC-5 (docs/tasks/p1-css-chrome.md): where the page content starts, in
    /// window CSS pixels вЂ” the single source of truth input-coordinate
    /// conversion ([`Self::page_point`], [`Self::update_cursor_icon`]) and
    /// the render-time page transform share, so a click/hover always lands
    /// on the same page element the frame actually painted there.
    ///
    /// This is [`Self::chrome_page_host_rect`]'s origin, falling back to
    /// `(0, CHROME_H)` before the first chrome layout exists, mirroring
    /// [`Self::relayout_chrome_host`]'s own degenerate-size guard вЂ” that frame
    /// paints the page flush with the window too.
    pub(crate) fn page_offset(&self) -> (f32, f32) {
        self.chrome_page_host_rect
            .map(|r| (r.x, r.y))
            .unwrap_or((0.0, toolbar::CHROME_H))
    }

    /// CC-5: `true` when `(x_css, y_css)` falls outside the page-content
    /// rect вЂ” i.e. over an opaque chrome furniture area (sidebar, toolbar,
    /// tab strip). A `None` [`Self::chrome_page_host_rect`] (no chrome layout
    /// yet) counts as "over chrome": mirrors [`Self::relayout_chrome_host`]'s
    /// guard вЂ” nothing is painted at the page rect either in that frame, so
    /// there is no page underneath to click through to yet.
    pub(crate) fn point_over_chrome(&self, x_css: f32, y_css: f32) -> bool {
        match self.chrome_page_host_rect {
            Some(r) => {
                x_css < r.x || x_css >= r.right() || y_css < r.y || y_css >= r.bottom()
            }
            None => true,
        }
    }

    /// CC-5: hit-tests [`Self::chrome_layout`] at window-CSS coordinates вЂ”
    /// the chrome document paints at the window origin, no scroll/page
    /// transform involved. `None` before the first chrome layout exists.
    pub(crate) fn chrome_hit_test(&self, x_css: f32, y_css: f32) -> Option<lumen_paint::HitTestResult> {
        let (layout, _) = self.chrome_layout.as_ref()?;
        hit_test(Point::new(x_css, y_css), layout)
    }

    /// CC-5: walks `hit.path` (bubble order, closest node first вЂ” the same
    /// list `HitTestResult` already builds for event-dispatch bubbling) for
    /// the nearest ancestor carrying a recognised `data-action`, mirroring
    /// how the legacy toolbar/tab-strip hit-testers resolve a click to one
    /// semantic action regardless of which child element (icon, label,
    /// badge) the point actually landed on. Returns the carrying node too вЂ”
    /// `dispatch_chrome_action` needs it to read action-specific sibling
    /// attributes (`data-view`, вЂ¦).
    pub(crate) fn chrome_action_at(
        &self,
        hit: &lumen_paint::HitTestResult,
    ) -> Option<(NodeId, lumen_chrome::ChromeAction)> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        hit.path.iter().find_map(|&nid| {
            doc.get(nid)
                .get_attr("data-action")
                .and_then(lumen_chrome::ChromeAction::from_attr_value)
                .map(|action| (nid, action))
        })
    }

    /// CC-6: reads and parses a `data-tab-id`/`data-ws-id`-style integer
    /// attribute off `nid` in the live `chrome_doc` вЂ” the id `ChromeModel`
    /// (`crates/chrome/src/model.rs`) stamps on rebuilt tab rows/workspace
    /// buttons so a click can be resolved back to a `tab_strip`/
    /// `workspace_panel` entry. `None` off the flag, before the first chrome
    /// layout, or if the attribute is missing/unparsable.
    pub(crate) fn chrome_data_id(&self, nid: NodeId, attr: &str) -> Option<i64> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        doc.get(nid).get_attr(attr)?.parse().ok()
    }

    /// BUG-422: the string sibling of [`Self::chrome_data_id`] вЂ” reads a
    /// `data-*` attribute off `nid` as an owned `String`.
    ///
    /// The `#view-history`/`#view-bookmarks` row actions key off the entry's
    /// URL rather than an integer id: `History::delete`/`Bookmarks::delete`/
    /// `Bookmarks::add` all take a URL, and `Lumen::refresh_history`'s FTS
    /// branch fabricates `HistoryItem::id` from the result position, so the
    /// id is not a stable handle under an active search query. Owned because
    /// every caller mutates `self` right after reading.
    pub(crate) fn chrome_data_attr(&self, nid: NodeId, attr: &str) -> Option<String> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        doc.get(nid).get_attr(attr).map(str::to_owned)
    }

    /// CC-5/CC-6: routes a chrome `data-action` click to the shell's existing
    /// handlers вЂ” the same functions the legacy toolbar/tab-strip hit-
    /// testers call, so behavior (reload semantics, panel toggling, вЂ¦)
    /// matches exactly. `SelectTab`/`CloseTab`/`SelectWorkspace`/`AddWorkspace`
    /// resolve the clicked row back to a real `tab_strip`/`workspace_panel`
    /// entry via the `data-tab-id`/`data-ws-id` attribute `ChromeModel`
    /// stamped on it (CC-6, `crates/chrome/src/model.rs`) вЂ” `nid` is the
    /// `data-action`-carrying node itself for these four. `SetSettingsSection`/
    /// `ShowView`/`SetSidebarTab` and friends (CC-10b) since grew the same
    /// pattern for the settings/history/bookmarks views and the right
    /// sidebar. A handful of actions remain permanent no-ops for reasons
    /// specific to each вЂ” see the comment on the final match arm below
    /// (BUG-426).
    pub(crate) fn dispatch_chrome_action(
        &mut self,
        nid: NodeId,
        action: lumen_chrome::ChromeAction,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        use lumen_chrome::ChromeAction;
        match action {
            ChromeAction::Reload => {
                // Mirrors `toolbar::ToolbarHit::Reload` вЂ” routed through the
                // UserInteraction task source rather than called directly
                // (HTML В§8.1.4).
                let flag = Rc::clone(&self.pending_reload);
                self.runtime.handle().queue_task(
                    runtime::TaskSource::UserInteraction,
                    move || { flag.set(true); },
                );
            }
            ChromeAction::NewTab => self.open_new_tab(),
            ChromeAction::OpenCertViewer => {
                let cert = self.cert_info.clone();
                self.cert_panel.toggle(cert);
                // CC-10: see the matching comment on `ToggleShieldPopover`.
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleShieldPopover => {
                self.shields.toggle();
                // CC-9: `#permPopover`'s `.open` class is baked into
                // `chrome_layout` at `relayout_chrome_host` time, same gap
                // CC-7 found for `#omniInput` вЂ” without this the popover
                // wouldn't show until some other trigger relayouts chrome.
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleFind => {
                if self.find.is_open() {
                    self.find.close();
                } else {
                    self.hint.close();
                    self.find.open();
                }
                self.relayout_chrome_host();
            }
            // CC-10b: `#rightSidebar` is a single tabbed panel in the design
            // (`.right-sidebar`/`.content-area` are flex siblings вЂ” opening
            // it really does push `#contentArea`, unlike the modal overlays
            // CC-9/CC-10 gated) вЂ” mutually exclusive with the AI tab so
            // `chrome_model_snapshot`'s `right_sidebar.tab` stays unambiguous.
            // `relayout_chrome()` keeps the legacy (flag-off) page-reflow
            // behavior; `relayout_chrome_host()` is the CC-7/9/10-class fix
            // this action was missing вЂ” `#rightSidebar`'s `.open` class is
            // baked into `chrome_layout` at relayout time, so without it the
            // panel wouldn't show until some other trigger relayouts chrome.
            ChromeAction::OpenWebSidebar => {
                self.ai_panel.visible = false;
                self.sidebar.toggle();
                self.relayout_chrome();
                self.relayout_chrome_host();
            }
            ChromeAction::OpenAiSidebar => {
                self.sidebar.visible = false;
                self.ai_panel.toggle();
                self.relayout_chrome();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleDownloads => {
                self.downloads.toggle_visible();
                self.relayout_chrome_host();
            }
            // BUG-408: shared by `#archiveToggleBtn`, `.nt-restore`, and
            // `#archivePanel`'s own close button (all three carry this same
            // action, mirroring how `toggle-downloads` closes its own panel).
            ChromeAction::ToggleArchive => {
                self.archive.toggle();
                self.relayout_chrome_host();
            }
            // BUG-408: `.arc-restore`/`.arc-dismiss` carry their own copy of
            // `data-archive-id` (mirrors `.tab-close`'s `data-tab-id`), so
            // `nid` is the button itself, not the row.
            ChromeAction::ArchiveRestore => {
                if let Some(id) = self.chrome_data_id(nid, "data-archive-id")
                    && let Some(entry) = self.archive.take(id as usize)
                {
                    if !entry.url.is_empty() {
                        self.navigate_to(PageSource::Url(entry.url));
                    }
                    self.archive.close();
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ArchiveDismiss => {
                if let Some(id) = self.chrome_data_id(nid, "data-archive-id") {
                    self.archive.take(id as usize);
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::OpenPrintDialog => {
                self.print_panel.toggle();
                // CC-10: see the matching comment on `ToggleShieldPopover`.
                self.relayout_chrome_host();
            }
            // BUG-420: `#printOrientationSelect` has exactly two `<option>`s
            // (РљРЅРёР¶РЅР°СЏ/РђР»СЊР±РѕРјРЅР°СЏ) вЂ” a click anywhere on the closed select
            // just flips between them, mirroring `#printOverlay`'s lack of a
            // real dropdown-popover mechanism in the chrome host.
            ChromeAction::CyclePrintOrientation => {
                self.print_panel.orientation = match self.print_panel.orientation {
                    panels::print_panel::Orientation::Portrait => panels::print_panel::Orientation::Landscape,
                    panels::print_panel::Orientation::Landscape => panels::print_panel::Orientation::Portrait,
                };
                self.relayout_chrome_host();
            }
            ChromeAction::TogglePrintBackgrounds => {
                self.print_panel.print_backgrounds = !self.print_panel.print_backgrounds;
                self.relayout_chrome_host();
            }
            // BUG-420: the "РџРµС‡Р°С‚СЊ" footer button вЂ” was `close-modal` (did
            // nothing but dismiss the overlay). Runs the real PDF export
            // with `PrintPanel`'s current settings, mirroring
            // `handle_print_request`'s JS `window.print()` path.
            ChromeAction::PrintConfirm => {
                self.handle_print_confirm();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleDevtools => self.devtools_console.toggle(),
            ChromeAction::ToggleProfileMenu => {
                self.profile_menu.toggle();
                if self.profile_menu.visible {
                    self.refresh_profile_menu_entries();
                }
            }
            // CC-10b: `data-view` picks the target. `#view-page`/`#view-history`/
            // `#view-bookmarks`/`#view-settings` are mutually exclusive
            // (`.view.active`, one at a time вЂ” `chrome_model_snapshot`'s
            // `content_view` derives from whichever legacy panel's `visible`
            // flag is set), so opening one closes the other two. Reuses the
            // exact legacy open/refresh calls the `Ctrl+H`/`Ctrl+Shift+O`/
            // `Ctrl+,` keyboard shortcuts already make, so behavior (data
            // load, draft flush on settings close) matches exactly.
            ChromeAction::ShowView => {
                let view = self
                    .chrome_doc
                    .as_ref()
                    .and_then(|(doc, _)| doc.get(nid).get_attr("data-view"))
                    .map(str::to_owned);
                match view.as_deref() {
                    Some("settings") => {
                        if self.settings_panel.visible {
                            self.close_settings_panel();
                        } else {
                            self.history_panel.visible = false;
                            self.bookmark_panel.visible = false;
                            self.open_settings_panel();
                        }
                    }
                    Some("history") => {
                        if !self.history_panel.visible {
                            if self.settings_panel.visible {
                                self.close_settings_panel();
                            }
                            self.bookmark_panel.visible = false;
                        }
                        self.history_panel.toggle();
                        if self.history_panel.visible {
                            self.refresh_history();
                        }
                    }
                    Some("bookmarks") => {
                        if !self.bookmark_panel.visible {
                            if self.settings_panel.visible {
                                self.close_settings_panel();
                            }
                            self.history_panel.visible = false;
                        }
                        self.bookmark_panel.toggle();
                        if self.bookmark_panel.visible {
                            self.refresh_bookmarks();
                        }
                    }
                    _ => {
                        // "page" or unrecognised: back to the active tab's page.
                        if self.settings_panel.visible {
                            self.close_settings_panel();
                        }
                        self.history_panel.visible = false;
                        self.bookmark_panel.visible = false;
                    }
                }
                self.relayout_chrome_host();
            }
            // BUG-422: `#view-history`/`#view-bookmarks` entry actions. Every
            // one of them resolves the clicked node through `data-hist-url`/
            // `data-bm-url`/`data-bm-folder` (stamped by `bind_history`/
            // `bind_bookmarks`, `crates/chrome/src/model.rs`) вЂ” the same
            // attribute-carries-the-context shape as `data-tab-id` above.
            //
            // Opening an entry also drops the view back to the page: the four
            // `#contentArea` views are mutually exclusive, so navigating while
            // `#view-history` is active would load the page behind a list that
            // stays on screen. Mirrors `ShowView`'s "page" arm.
            ChromeAction::OpenHistoryEntry => {
                if let Some(url) = self.chrome_data_attr(nid, "data-hist-url").filter(|u| !u.is_empty()) {
                    self.history_panel.visible = false;
                    self.navigate_to(PageSource::Url(url));
                    self.relayout_chrome_host();
                }
            }
            // The design's per-row star. `Bookmarks::add` upserts on the URL,
            // so a repeat click is idempotent rather than a duplicate row.
            // Folder `""` = the tree root, matching what `BookmarkPanel`
            // treats as the unfiltered set.
            ChromeAction::BookmarkHistoryEntry => {
                if let Some(url) = self.chrome_data_attr(nid, "data-hist-url").filter(|u| !u.is_empty()) {
                    let title = self
                        .chrome_data_attr(nid, "data-hist-title")
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| url.clone());
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let _ = self.bookmarks.add(&url, &title, "", &[], "", now);
                    if self.bookmark_panel.visible {
                        self.refresh_bookmarks();
                    }
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::CopyHistoryEntry => {
                if let Some(url) = self.chrome_data_attr(nid, "data-hist-url").filter(|u| !u.is_empty()) {
                    use lumen_core::ext::ClipboardProvider;
                    platform::clipboard::PlatformClipboard.write_text(&url);
                }
            }
            ChromeAction::DeleteHistoryEntry => {
                if let Some(url) = self.chrome_data_attr(nid, "data-hist-url").filter(|u| !u.is_empty()) {
                    let _ = self.history_store.delete(&url);
                    self.refresh_history();
                    self.relayout_chrome_host();
                }
            }
            // `.hist-head`'s "РћС‡РёСЃС‚РёС‚СЊ" button. Wipes the store, not just the
            // panel's cached rows вЂ” `refresh_history` then re-reads it.
            ChromeAction::ClearHistory => {
                let _ = self.history_store.clear();
                self.refresh_history();
                self.relayout_chrome_host();
            }
            ChromeAction::OpenBookmark => {
                if let Some(url) = self.chrome_data_attr(nid, "data-bm-url").filter(|u| !u.is_empty()) {
                    self.bookmark_panel.visible = false;
                    self.navigate_to(PageSource::Url(url));
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::DeleteBookmark => {
                if let Some(url) = self.chrome_data_attr(nid, "data-bm-url").filter(|u| !u.is_empty()) {
                    let _ = self.bookmarks.delete(&url);
                    self.refresh_bookmarks();
                    self.relayout_chrome_host();
                }
            }
            // `data-bm-folder` is always present on a bound `.bm-folder` row вЂ”
            // `""` is the "Р’СЃРµ Р·Р°РєР»Р°РґРєРё" row and means "no filter", so an
            // empty value is meaningful here (unlike the URL actions above).
            ChromeAction::SelectFolder => {
                if let Some(folder) = self.chrome_data_attr(nid, "data-bm-folder") {
                    self.bookmark_panel.selected_folder =
                        if folder.is_empty() { None } else { Some(folder) };
                    self.bookmark_panel.scroll_y = 0.0;
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::SelectTab => {
                if let Some(id) = self.chrome_data_id(nid, "data-tab-id")
                    && let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == id as usize)
                {
                    self.switch_tab(idx);
                }
            }
            ChromeAction::CloseTab => {
                if let Some(id) = self.chrome_data_id(nid, "data-tab-id")
                    && let Some(idx) = self.tab_strip.tabs.iter().position(|t| t.id == id as usize)
                {
                    self.close_tab(idx, event_loop);
                }
            }
            ChromeAction::SelectWorkspace => {
                if let Some(id) = self.chrome_data_id(nid, "data-ws-id") {
                    self.workspace_panel.set_active(Some(id));
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::AddWorkspace => {
                let idx = self.workspace_panel.workspaces.len();
                let name = format!("Workspace {}", idx + 1);
                let color = panels::workspace_panel::default_color_for_index(idx);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                if let Ok(id) = self.workspaces.create(&name, color, "", None, now) {
                    self.refresh_workspaces();
                    self.workspace_panel.set_active(Some(id));
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::ToggleSidebar => {
                self.chrome_sidebar_collapsed = !self.chrome_sidebar_collapsed;
                self.relayout_chrome_host();
            }
            // CC-9: `#omniDropdown` rows carry `data-sugg-idx` (stamped by
            // `bind_dropdown`, mirroring `data-tab-id`) вЂ” resolve it back to
            // `AddressBarState::suggestions()[idx]` and commit exactly like
            // `AddressBarState::commit()`'s `selected_idx` branch would, just
            // without requiring keyboard navigation to have set that index
            // first.
            ChromeAction::OmniGo => {
                if let Some(idx) =
                    self.chrome_data_id(nid, "data-sugg-idx").and_then(|i| usize::try_from(i).ok())
                {
                    self.address_bar.commit_suggestion(idx);
                    if let Some(value) = self.address_bar.take_commit() {
                        self.handle_omnibox_commit(value);
                    }
                    self.relayout_chrome_host();
                }
            }
            // CC-9: resolves the clicked button's `.perm-row` ancestor back
            // to a `PermissionKind` by position (the asset's two static rows
            // are Camera then Microphone, `PermissionKind::ALL`'s first two вЂ”
            // see `Self::chrome_permission_kind_for_node`), then sets it
            // directly per `data-perm` ("allow"/"deny"). Unlike the legacy
            // panel's single cycle button, the design has two distinct
            // buttons with no "ask" control, so this sets state directly
            // rather than calling `PermissionPanel::cycle_permission`.
            ChromeAction::SetPermission => {
                if let Some(kind) = self.chrome_permission_kind_for_node(nid)
                    && let Some(perm) = self
                        .chrome_doc
                        .as_ref()
                        .and_then(|(doc, _)| doc.get(nid).get_attr("data-perm"))
                {
                    let state = match perm {
                        "allow" => Some(panels::permission_panel::PermissionState::Allow),
                        "deny" => Some(panels::permission_panel::PermissionState::Deny),
                        _ => None,
                    };
                    if let Some(state) = state {
                        self.permission.set_permission(kind, state);
                        self.relayout_chrome_host();
                    }
                }
            }
            // CC-10: `#cpOverlay` itself carries this action (scrim click
            // closes the palette, mirroring the legacy modal's own
            // click-outside behavior) вЂ” `nid` doesn't need resolving further.
            ChromeAction::ClosePalette => {
                self.command_palette.close();
                self.relayout_chrome_host();
            }
            // CC-10: shared by `#certOverlay` and `#printOverlay` (root
            // scrim, `.modal-close`, and both footer buttons all carry this
            // same action) вЂ” which one to close is resolved by walking up
            // from `nid` to whichever modal ancestor it's inside.
            ChromeAction::CloseModal => match self.chrome_modal_ancestor(nid) {
                Some(ChromeModalKind::Cert) => {
                    self.cert_panel.close();
                    self.relayout_chrome_host();
                }
                Some(ChromeModalKind::Print) => {
                    self.print_panel.close();
                    self.relayout_chrome_host();
                }
                None => {}
            },
            // CC-10b: `.set-nav .item`/`.set-section` both carry the same
            // slug on `data-section`/`data-set` вЂ” `bind_settings` matches
            // `ChromeSettingsModel::active_section` against either
            // attribute, so this only needs to store the clicked slug.
            ChromeAction::SetSettingsSection => {
                if let Some(section) =
                    self.chrome_doc.as_ref().and_then(|(doc, _)| doc.get(nid).get_attr("data-section"))
                {
                    self.chrome_settings_section = section.to_owned();
                    self.relayout_chrome_host();
                }
            }
            // CC-10b: switches the active tab without closing the panel
            // (unlike `OpenAiSidebar`/`OpenWebSidebar`, which toggle
            // open/closed) вЂ” mirrors clicking a tab in an already-open
            // `#rightSidebar`.
            ChromeAction::SetSidebarTab => {
                if let Some(tab) =
                    self.chrome_doc.as_ref().and_then(|(doc, _)| doc.get(nid).get_attr("data-rs-tab"))
                {
                    match tab {
                        "ai" => {
                            self.ai_panel.visible = true;
                            self.sidebar.visible = false;
                        }
                        "web" => {
                            self.sidebar.visible = true;
                            self.ai_panel.visible = false;
                        }
                        _ => {}
                    }
                    self.relayout_chrome();
                    self.relayout_chrome_host();
                }
            }
            ChromeAction::CloseRightSidebar => {
                self.ai_panel.visible = false;
                self.sidebar.visible = false;
                self.relayout_chrome();
                self.relayout_chrome_host();
            }
            // BUG-421: of `#view-settings`'s six `.toggle`s, only these two
            // (Privacy в†’ "Adblock & Fingerprinting") have a clean 1:1 backing
            // field on `SettingsPanel::draft` вЂ” they got their own
            // `data-action` in the design reference (`toggleShields`/
            // `toggleFingerprintMode`) instead of the shared `toggle-switch`,
            // so `nid` here is already the specific toggle and needs no
            // structural resolver. Persisted on close via
            // `close_settings_panel` в†’ `settings_store.apply_snapshot`, same
            // as every other `draft` field.
            ChromeAction::ToggleShields => {
                self.settings_panel.toggle_shields();
                // BUG-411: the setting is the fallback every host without a
                // per-site exception uses, so a flip here has to reach the
                // live filter too вЂ” not just the draft that `close_settings_panel`
                // will persist.
                self.shields.set_default_enabled(self.settings_panel.draft.shields_enabled);
                self.sync_adblock_filter();
                self.relayout_chrome_host();
            }
            // BUG-411: the per-site switch restored into `#permPopover`. Unlike
            // `ToggleShields` above (the global setting) this keys off the
            // current host, and unlike the legacy panel's switch вЂ” which only
            // ever painted itself вЂ” it drives the real process-global filter.
            ChromeAction::ToggleSiteShields => {
                self.shields.toggle_current_site();
                self.sync_adblock_filter();
                self.relayout_chrome_host();
            }
            ChromeAction::ToggleFingerprintMode => {
                self.settings_panel.toggle_fingerprint_mode();
                self.relayout_chrome_host();
            }
            // BUG-426 reinvestigation (2026-08-01): all six of these were
            // filed together as "sit in one empty branch" but each is a
            // no-op for its own, unrelated reason вЂ” none is a small wiring
            // gap like BUG-419/420/421 turned out to be.
            //
            // `SetProfile`: `#profileMenu`/`.pm-item` in the chrome asset are
            // permanently unreachable, not just unwired вЂ” CC-15-1
            // (`docs/tasks/p1-css-chrome.md`) deliberately kept the profile
            // switcher a legacy overlay (`panels::profile_menu::build_panel`,
            // painted and hit-tested outside `chrome_doc` entirely, see the
            // `WindowEvent::MouseInput` branch above `ToggleProfileMenu`'s
            // callers) rather than migrate it to `ChromeModel`/`bind_model`;
            // nothing ever sets `#profileMenu`'s `.open` class, so it never
            // gets a layout box for the engine chrome to hit-test in the
            // first place. `ChromeModel::profile_slug` already reflects
            // whatever profile the legacy path activates, same as CC-15-1's
            // rationale describes.
            //
            // `ArchiveCard`: the two `.bm-card.readlater` demo cards
            // (`data-action="archive-card"`) live inside `#view-bookmarks`'s
            // `.bm-grid`, whose *entire* card list `bind_bookmarks`
            // (`crates/chrome/src/model.rs`) deletes and rebuilds from
            // `ChromeBookmarksModel::cards` on every relayout вЂ”
            // `remove_children_with_class(doc, grid, "bm-card")` matches by
            // class token, so it removes `.bm-card.readlater` too, and the
            // rebuilt cards never carry a `readlater`/`archive-card` variant
            // (no such concept in `ChromeBookmarkCardModel`). The action-
            // carrying markup is wiped before the first paint; this branch
            // is provably dead code, not merely low priority.
            //
            // `ToggleSwitch`: of `#view-settings`'s six `.toggle`s, the two
            // with a clean 1:1 backing field got their own `data-action` in
            // [BUG-421](../../../bugs/BUG-421-FIXED.md) (`ToggleShields`/
            // `ToggleFingerprintMode`, handled above). The remaining four
            // ("РџСЂРёРЅСѓРґРёС‚РµР»СЊРЅС‹Р№ HTTPS", the two Extensions rows, and the QA
            // "РЎС‚Р°Р±РёР»СЊРЅС‹Рµ test-id" row) have no matching real-state field at
            // all вЂ” no force-HTTPS setting, no extensions/QA-flag store вЂ”
            // so a click still can't resolve to anything.
            //
            // `ToggleFocusTimer`/`ToggleFocus`: unlike the above, real
            // backing state exists (`self.focus: FocusModePanel`) and is
            // fully interactive already вЂ” but through a *different* legacy
            // overlay (`panels::focus_panel::build_panel` + its own
            // `MouseInput`/`FocusHit` hit-test, unconditionally painted
            // whenever `self.focus.active`), not `chrome_doc`. The chrome
            // asset's `.focus-timer` pill is a simpler visual (icon + `MM:SS`
            // + two buttons) than the legacy widget's card-with-progress-ring
            // вЂ” `body` never gets a `focus-mode` class, so the pill has no
            // layout box today. Wiring these two actions for real would mean
            // either drawing both widgets at once (visibly duplicated) or
            // retiring the ring animation to cut over to the frozen design's
            // pill, the same class of legacy-overlay-vs-engine-chrome call
            // CC-15-1 already made for the profile switcher вЂ” a follow-up
            // task, not a same-shape fix as this bug's other five actions.
            //
            // `SetDevtoolsTab`: `.dt-tab`'s four static rows (Elements /
            // Console / Network / Sources, `data-dt-tab="вЂ¦"`) mock a
            // multi-panel DevTools UI the engine does not have вЂ”
            // `self.devtools_console: ConsolePanel` is a single JS-console
            // view with no per-tab data behind Elements/Network/Sources, so
            // there is nothing to switch between.
            ChromeAction::SetProfile
            | ChromeAction::ArchiveCard
            | ChromeAction::ToggleSwitch
            | ChromeAction::ToggleFocusTimer
            | ChromeAction::ToggleFocus
            | ChromeAction::SetDevtoolsTab => {}
        }
    }

    /// CC-10: which modal `nid` (a `close-modal`-carrying node, or one of its
    /// descendants) belongs to вЂ” `#certOverlay` and `#printOverlay` share the
    /// same `data-action` value, so this walks up the tree to disambiguate,
    /// mirroring [`Self::chrome_permission_kind_for_node`].
    pub(crate) fn chrome_modal_ancestor(&self, nid: NodeId) -> Option<ChromeModalKind> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        let cert_overlay = doc.find_by_id(lumen_chrome::ids::CERT_OVERLAY);
        let print_overlay = doc.find_by_id(lumen_chrome::ids::PRINT_OVERLAY);
        let mut cur = Some(nid);
        while let Some(id) = cur {
            if Some(id) == cert_overlay {
                return Some(ChromeModalKind::Cert);
            }
            if Some(id) == print_overlay {
                return Some(ChromeModalKind::Print);
            }
            cur = doc.get(id).parent;
        }
        None
    }

    /// CC-9: walks up from `nid` (a `.perm-btn` inside `#permPopover`) to its
    /// `.perm-row` ancestor, then resolves that row's position among
    /// `#permPopover`'s `.perm-row` children to a [`PermissionKind`] вЂ”
    /// `PermissionKind::ALL`'s first two entries, matching the frozen
    /// design's fixed row order (Camera, Microphone; it has no rows for
    /// Notifications/Clipboard). `None` if `nid` isn't inside a `.perm-row`,
    /// or the row's index has no matching kind.
    pub(crate) fn chrome_permission_kind_for_node(&self, nid: NodeId) -> Option<panels::permission_panel::PermissionKind> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        let has_class = |id: NodeId, class: &str| {
            doc.get(id).get_attr("class").is_some_and(|c| c.split_whitespace().any(|t| t == class))
        };
        let mut cur = doc.get(nid).parent?;
        while !has_class(cur, "perm-row") {
            cur = doc.get(cur).parent?;
        }
        let popover = doc.find_by_id(lumen_chrome::ids::PERM_POPOVER)?;
        let idx = doc.get(popover).children.iter().copied().filter(|&c| has_class(c, "perm-row")).position(|c| c == cur)?;
        panels::permission_panel::PermissionKind::ALL.get(idx).copied()
    }

    /// BUG-411: push the shields state of the current host into the
    /// process-global ad-block toggle, so `#permPopover`'s switch is the real
    /// control rather than an indicator.
    ///
    /// The filter itself stays installed either way (`config::init_adblock`);
    /// this only flips whether `fetch_single`'s gate consults it. Call after
    /// anything that can change the answer of
    /// [`shields_panel::ShieldsPanel::enabled_for_current`]: a navigation
    /// (new host), the popover switch, the settings toggle, a tab switch.
    pub(crate) fn sync_adblock_filter(&self) {
        lumen_network::set_global_adblock_enabled(self.shields.enabled_for_current());
    }

    /// Snapshot the tab strip, toolbar, and omnibox for the synthetic chrome
    /// AX nodes (DS-17) вЂ” `lumen_a11y::chrome::chrome_nodes` turns this into
    /// `TabList`/`ToolBar` siblings of the DOM-derived tree.
    pub(crate) fn chrome_snapshot(&self) -> lumen_a11y::chrome::ChromeSnapshot {
        use lumen_a11y::chrome::{ChromeButton, ChromeSnapshot, ChromeTab};

        let tabs = self
            .tab_strip
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, tab)| ChromeTab { title: tab.title.clone(), selected: idx == self.tab_strip.active })
            .collect();

        let profile_name = self.profile_menu.active_entry().map(|e| e.name.as_str()).unwrap_or("?");
        let buttons = vec![
            ChromeButton { name: format!("РџСЂРѕС„РёР»СЊ: {profile_name}"), pressed: None },
            ChromeButton { name: "РќР°Р·Р°Рґ".to_owned(), pressed: None },
            ChromeButton { name: "Р’РїРµСЂС‘Рґ".to_owned(), pressed: None },
            ChromeButton { name: "РћР±РЅРѕРІРёС‚СЊ".to_owned(), pressed: None },
            ChromeButton { name: "РќР°Р№С‚Рё РЅР° СЃС‚СЂР°РЅРёС†Рµ".to_owned(), pressed: Some(self.find.is_open()) },
            ChromeButton { name: "Р’РµР±-СЃР°Р№РґР±Р°СЂ".to_owned(), pressed: Some(self.sidebar.visible) },
            ChromeButton { name: "РР-СЃР°Р№РґР±Р°СЂ".to_owned(), pressed: Some(self.ai_panel.visible) },
            ChromeButton { name: "Р—Р°РіСЂСѓР·РєРё".to_owned(), pressed: Some(self.downloads.visible) },
            ChromeButton { name: "DevTools".to_owned(), pressed: Some(self.devtools_console.visible) },
            ChromeButton { name: "РќР°СЃС‚СЂРѕР№РєРё".to_owned(), pressed: Some(self.settings_panel.visible) },
        ];

        let omnibox_value = if self.address_bar.is_open() {
            self.address_bar.input().to_owned()
        } else {
            self.current_display_url().to_owned()
        };

        ChromeSnapshot { tabs, buttons, omnibox_value }
    }

    /// Chrome AX siblings for [`Self::update_platform_ax_tree`]/
    /// `automation_a11y_tree` вЂ” CC-13: these come from the engine-rendered
    /// chrome `Document` via `lumen_a11y::chrome::chrome_root_from_document`
    /// (real ARIA roles off `assets/chrome/chrome.html`, injected at generation
    /// time). The DS-17 synthetic-snapshot fallback below is unreachable since
    /// CC-15-6 removed the rollback flag ([`Self::chrome_doc`] is always
    /// `Some`); it is kept only as the `None`-arm of that `Option`.
    pub(crate) fn chrome_ax_nodes(&self) -> Vec<lumen_a11y::AXNode> {
        if let Some((doc, _)) = &self.chrome_doc {
            let flat_tree = lumen_dom::build_flat_tree(doc);
            vec![lumen_a11y::chrome::chrome_root_from_document(doc, doc.root(), &flat_tree)]
        } else {
            lumen_a11y::chrome::chrome_nodes(&self.chrome_snapshot())
        }
    }

    /// Rebuild the platform accessibility tree from the current DOM and push it to
    /// the OS bridge. Called after every full page load and tab switch вЂ” the
    /// chrome nodes (DS-17, CC-13) are attached as siblings so they stay live too.
    pub(crate) fn update_platform_ax_tree(&mut self) {
        let Some(src) = &self.layout_source else { return };
        let Ok(doc) = src.document.lock() else { return };
        let flat_tree = lumen_dom::build_flat_tree(&doc);
        let ax_tree = lumen_a11y::build_ax_tree(&doc, doc.root(), &flat_tree);
        let chrome = self.chrome_ax_nodes();
        let ax_tree = lumen_a11y::chrome::attach_chrome(ax_tree, chrome);
        self.platform_bridge.update(&ax_tree);
    }
}

/// BUG-341 S17 вЂ” flatten `bind_model_tracked`'s per-node report into the
/// `(NodeId, NodeChange)` pairs `restyle_root_set_for_node_change` consumes.
///
/// One node can report several attribute writes plus a child-list change in the
/// same bind; each is answered separately and the root-set unions the results,
/// so a node whose class changed *and* whose children moved still widens to its
/// parent via the structural half.
pub(crate) fn chrome_node_changes(
    touched: &lumen_chrome::ChromeMutations,
) -> impl Iterator<Item = (lumen_dom::NodeId, lumen_layout::style::NodeChange<'_>)> {
    use lumen_layout::style::NodeChange;
    touched.selector.iter().flat_map(|(id, t)| {
        t.attrs
            .iter()
            .map(move |a| (*id, NodeChange::Attr(a.as_str())))
            .chain(t.structural.then_some((*id, NodeChange::Unattributed)))
    })
}

/// CC-4/CC-9: removes `#contentArea`'s [`LayoutBox`] from `lb`'s subtree
/// (depth-first, first match) вЂ” not just its children, but its own box (and
/// background paint command) too, so the real page painted separately at
/// that rect is never covered by it. Never matches `lb` itself, only
/// descendants вЂ” `#contentArea` is never the chrome document's root box.
///
/// CC-9: two of `#contentArea`'s own children вЂ” `#findBar`, `#downloadsPanel`
/// вЂ” are real popovers this pass *does* want painted (they sit outside the
/// pruned rect via `position:absolute`, CSS Positioned Layout L3 В§9.10, so
/// splicing them elsewhere in the tree does not change which stacking
/// context they join: `#contentArea` itself creates none). `salvage_ids`
/// lists which descendant node ids to keep; they're spliced back into `lb`
/// at the exact slot `#contentArea` occupied, preserving both their absolute
/// paint rects (already resolved by the out-of-flow layout pass) and their
/// tree-order position relative to `#contentArea`'s former siblings.
///
/// BUG-341 S22: the pruning is **reversible**. Every removal is recorded in the
/// returned [`ContentAreaDetachment`], and [`restore_content_area`] puts the
/// tree back exactly as it was вЂ” which is what lets the next pass take the
/// live tree as its `prev` basis instead of the pipeline copying a pristine
/// one aside on every frame (that copy was the largest single item left in an
/// incremental chrome cycle; see the S22 census in `bugs/BUG-341-OPEN.md`).
pub(crate) fn take_content_area(
    lb: &mut LayoutBox,
    node: lumen_dom::NodeId,
    salvage_ids: &[&str],
    doc: &lumen_dom::Document,
) -> Option<(Rect, ContentAreaDetachment)> {
    let mut path = Vec::new();
    take_content_area_at(lb, node, salvage_ids, doc, &mut path)
}

/// [`take_content_area`]'s recursion, carrying the child-index path walked so
/// far so the detachment record can name `#contentArea`'s holder.
fn take_content_area_at(
    lb: &mut LayoutBox,
    node: lumen_dom::NodeId,
    salvage_ids: &[&str],
    doc: &lumen_dom::Document,
    path: &mut Vec<usize>,
) -> Option<(Rect, ContentAreaDetachment)> {
    if let Some(slot) = lb.children.iter().position(|c| c.node == node) {
        let mut removed = lb.children.remove(slot);
        let rect = removed.rect;
        let mut salvaged = Vec::new();
        salvage_layout_boxes(&mut removed, salvage_ids, doc, &mut Vec::new(), &mut salvaged);
        let mut salvage_paths = Vec::with_capacity(salvaged.len());
        for (offset, (from, b)) in salvaged.into_iter().enumerate() {
            salvage_paths.push(from);
            lb.children.insert(slot + offset, b);
        }
        return Some((
            rect,
            ContentAreaDetachment { holder_path: path.clone(), slot, removed, salvage_paths },
        ));
    }
    for (i, child) in lb.children.iter_mut().enumerate() {
        path.push(i);
        if let Some(found) = take_content_area_at(child, node, salvage_ids, doc, path) {
            return Some(found);
        }
        path.pop();
    }
    None
}

/// BUG-341 S22: everything [`take_content_area`] removed from a chrome box
/// tree, in enough detail for [`restore_content_area`] to undo it exactly.
///
/// "Exactly" is the whole point, and the cost of getting it wrong is higher
/// than it looks. The restored tree is handed to
/// `layout_mutation_incremental_restyle` as its `prev` basis, and
/// `incremental_build_box` moves whole *clean* subtrees straight across from
/// that basis вЂ” on an interaction cycle `#contentArea`'s parent is clean, so a
/// basis missing `#contentArea` produces a document missing `#contentArea`,
/// and the next cycle inherits that tree in turn. It is not a slow frame, it
/// is 155 boxes where there should be 318, permanently. Measured, and gated:
/// `bug341_s22_a_restored_basis_carries_the_whole_document_forward`.
pub(crate) struct ContentAreaDetachment {
    /// Child-index path from the tree root down to the box that held
    /// `#contentArea` (empty when the root itself held it).
    holder_path: Vec<usize>,
    /// Index `#contentArea` occupied among that holder's children вЂ” also the
    /// slot the salvaged popovers were spliced into.
    slot: usize,
    /// `#contentArea`'s own box, with the salvaged popovers already lifted out
    /// of its subtree.
    removed: LayoutBox,
    /// For each salvaged popover, in removal order, the child-index path
    /// inside [`Self::removed`] it was removed from вЂ” the last element is the
    /// index within that box's children. Restoring walks these in **reverse**,
    /// because each path was recorded against the tree state of its own
    /// removal.
    pub(crate) salvage_paths: Vec<Vec<usize>>,
}

/// BUG-341 S22: inverse of [`take_content_area`] вЂ” re-inserts the salvaged
/// popovers into `#contentArea`'s subtree and `#contentArea` back into its
/// former slot, reproducing the pre-pruning tree box for box.
///
/// Returns `false` if any recorded path no longer addresses a box (only
/// possible if something mutated the tree between the two calls, which nothing
/// does today вЂ” `chrome_layout` is read-only until the next pass replaces it).
/// The caller treats that as "no usable `prev`" and takes the full-layout path,
/// so a stale record costs a slow frame, never a wrong one.
pub(crate) fn restore_content_area(root: &mut LayoutBox, detached: ContentAreaDetachment) -> bool {
    let ContentAreaDetachment { holder_path, slot, mut removed, salvage_paths } = detached;
    let Some(holder) = follow_box_path_mut(root, &holder_path) else { return false };
    if slot + salvage_paths.len() > holder.children.len() {
        return false;
    }
    let salvaged: Vec<LayoutBox> = holder.children.drain(slot..slot + salvage_paths.len()).collect();
    for (from, b) in salvage_paths.into_iter().zip(salvaged).rev() {
        let Some((&idx, head)) = from.split_last() else { return false };
        let Some(parent) = follow_box_path_mut(&mut removed, head) else { return false };
        if idx > parent.children.len() {
            return false;
        }
        parent.children.insert(idx, b);
    }
    holder.children.insert(slot, removed);
    true
}

/// Walks `path`'s child indices down from `b`. `None` if any index is out of
/// range.
fn follow_box_path_mut<'a>(b: &'a mut LayoutBox, path: &[usize]) -> Option<&'a mut LayoutBox> {
    let mut cur = b;
    for &i in path {
        cur = cur.children.get_mut(i)?;
    }
    Some(cur)
}

/// Depth-first: removes every descendant of `lb` whose element id is in
/// `salvage_ids`, appending it to `out` in tree order together with the
/// child-index path (relative to the box this recursion started at) it came
/// from. Used by [`take_content_area`] to rescue specific popovers out of
/// `#contentArea` before the rest of its subtree is discarded вЂ” and by
/// [`restore_content_area`] to put them back.
fn salvage_layout_boxes(
    lb: &mut LayoutBox,
    salvage_ids: &[&str],
    doc: &lumen_dom::Document,
    prefix: &mut Vec<usize>,
    out: &mut Vec<(Vec<usize>, LayoutBox)>,
) {
    let mut i = 0;
    while i < lb.children.len() {
        let matches = doc.get(lb.children[i].node).get_attr("id").is_some_and(|id| salvage_ids.contains(&id));
        if matches {
            let mut from = prefix.clone();
            from.push(i);
            out.push((from, lb.children.remove(i)));
        } else {
            prefix.push(i);
            salvage_layout_boxes(&mut lb.children[i], salvage_ids, doc, prefix, out);
            prefix.pop();
            i += 1;
        }
    }
}

/// CC-10: which modal `Lumen::chrome_modal_ancestor` resolved a `CloseModal`
/// click to вЂ” `#certOverlay` and `#printOverlay` share the same
/// `data-action="close-modal"` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChromeModalKind {
    Cert,
    Print,
}
