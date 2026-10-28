//! Browser chrome (CC track): layout of the chrome host document, the model
//! snapshot handed to `lumen-chrome`, hit-testing, chrome action dispatch and
//! the synthetic chrome accessibility nodes.
//!
//! SPLIT-SH1 (2026-08-26): moved verbatim out of `main.rs`. Behaviour, order of
//! operations and method bodies are unchanged; only module path and visibility
//! (`fn` -> `pub(crate) fn`, required for a caller in the parent module) differ.
//!
//! File-size gate (docs/lint-policy.md §5.1): the chrome click hit-test and the
//! `data-action` dispatcher live in `chrome_action.rs`.

use crate::*;

/// Console lines `#dt-console` has room for (250 px panel minus its tab row).
const CONSOLE_VISIBLE_LINES: usize = 8;

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
    /// `#omniInput` — `Some` exactly while the legacy `address_bar` is open;
    /// its caret is still hand-painted (no native `<input>` caret exists,
    /// see [`Self::chrome_omni_input_rect`]), only editing state moved to
    /// chrome-DOM. The interactive thread-locals are process-wide (brief
    /// risk #6), so this pass explicitly sets them from chrome's own state
    /// rather than inheriting whatever the page's last [`Self::relayout`]
    /// left behind, and clears them again afterward so a subsequent page
    /// relayout does not inherit chrome's state either.
    ///
    /// The design reference's `#contentArea` — the container it reserves for
    /// live tab content, doubling as the brief's "`#page-host`" (no new id
    /// was introduced) — carries its own placeholder markup (new-tab tiles, a
    /// demo site page, …) meant for standalone preview
    /// (`about:chrome-preview`, CC-1), not for stacking under the real tab
    /// content this host paints separately at that same rect
    /// ([`Self::chrome_page_host_rect`]). Since this pass's display list
    /// paints *above* the page in `overlay_buf`, leaving that placeholder in
    /// — or even just clearing its children but keeping its own box — would
    /// permanently hide the real page behind either the placeholder markup or
    /// `#contentArea`'s own `background:var(--surface-0)` fill. This pass
    /// therefore removes `#contentArea` from the tree entirely
    /// ([`take_content_area`]) right after layout, capturing its rect into
    /// `chrome_page_host_rect` first, and before [`paint_ordered`] — except
    /// `#findBar`/`#downloadsPanel` (CC-9), salvaged back into the tree at
    /// `#contentArea`'s former slot since they're real popovers, not preview
    /// placeholder content.
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
        // layout pass — no separate dirty flag, `bind_model` is cheap (a
        // handful of attribute/text mutations + two small list rebuilds).
        if self.network_panel.visible {
            self.network_panel.refresh();
        }
        if self.privacy.visible {
            self.privacy.refresh();
        }
        if self.dom_inspector.visible {
            self.dom_inspector.set_network_entries(self.network_panel.entries_clone());
        }
        self.refresh_hbar_freeze();
        let model = self.chrome_model_snapshot();
        let Some((doc, sheet)) = self.chrome_doc.as_mut() else { return };
        // BUG-341 S6: `bind_model_tracked` (not plain `bind_model`) — reports
        // every node whose selector-relevant attribute/class actually changed
        // value, or whose row-list container gained/lost a member, so a
        // content-mutating pass (typed omnibox text, a tab title, …) can also
        // take the incremental path below instead of only a pure
        // interactive-state transition (S5's limit — see BUG-341 "S5" §"Not
        // attempted").
        let touched = lumen_chrome::bind_model_tracked(doc, &model);
        // BUG-625: меряем тем же резолвом семейств, которым хром рисуется.
        let Some(measurer) = chrome_measurer() else { return };
        // CC-7: `#omniInput` is focused (`:focus`/`:focus-within`, e.g. the
        // `.omnibox` accent ring) exactly while the legacy `address_bar` is
        // open — there is no other focusable element in the chrome document
        // yet, so a single node id is enough.
        let omni_input = doc.find_by_id(lumen_chrome::ids::OMNI_INPUT);
        let chrome_focus = if self.address_bar.is_open() { omni_input } else { None };
        lumen_layout::set_interactive_state(self.chrome_hovered_nid, chrome_focus, self.chrome_active_nid);
        // BUG-341 S5/S6: `layout_mutation_incremental` (plain graft_geometry,
        // no incremental cascade) was tried here early on and measured
        // *worse* than a plain full layout (`graft_geometry`'s then-O(depth)
        // redundant clone bug) — fixed since (BUG-341 "third session"), and
        // now combined with the S3 incremental cascade via
        // `layout_mutation_incremental_restyle`. Safe whenever a previous
        // pristine tree/cascade cache exists to graft/diff against and
        // viewport/Forced-Colors still match — a resize or Forced-Colors flip
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
        // out — putting the second back into the first yields the pre-pruning
        // tree box for box, for the price of one insert per salvaged popover
        // instead of a whole-tree copy. Taking `chrome_layout` by value is
        // what makes it free: the incremental path below *moves* the reusable
        // subtrees out of the basis (S19), so the basis does not survive the
        // call either way. Both fields are reassigned from this pass's own
        // result further down, on both arms; the display list is dropped with
        // the tree because this pass builds a new one.
        let mut prev_pristine = match (self.chrome_layout.take(), self.chrome_content_area_detached.take()) {
            (Some((mut lb, _stale_dl)), Some(detached)) => restore_content_area(&mut lb, detached).then_some(lb),
            // No `#contentArea` in the chrome document (or it had no box):
            // nothing was pruned, so the live tree *is* the pristine one.
            (Some((lb, _stale_dl)), None) => Some(lb),
            (None, _) => None,
        };
        // BUG-1059: undo the previous pass's `take_floating_panel` calls
        // (below) the same way `restore_content_area` above undoes its
        // `#contentArea` pruning — the incremental basis must be the FULL
        // pristine tree, `#demoBar`/`#infoPanel` included, or the box-reuse
        // graft below would find those nodes missing from `prev` and (per
        // `ContentAreaDetachment`'s own doc comment) silently carry that
        // absence forward every pass after. A restore failure discards the
        // whole basis, same fallback `restore_content_area` uses.
        let floating_detached = std::mem::take(&mut self.chrome_floating_detached);
        if let Some(lb) = prev_pristine.as_mut()
            && !floating_detached.into_iter().rev().all(|d| restore_floating_panel(lb, d))
        {
            prev_pristine = None;
        }
        let (mut layout, cascade_styles) = match (
            viewport_stable && forced_colors_stable,
            prev_pristine,
        ) {
            (true, Some(prev)) => {
                let (prev_hover, prev_focus, prev_active) = self.chrome_prev_interactive;
                // BUG-341 S7: computed once per pass, not once per axis — the
                // stylesheet/shadow-DOM shape doesn't change between the three
                // hover/focus/active calls below.
                // PERF-16 срез 4: and kept from pass to pass, the sheet being the same one.
                let (state_index, node_index) = self.chrome_restyle_indexes.indexes(doc, sheet);
                let mut dirty_roots = std::collections::HashSet::new();
                dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                    doc,
                    prev_hover,
                    new_interactive.0,
                    state_index,
                ));
                dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                    doc,
                    prev_focus,
                    new_interactive.1,
                    state_index,
                ));
                dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                    doc,
                    prev_active,
                    new_interactive.2,
                    state_index,
                ));
                // BUG-341 S6: DOM-mutation root-set, unioned with the
                // interactive-state one above — `touched` is empty on a pure
                // hover/focus/active-only pass (S5's original case), non-empty
                // whenever `bind_model` actually changed content this cycle.
                // BUG-341 S17: the report names the mutated attributes, so the
                // root-set can narrow each one to the node itself unless some
                // selector reaches a sibling from a compound matching it.
                dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(
                    doc,
                    chrome_node_changes(&touched),
                    node_index,
                ));
                let delta = lumen_layout::counters::RestyleDelta {
                    prev_styles: std::mem::take(&mut self.chrome_prev_cascade_styles),
                    dirty_roots,
                    // BUG-341 S16: `bind_model_tracked` enumerates every node
                    // whose content it mutated (see `ChromeMutations::content`
                    // and the source-level completeness gate behind it), so
                    // this cycle can name them instead of declaring the whole
                    // document unstable the way S4-S15 had to — one changed
                    // omnibox character no longer costs all 318 boxes.
                    content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
                    shallow_roots: Default::default(), point_roots: Default::default(),
                };
                lumen_layout::counters::set_incremental_restyle(true);
                // BUG-341 S15: reuse whole box subtrees from `prev` too, not
                // just their geometry. Licensed by the `content_dirty` record
                // this call site establishes above — a subtree containing a
                // mutated node stays out of `clean_subtrees`.
                lumen_layout::box_tree::set_incremental_box_build(true);
                let result = lumen_layout::box_tree::layout_mutation_incremental_restyle(
                    doc,
                    sheet,
                    viewport,
                    measurer,
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
                measurer,
                &*self.hyp_provider,
                self.dark_mode,
            ),
        };
        lumen_layout::clear_interactive_state();
        // Persist this pass's cascade cache as the `prev` basis for the next
        // call's incremental path (BUG-341 S5). Its box-tree counterpart is no
        // longer copied here — S22 records `take_content_area`'s removals
        // below instead and undoes them at the top of the next pass.
        self.chrome_prev_cascade_styles = cascade_styles.into_styles();
        self.chrome_prev_viewport = Some(viewport);
        self.chrome_prev_interactive = new_interactive;
        self.chrome_prev_forced_colors = forced_colors;
        // CC-9: `#findBar`/`#downloadsPanel` are salvaged out of
        // `#contentArea` before the rest of it is discarded — see
        // `take_content_area`'s doc comment. CC-10 adds the command palette
        // and cert/print modals — all three are `position:absolute` direct
        // children of `#contentArea` too, same reasoning as `#findBar`/
        // `#downloadsPanel`.
        // BUG-341 S22: the detachment record is kept, not discarded — it is
        // what lets the next pass rebuild this tree's pristine form instead of
        // this pass copying one aside.
        let pruned = doc.find_by_id(lumen_chrome::ids::CONTENT_AREA).and_then(|id| {
            take_content_area(
                &mut layout,
                id,
                &SALVAGED_POPOVER_IDS,
                doc,
            )
        });
        let (page_host_rect, detached) = match pruned {
            Some((rect, detached)) => (Some(rect), Some(detached)),
            None => (None, None),
        };
        self.chrome_page_host_rect = page_host_rect;
        self.chrome_content_area_detached = detached;
        // BUG-1059: `#demoBar`/`#infoPanel` are `position:fixed` siblings of
        // `#contentArea` (not descendants — chrome.html declares them
        // directly under `<body>`), deliberately positioned inside
        // `chrome_page_host_rect`. Detach them from `layout` before it is
        // flattened into `chrome_dl` below, so `build_chrome_overlay_strips`'s
        // 4-strip clip (cut around that same rect) never sees them — see
        // `FloatingPanelDetachment`'s doc comment for why they'd otherwise be
        // silently discarded. Painted separately, unclipped, via
        // `chrome_floating_dl` (`RedrawRequested`).
        let popover_nodes: Vec<_> =
            SALVAGED_POPOVER_IDS.iter().filter_map(|&id| doc.find_by_id(id).map(|n| (n, id))).collect();
        let mut floating_detached = Vec::new();
        for id in [lumen_chrome::ids::DEMO_BAR, lumen_chrome::ids::INFO_PANEL, lumen_chrome::ids::UPDATE_BAR, lumen_chrome::ids::LOGIN_BAR, lumen_chrome::ids::MEDIA_BAR, lumen_chrome::ids::HANG_BAR, lumen_chrome::ids::DIALOG_BOX, lumen_chrome::ids::SECURITY_BOX, lumen_chrome::ids::PROFILE_FRAME] {
            if let Some(node) = doc.find_by_id(id)
                && let Some((_rect, detached)) = take_floating_panel(&mut layout, node, id)
            {
                floating_detached.push(detached);
            }
        }
        self.chrome_floating_detached = floating_detached;
        // CC-18 срез 3: the display list is built by a shared helper, so a
        // drag step can rebuild it (with the panel's offset applied) without
        // a relayout. With no offset it emits exactly what the loop above
        // used to emit inline.
        self.rebuild_chrome_floating_dl();
        // CC-7/CC-9: captured non-destructively (unlike `#contentArea`
        // above) — these nodes stay in the tree and paint normally.
        let omni_box = omni_input.and_then(|id| lumen_layout::find_box_by_node(&layout, id));
        self.chrome_omni_input_rect = omni_box.map(|b| b.rect);
        if let Some(b) = omni_box {
            self.chrome_omni_font_size = b.style.font_size;
        }
        // CC-11: sync transitions — compare chrome_prev_styles with the fresh
        // layout before replacing it, mirroring apply_relayout_result's
        // page-side sync (main.rs's collect_box_styles + sync loop). No
        // @starting-style handling here: bind_model mutates existing
        // chrome_doc nodes in place rather than inserting/removing them, so
        // there are no "entering" nodes the way JS page mutation can produce.
        let now_s = self.epoch.elapsed().as_secs_f32();
        let mut new_styles = crate::layout_walk::StyleMap::default();
        collect_box_styles(&layout, &mut new_styles);
        for (node, new_style) in &new_styles {
            if let Some(old_style) = self.chrome_prev_styles.get(node) {
                self.chrome_transition_scheduler.sync(*node, old_style, new_style, now_s);
            }
        }
        self.chrome_prev_styles = new_styles;
        // BUG-1605: the salvaged popovers lie inside `chrome_page_host_rect`, so
        // the strip clip in `build_chrome_overlay_strips` would discard every
        // one of them. Lift them out for the chrome paint, paint them on their
        // own (z-index order) and put them back — hit-testing and the next
        // pass's incremental basis keep seeing them in `layout`.
        let mut popovers = Vec::new();
        for (node, id) in popover_nodes {
            if let Some((_rect, detached)) = take_floating_panel(&mut layout, node, id) {
                popovers.push(detached);
            }
        }
        let dl = paint_ordered_cached(&layout, &mut self.chrome_emit_cache);
        let mut popover_dl = lumen_paint::DisplayList::new();
        let mut by_z: Vec<&FloatingPanelDetachment> = popovers.iter().collect();
        by_z.sort_by_key(|d| d.removed.style.z_index.unwrap_or(0));
        for d in by_z {
            popover_dl.extend_from_slice(&paint_ordered(&d.removed));
        }
        self.chrome_popover_dl = (!popover_dl.is_empty()).then_some(popover_dl);
        for d in popovers.into_iter().rev() {
            let _ = restore_floating_panel(&mut layout, d);
        }
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
        // BUG-405 срез 50: bumped unconditionally (not just when `dl`'s bytes
        // actually differ) — a safe superset of "content changed" that lets
        // `ChromeOverlayFrameCache` invalidate on generation equality alone,
        // no hashing needed. See the struct's own doc comment.
        self.chrome_layout_generation = self.chrome_layout_generation.wrapping_add(1);
        self.chrome_layout = Some((layout, dl));
    }

    /// Strip indices of the sidebar tab list in display order, with the tree
    /// placement of each row in tree mode (UX-CHROME-PANELS 14,
    /// `Ctrl+Shift+B`): `tree::visible_order` omits collapsed subtrees and
    /// gives every row its depth and (with children) a collapse arrow. The flat
    /// list follows `visible_indices()` (BUG-409: a collapsed group's
    /// non-leftmost members stay hidden behind the chip row).
    pub(crate) fn sidebar_tab_order(&self) -> Vec<(usize, Option<lumen_chrome::ChromeTabTree>)> {
        if !self.tree_tabs.visible {
            return self.tab_strip.visible_indices().into_iter().map(|i| (i, None)).collect();
        }
        tabs::tree::visible_order(&self.tab_strip.tabs, &self.tree_tabs.collapsed)
            .iter()
            .map(|r| {
                let tree = lumen_chrome::ChromeTabTree {
                    depth: r.depth,
                    expanded: r.has_children.then(|| !self.tree_tabs.collapsed.contains(&r.id)),
                };
                (r.strip_idx, Some(tree))
            })
            .collect()
    }

    /// CC-6 (docs/tasks/p1-css-chrome.md): snapshots tab strip, workspaces,
    /// theme, tab layout, and active profile into a [`lumen_chrome::ChromeModel`]
    /// — [`Self::relayout_chrome_host`] binds this before every chrome layout
    /// pass. Mirrors the same shell fields [`Self::chrome_snapshot`] (DS-17 a11y
    /// tree) reads, shaped for [`lumen_chrome::bind_model`] instead. CC-7 adds
    /// the omnibox value/spoof-warning, read from the legacy `address_bar`.
    pub(crate) fn chrome_model_snapshot(&self) -> lumen_chrome::ChromeModel {
        let active_id = self.tab_strip.tabs.get(self.tab_strip.active).map(|t| t.id);
        // BUG-409: iterate `visible_indices()` rather than `tabs` directly —
        // a collapsed group's non-leftmost members stay hidden behind the
        // leftmost (chip) row, mirroring the legacy strip's own collapse
        // behaviour. For a strip with no collapsed groups this is `0..len()`.
        let order = self.sidebar_tab_order();
        // The sidebar list is windowed by `vertical_tabs.scroll_rows` (the
        // chrome has no wheel-scrollable container).
        let skip = self
            .vertical_tabs
            .scroll_rows
            .min(panels::vertical_tabs::VerticalTabsPanel::max_scroll_rows(order.len()));
        let tabs = order
            .into_iter()
            .skip(skip)
            .map(|(i, tree)| {
                let t = &self.tab_strip.tabs[i];
                lumen_chrome::ChromeTabModel {
                    tree,
                    id: t.id,
                    title: t.title.clone(),
                    active: Some(t.id) == active_id,
                    sleeping: t.tab_state == TabState::Hibernated,
                    // UX-LOADING: only the active tab's navigation is live —
                    // a background tab's streaming events are discarded
                    // (`mark_bg_tab_needs_reload`), so it is not "loading".
                    loading: Some(t.id) == active_id && self.nav_start.is_some(),
                    // CC-8: tree-style tabs (7A.2) — a tab with an opener is
                    // rendered as a `.child` row with a `.tree-line` connector.
                    // The asset's CSS only indents one nesting level, so this
                    // collapses depth ≥1 to a single boolean rather than
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
        // applies — the asset's `.dropdown` isn't scroll-clipped, so an
        // uncapped list would grow the popover past its designed height.
        let dropdown_suggestions: Vec<lumen_chrome::ChromeSuggestionModel> = self
            .address_bar
            .suggestions()
            .iter()
            .take(address_bar::MAX_VISIBLE)
            .enumerate()
            .map(|(idx, s)| {
                // CC-15-3/DS-6: punycode-guard both strings, as the legacy
                // `build_dropdown` did — without this a homograph host in a
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
        // CC-9: `current_matches()` re-scans the display list for the query —
        // cheap relative to a full relayout, and this snapshot only runs on
        // explicit chrome-relayout triggers (resize/click/key), not every
        // `RedrawRequested` frame (see `Self::relayout_chrome_host`'s doc).
        let (find_matches, find_layout_fix) =
            if self.find.is_open() { self.current_matches_with_fix() } else { (Vec::new(), None) };
        let find_matches_len = find_matches.len();
        // CC-15-6/BUG-419: the "ERR" state is carried over from the deleted
        // legacy bar (`find::append_bar`) — without it an invalid regex is
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
            layout_fix: find_layout_fix.unwrap_or_default(),
        };
        let downloads: Vec<lumen_chrome::ChromeDownloadModel> = self
            .downloads
            .entries()
            .iter()
            .map(|d| {
                let (meta, progress_fraction) = match &d.status {
                    download::DownloadStatus::Pending => ("В очереди…".to_owned(), None),
                    download::DownloadStatus::InProgress => {
                        let text = match d.total {
                            Some(t) if t > 0 => format!(
                                "{} / {} — идёт загрузка…",
                                download::human_bytes(d.received),
                                download::human_bytes(t)
                            ),
                            _ => "Загрузка…".to_owned(),
                        };
                        (text, Some(d.progress_fraction().unwrap_or(0.6)))
                    }
                    download::DownloadStatus::Done { bytes } => {
                        (format!("{} — готово", download::human_bytes(*bytes)), None)
                    }
                    download::DownloadStatus::Failed(reason) => (format!("Ошибка: {reason}"), None),
                    download::DownloadStatus::Cancelled => ("Отменено".to_owned(), None),
                };
                lumen_chrome::ChromeDownloadModel {
                    id: d.id.raw(),
                    ext_label: download::extension_label(&d.filename),
                    name: d.filename.clone(),
                    meta,
                    progress_fraction,
                    actions: match &d.status {
                        download::DownloadStatus::Done { .. } => vec![
                            lumen_chrome::ChromeDownloadAction::Open,
                            lumen_chrome::ChromeDownloadAction::Reveal,
                        ],
                        download::DownloadStatus::InProgress | download::DownloadStatus::Pending => {
                            vec![lumen_chrome::ChromeDownloadAction::Cancel]
                        }
                        _ => Vec::new(),
                    },
                }
            })
            .collect();
        // BUG-408: mirrors `TabArchive`'s entries into `#archivePanel`'s
        // `.arc-list` — same shape `ChromeTabModel`'s favicon fallback and
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
        // permission rows into one `#permPopover` — no separate engine
        // control exists for `PermissionPanel::visible` (`Ctrl+Shift+P`), so
        // either legacy toggle shows it.
        let popover_open = self.shields.visible || self.permission.visible;
        // BUG-411: all four `PermissionKind::ALL` rows — the asset gained the
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
        // can render — a command's keyboard shortcut, or a bookmark/history
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
                        index: rank,
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
        // `PanelCertData`'s fields (no TLS-version/revocation/CT-status/error
        // slot) — missing/empty individual fields render as `"—"`, mirroring
        // `cert_panel::build_rows`'s own em-dash fallback.
        let dash = |s: &str| if s.is_empty() { "\u{2014}".to_owned() } else { s.to_owned() };
        let cert = match &self.cert_panel.cert {
            Some(c) if c.has_data() => {
                let san = if c.san_list.is_empty() { "\u{2014}".to_owned() } else { c.san_list.join(", ") };
                let issuer = if !c.issuer_org.is_empty() { c.issuer_org.clone() } else { dash(&c.issuer_cn) };
                // A6: `#certOverlay` has no dedicated warning slot yet (see
                // `PanelCertData::error`'s doc) — until that CC design-asset
                // slice lands, at least the title communicates the warning
                // rather than silently showing a clean-looking green panel
                // for a cert only accepted via "Proceed anyway".
                let title = if c.has_error() {
                    format!("⚠ Сертификат небезопасен — {}", dash(&c.subject_cn))
                } else {
                    format!("Сертификат — {}", dash(&c.subject_cn))
                };
                lumen_chrome::ChromeCertModel {
                    open: self.cert_panel.visible,
                    title,
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
                title: "Сертификат недоступен".to_owned(),
                rows: std::array::from_fn(|_| "\u{2014}".to_owned()),
                fingerprint: "\u{2014}".to_owned(),
            },
        };
        // UX-CHROME-PANELS: `#shortcutsOverlay` — the shell windows the rows
        // (the chrome has no wheel-scroll container).
        let shortcuts = {
            let panel = &self.shortcuts_panel;
            let count = panels::shortcuts_panel::ShortcutsPanel::visible_count(self.viewport_height_css());
            let rows = panel
                .rows
                .iter()
                .enumerate()
                .skip(panel.first_row)
                .take(count)
                .map(|(index, r)| {
                    let rebinding = panel.rebinding == Some(index);
                    lumen_chrome::ChromeShortcutRowModel {
                        index,
                        label: r.label.to_owned(),
                        binding: if rebinding { "Нажмите клавишу…".to_owned() } else { r.binding_label() },
                        rebinding,
                    }
                })
                .collect();
            lumen_chrome::ChromeShortcutsModel { open: panel.visible, rows }
        };
        // UX-CHROME-PANELS: `#devtools` shows the tail of the JS console (the
        // chrome has no wheel-scroll container, so no scrolling).
        let console = {
            let panel = &self.devtools_console;
            let msgs = panel.messages();
            let lines = msgs[msgs.len().saturating_sub(CONSOLE_VISIBLE_LINES)..]
                .iter()
                .map(|m| lumen_chrome::ChromeConsoleLineModel {
                    level: match m.level {
                        devtools::console_panel::ConsoleLevel::Log => 0,
                        devtools::console_panel::ConsoleLevel::Warn => 1,
                        devtools::console_panel::ConsoleLevel::Error => 2,
                    },
                    text: m.text.clone(),
                })
                .collect();
            lumen_chrome::ChromeConsoleModel { open: panel.visible, total: msgs.len(), lines }
        };
        // UX-CHROME-PANELS: `#networkPanel` mirrors the (already refreshed) `NetworkPanel`.
        let network = self.network_panel.chrome_model();
        // UX-CHROME-PANELS: `#readLaterPanel` mirrors `ReadLaterPanel`.
        let read_later = self.read_later_panel.chrome_model();
        // UX-CHROME-PANELS: `#privacyPanel` mirrors the (already refreshed) `PrivacyPanel`.
        let privacy = self.privacy.chrome_model(panels::privacy_panel::list_body_height(
            self.viewport_height_css() + toolbar::CHROME_H,
            toolbar::CHROME_H,
        ));
        // UX-CHROME-PANELS: `#accessOverlay` mirrors the draft of `A11yPanel`.
        let access = {
            let panel = &self.a11y_panel;
            let m = panel.draft.font_size_multiplier;
            let font_value = [0.8_f64, 1.0, 1.25, 1.5, 2.0]
                .iter()
                .find(|v| (**v - m).abs() < 1e-6)
                .map(|v| v.to_string())
                .unwrap_or_default();
            let cursor_value = match panel.draft.cursor_size {
                lumen_storage::CursorSize::Normal => "normal",
                lumen_storage::CursorSize::Large => "large",
                lumen_storage::CursorSize::ExtraLarge => "extra",
            };
            lumen_chrome::ChromeAccessModel {
                open: panel.visible,
                font_value,
                reduced_motion: panel.draft.reduced_motion,
                forced_colors: panel.draft.forced_colors,
                cursor_value: cursor_value.to_owned(),
            }
        };
        // CC-10b: which `#contentArea` view is shown — mirrors whichever of
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
        // CC-10b: DS-16's "history not saved" banner — same anonymous-profile
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
        // CC-10b: `"Все закладки"` (the `None`-filter entry) followed by the
        // real folder set — mirrors `bookmark_panel::hit_test`'s own "All"
        // row convention.
        let mut bookmark_folders = vec![lumen_chrome::ChromeBookmarkFolderModel {
            label: "Все закладки".to_owned(),
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
            title: self.bookmark_panel.selected_folder.clone().unwrap_or_else(|| "Все закладки".to_owned()),
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
        // UX-PASSWORDS срез 5: список читается из SQLite, поэтому только пока
        // открыта именно вкладка «Пароли».
        let passwords_store = password_store::global().filter(|_| !self.active_profile_is_anonymous());
        let passwords = if self.settings_panel.visible && self.chrome_settings_section == "passwords" {
            passwords_store
                .and_then(|s| s.list().ok())
                .unwrap_or_default()
                .into_iter()
                .filter(|(origin, _)| origin != card_store::ORIGIN)
                .collect()
        } else {
            Vec::new()
        };
        // UX-AUTOFILL срез 5: значения форм и карты читаются так же — только на своей вкладке.
        let autofill_store = autofill_store::global().filter(|_| !self.active_profile_is_anonymous());
        let autofill_open = self.settings_panel.visible && self.chrome_settings_section == "autofill";
        let autofill: Vec<(String, String, String)> = if autofill_open {
            autofill_store
                .and_then(|s| s.list_all().ok())
                .unwrap_or_default()
                .into_iter()
                .map(|e| (e.origin, e.field_name, e.value))
                .collect()
        } else {
            Vec::new()
        };
        let cards: Vec<(String, String)> = if autofill_open {
            passwords_store
                .map(|s| card_store::saved_cards(s).iter().map(|c| (c.last4(), c.label())).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let settings = lumen_chrome::ChromeSettingsModel {
            active_section: self.chrome_settings_section.clone(),
            ad_block_on: self.settings_panel.draft.shields_enabled,
            fingerprint_on: self.settings_panel.draft.fingerprint_mode != "off",
            passwords,
            passwords_available: passwords_store.is_some(),
            autofill,
            cards,
            autofill_available: autofill_store.is_some(),
            site_permissions: if self.settings_panel.visible && self.chrome_settings_section == "permissions" {
                self.permission
                    .saved()
                    .into_iter()
                    .map(|(origin, kind, state)| {
                        (origin, kind.label().to_owned(), state == panels::permission_panel::PermissionState::Allow)
                    })
                    .collect()
            } else {
                Vec::new()
            },
            current_site: self
                .permission
                .current_origin
                .as_deref()
                .and_then(lumen_storage::PartitionKey::parse)
                .map(|k| k.site().to_owned()),
            spell_enabled: crate::assets::SPELL_DICTS.get().is_none_or(|d| d.prefs().enabled),
            spell_languages: if self.settings_panel.visible && self.chrome_settings_section == "spelling" {
                crate::assets::SPELL_DICTS.get().map(|d| d.languages()).unwrap_or_default()
            } else {
                Vec::new()
            },
            translate_languages: if self.settings_panel.visible && self.chrome_settings_section == "translate" {
                let current = self.settings_store.translate_language();
                crate::translate_manager::LANGUAGES
                    .iter()
                    .map(|(code, _, label)| ((*code).to_owned(), (*label).to_owned(), *code == current))
                    .collect()
            } else {
                Vec::new()
            },
            translate_model: if self.settings_panel.focused_input == Some(panels::settings_panel::SettingInput::TranslateModel) {
                format!("{}▏", self.settings_panel.translate_model_draft)
            } else {
                Some(self.settings_store.translate_model())
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| crate::translate_manager::DEFAULT_MODEL.to_owned())
            },
        };
        // CC-10b: the design's single tabbed `#rightSidebar` merges the
        // legacy independently-dockable `ai_panel`/`sidebar` — kept mutually
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
            layout_vertical: self.vertical_tabs.visible || self.tree_tabs.visible,
            profile_slug,
            profile_menu_open: self.profile_menu.visible,
            control_panel: lumen_chrome::ChromeControlPanelModel {
                shape: self.control_panel_shape,
                mini_open: self.control_panel_mini_open,
                info_open: self.control_panel_info_open,
            },
            tabs,
            workspaces,
            omnibox: lumen_chrome::OmniboxModel {
                value: omnibox_value,
                warning: omnibox_warning.map(str::to_owned),
                zoom_percent: zoom::percent_label(self.zoom_factor),
                popups_blocked: self
                    .blocked_popups
                    .get(&self.tab_strip.tabs[self.tab_strip.active].id)
                    .map_or(0, |urls| u32::try_from(urls.len()).unwrap_or(u32::MAX)),
                help_chip: self.help_chip_label(),
            },
            sidebar_collapsed: self.chrome_sidebar_collapsed,
            dropdown,
            find,
            downloads_open: self.downloads.visible,
            downloads,
            downloads_clearable: self.downloads.has_finished_entries(),
            archive_open: self.archive.visible,
            archive,
            popover_open,
            blocked_total: self.shields.blocked_total_count(),
            permissions,
            // BUG-411: the popover names the host it applies to and carries the
            // per-site shields switch — both lost when CC-15-4 removed the
            // legacy panels that used to show them.
            popover_domain: self.shields.current_domain.clone().unwrap_or_default(),
            site_shields_on: self.shields.enabled_for_current(),
            compat_fixes: u32::try_from(self.compat_applied.len()).unwrap_or(u32::MAX),
            fix: self.fix_panel_model(),
            palette,
            cert,
            shortcuts,
            console,
            network,
            inspector: self.dom_inspector.chrome_model(),
            read_later,
            privacy,
            focus: self.focus.chrome_model(),
            note: self.note_viewer.chrome_model(),
            pip: self.pip.chrome_model(),
            tab_ctx: self
                .tab_context_menu
                .chrome_model(self.viewport_width_css(), self.window_height_css()),
            page_ctx: self
                .page_context_menu
                .chrome_model(self.viewport_width_css(), self.window_height_css()),
            workspace_bar_open: self.workspace_panel.visible,
            hbar_tab_width: self.hbar_freeze.map(|f| f.width),
            access,
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
            update: self.update_ui.chrome_model(),
            login_offer: self.login_offer_model(),
            media_bar: self.media_bar.model(),
            hang_bar: self.script_hang.model(),
            dialog: self.page_dialog_model(),
            security: self.security_model(),
        }
    }

    /// CC-8: renders a [`lumen_layout::Color`] as the `#RRGGBB` string
    /// `ChromeModel`'s container/workspace colour fields need (CSS custom
    /// properties and inline `style="background:…"` are both plain text).
    /// Drops alpha — every caller (container accent, workspace accent) is
    /// opaque in practice.
    pub(crate) fn chrome_hex_color(c: lumen_layout::Color) -> String {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    }

    /// CC-5 (docs/tasks/p1-css-chrome.md): where the page content starts, in
    /// window CSS pixels — the single source of truth input-coordinate
    /// conversion ([`Self::page_point`], [`Self::update_cursor_icon`]) and
    /// the render-time page transform share, so a click/hover always lands
    /// on the same page element the frame actually painted there.
    ///
    /// This is [`Self::chrome_page_host_rect`]'s origin, falling back to
    /// `(0, CHROME_H)` before the first chrome layout exists, mirroring
    /// [`Self::relayout_chrome_host`]'s own degenerate-size guard — that frame
    /// paints the page flush with the window too.
    pub(crate) fn page_offset(&self) -> (f32, f32) {
        self.chrome_page_host_rect
            .map(|r| (r.x, r.y))
            .unwrap_or((0.0, toolbar::CHROME_H))
    }

    /// CC-5: `true` when `(x_css, y_css)` falls outside the page-content
    /// rect — i.e. over an opaque chrome furniture area (sidebar, toolbar,
    /// tab strip). A `None` [`Self::chrome_page_host_rect`] (no chrome layout
    /// yet) counts as "over chrome": mirrors [`Self::relayout_chrome_host`]'s
    /// guard — nothing is painted at the page rect either in that frame, so
    /// there is no page underneath to click through to yet.
    pub(crate) fn point_over_chrome(&self, x_css: f32, y_css: f32) -> bool {
        match self.chrome_page_host_rect {
            Some(r) => {
                x_css < r.x || x_css >= r.right() || y_css < r.y || y_css >= r.bottom()
            }
            None => true,
        }
    }

    /// CC-5: hit-tests [`Self::chrome_layout`] at window-CSS coordinates —
    /// the chrome document paints at the window origin, no scroll/page
    /// transform involved. `None` before the first chrome layout exists.
    pub(crate) fn chrome_hit_test(&self, x_css: f32, y_css: f32) -> Option<lumen_paint::HitTestResult> {
        let (layout, _) = self.chrome_layout.as_ref()?;
        hit_test(Point::new(x_css, y_css), layout)
    }

    /// CC-5: walks `hit.path` (bubble order, closest node first — the same
    /// list `HitTestResult` already builds for event-dispatch bubbling) for
    /// the nearest ancestor carrying a recognised `data-action`, mirroring
    /// how the legacy toolbar/tab-strip hit-testers resolve a click to one
    /// semantic action regardless of which child element (icon, label,
    /// badge) the point actually landed on. Returns the carrying node too —
    /// `dispatch_chrome_action` needs it to read action-specific sibling
    /// attributes (`data-view`, …).
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

    /// Resolves `id` against [`Self::chrome_doc`] and returns that node's
    /// rect in [`Self::chrome_layout`] — `None` before the first chrome
    /// layout exists, or if `id` does not resolve (should not happen for any
    /// `ids::*` constant, which the build-time gate guarantees exists in the
    /// asset — see `crates/chrome/build.rs`).
    fn chrome_node_rect(&self, id: &str) -> Option<Rect> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        let (layout, _) = self.chrome_layout.as_ref()?;
        let nid = doc.find_by_id(id)?;
        lumen_layout::find_box_by_node(layout, nid).map(|b| b.rect)
    }

    /// `#sbTabs` rect (UX-CHROME-PANELS 14): where the wheel scrolls the
    /// sidebar tab list.
    pub(crate) fn chrome_sb_tabs_rect(&self) -> Option<Rect> {
        self.chrome_node_rect(lumen_chrome::ids::SB_TABS)
    }

    /// UX-TAB-CLOSE-FREEZE: pins the horizontal strip to the width tab
    /// `tab_id` has right now, so the neighbour that slides under the cursor
    /// after the close is exactly as wide. Call *before* the tab is closed by
    /// a mouse action. No-op when the horizontal strip is not on screen.
    pub(crate) fn freeze_hbar_for_close(&mut self, tab_id: usize) {
        if self.vertical_tabs.visible || self.tree_tabs.visible {
            return;
        }
        let width = (|| {
            let (doc, _) = self.chrome_doc.as_ref()?;
            let (layout, _) = self.chrome_layout.as_ref()?;
            let strip = doc.find_by_id(lumen_chrome::ids::HBAR_TABS)?;
            let want = tab_id.to_string();
            let row = doc.get(strip).children.iter().copied().find(|&c| {
                let attrs = doc.get(c);
                attrs.get_attr("data-tab-id") == Some(want.as_str())
                    && attrs.get_attr("class").is_some_and(|cl| cl.split_whitespace().any(|t| t == "hbar-tab"))
            })?;
            lumen_layout::find_box_by_node(layout, row).map(|b| b.rect.width)
        })();
        if let Some(w) = width.filter(|w| *w > 0.0) {
            self.hbar_freeze = Some(tabs::strip_freeze::HbarFreeze::new(w, self.tab_strip.len(), self.viewport_width_css()));
        }
    }

    /// UX-TAB-CLOSE-FREEZE: drops a stale freeze (tab opened, window resized)
    /// before the chrome model is rebuilt.
    pub(crate) fn refresh_hbar_freeze(&mut self) {
        if let Some(mut freeze) = self.hbar_freeze {
            let keep = freeze.keep(self.tab_strip.len(), self.viewport_width_css());
            self.hbar_freeze = keep.then_some(freeze);
        }
    }

    /// UX-TAB-CLOSE-FREEZE: releases the freeze once the cursor leaves the
    /// strip's zone (strip rect + 60 px sideways, 40 px up/down).
    pub(crate) fn release_hbar_freeze_if_outside(&mut self, x_css: f32, y_css: f32) {
        if self.hbar_freeze.is_none() {
            return;
        }
        let inside = self
            .chrome_node_rect(lumen_chrome::ids::HBAR_TABS)
            .is_some_and(|r| tabs::strip_freeze::in_freeze_zone((r.x, r.y, r.width, r.height), x_css, y_css));
        if !inside {
            self.hbar_freeze = None;
            self.relayout_chrome_host();
            self.request_redraw();
        }
    }

    /// `true` when `nid` is the chrome element with DOM id `id`.
    pub(crate) fn chrome_node_id_is(&self, nid: NodeId, id: &str) -> Option<bool> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        Some(doc.find_by_id(id) == Some(nid))
    }

    /// CC-6: reads and parses a `data-tab-id`/`data-ws-id`-style integer
    /// attribute off `nid` in the live `chrome_doc` — the id `ChromeModel`
    /// (`crates/chrome/src/model.rs`) stamps on rebuilt tab rows/workspace
    /// buttons so a click can be resolved back to a `tab_strip`/
    /// `workspace_panel` entry. `None` off the flag, before the first chrome
    /// layout, or if the attribute is missing/unparsable.
    pub(crate) fn chrome_data_id(&self, nid: NodeId, attr: &str) -> Option<i64> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        doc.get(nid).get_attr(attr)?.parse().ok()
    }

    /// BUG-422: the string sibling of [`Self::chrome_data_id`] — reads a
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
    /// handlers — the same functions the legacy toolbar/tab-strip hit-
    /// testers call, so behavior (reload semantics, panel toggling, …)
    /// matches exactly. `SelectTab`/`CloseTab`/`SelectWorkspace`/`AddWorkspace`
    /// resolve the clicked row back to a real `tab_strip`/`workspace_panel`
    /// entry via the `data-tab-id`/`data-ws-id` attribute `ChromeModel`
    /// stamped on it (CC-6, `crates/chrome/src/model.rs`) — `nid` is the
    /// `data-action`-carrying node itself for these four. `SetSettingsSection`/
    /// `ShowView`/`SetSidebarTab` and friends (CC-10b) since grew the same
    /// pattern for the settings/history/bookmarks views and the right
    /// sidebar. A handful of actions remain permanent no-ops for reasons
    /// specific to each — see the comment on the final match arm below
    /// (BUG-426).
    ///
    /// DS-16: resets [`Self::anonymous_cookie_jar`] to a fresh in-memory jar
    /// — shared by both real sites that switch *into* the Anonymous profile
    /// (the legacy `#profileMenu` overlay's `MouseInput` handler and
    /// `ChromeAction::SetProfile` below, CC-18) so a switch through either
    /// one never carries a cookie over from a previous Anonymous run.
    #[allow(clippy::expect_used)] // унаследовано, docs/lint-policy.md §10
    pub(crate) fn reset_anonymous_cookie_jar(&mut self) {
        self.anonymous_cookie_jar =
            Arc::new(lumen_storage::CookieJar::open_in_memory().expect("anonymous_cookie_jar reset"));
    }

    /// COMPAT-PRIVACY-UNBREAK-LOCALE / -PROFILE: `Accept-Language`,
    /// `navigator.languages` and the time zone follow the active profile's
    /// identity mode — the OS list and zone in Personal/Work, `en-US` and UTC
    /// in Anonymous and Guest — from the next navigation (every page reads the
    /// process-global values when its JS context starts).
    /// COMPAT-PRIVACY-UNBREAK-NOISE: the canvas / audio noise secret follows the
    /// profile too. Call after the active profile changed and once at startup.
    pub(crate) fn sync_profile_locale(&self) {
        let crowd = self.active_profile_is_crowd();
        #[cfg(feature = "v8")]
        {
            let anonymous = self.active_profile_is_anonymous();
            crate::config::global().install_navigator_for(crowd);
            let id = self.profile_menu.active_entry().map(|e| e.id);
            crate::config::global().install_noise(anonymous, id);
        }
        #[cfg(not(feature = "v8"))]
        crate::config::global().install_accept_language(crowd);
    }

    /// UX-PERMISSIONS-3: Anonymous decides into a fresh in-memory store (as
    /// with its cookie jar), every other profile into `permissions.db`.
    /// Call after the active profile changed and once at startup.
    pub(crate) fn sync_permission_store(&mut self) {
        let store = if self.active_profile_is_anonymous() {
            lumen_storage::Permissions::open_in_memory().ok().map(Arc::new)
        } else {
            Some(Arc::clone(&self.permissions_disk_store))
        };
        if let Some(store) = store {
            // UX-USER-ACTIVATION: the JS gate asks this store whether the user
            // allowed popups / clipboard writes for the document's site.
            let gate_store = Arc::clone(&store);
            lumen_js::popup_policy::set_site_resolver(Some(Arc::new(move |origin, kind| {
                panels::permission_panel::site_allows(&gate_store, origin, kind)
            })));
            self.permission.set_store(store);
        }
    }

    /// UX-CHROME-PANELS: persists the accessibility draft, hides the panel and
    /// re-delivers media changes (prefers-reduced-motion / forced colors).
    pub(crate) fn close_a11y_panel(&mut self) {
        let _ = self.a11y_store.apply_snapshot(&self.a11y_panel.draft);
        self.a11y_panel.visible = false;
        self.deliver_a11y_media_changes();
        // Re-style with the (possibly toggled) forced-colors pref; no page
        // geometry is read afterwards, so this stays async-safe (M2.2b-6).
        self.relayout_chrome();
        self.relayout_chrome_host();
    }

    /// CC-10: which modal `nid` (a `close-modal`-carrying node, or one of its
    /// descendants) belongs to — `#certOverlay` and `#printOverlay` share the
    /// same `data-action` value, so this walks up the tree to disambiguate,
    /// mirroring [`Self::chrome_permission_kind_for_node`].
    pub(crate) fn chrome_modal_ancestor(&self, nid: NodeId) -> Option<ChromeModalKind> {
        let (doc, _) = self.chrome_doc.as_ref()?;
        let cert_overlay = doc.find_by_id(lumen_chrome::ids::CERT_OVERLAY);
        let print_overlay = doc.find_by_id(lumen_chrome::ids::PRINT_OVERLAY);
        let shortcuts_overlay = doc.find_by_id(lumen_chrome::ids::SHORTCUTS_OVERLAY);
        let access_overlay = doc.find_by_id(lumen_chrome::ids::ACCESS_OVERLAY);
        let mut cur = Some(nid);
        while let Some(id) = cur {
            if Some(id) == cert_overlay {
                return Some(ChromeModalKind::Cert);
            }
            if Some(id) == print_overlay {
                return Some(ChromeModalKind::Print);
            }
            if Some(id) == shortcuts_overlay {
                return Some(ChromeModalKind::Shortcuts);
            }
            if Some(id) == access_overlay {
                return Some(ChromeModalKind::Access);
            }
            cur = doc.get(id).parent;
        }
        None
    }

    /// CC-9: walks up from `nid` (a `.perm-btn` inside `#permPopover`) to its
    /// `.perm-row` ancestor, then resolves that row's position among
    /// `#permPopover`'s `.perm-row` children to a [`PermissionKind`] —
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

    /// UX-PERMISSIONS-2: hand the user's answer to the page's pending
    /// `Notification.requestPermission()` promises.
    pub(crate) fn settle_page_permission(
        &self,
        kind: panels::permission_panel::PermissionKind,
        state: panels::permission_panel::PermissionState,
    ) {
        use panels::permission_panel::PermissionState;
        let name = match state {
            PermissionState::Allow => "granted",
            PermissionState::Deny => "denied",
            PermissionState::Ask => "default",
        };
        let script = lumen_js::notifications_bindings::settle_script(kind.page_name(), name);
        let _ = route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| j.eval_js(&script));
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
    /// AX nodes (DS-17) — `lumen_a11y::chrome::chrome_nodes` turns this into
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
            ChromeButton { name: format!("Профиль: {profile_name}"), pressed: None },
            ChromeButton { name: "Назад".to_owned(), pressed: None },
            ChromeButton { name: "Вперёд".to_owned(), pressed: None },
            ChromeButton { name: "Обновить".to_owned(), pressed: None },
            ChromeButton { name: "Найти на странице".to_owned(), pressed: Some(self.find.is_open()) },
            ChromeButton { name: "Веб-сайдбар".to_owned(), pressed: Some(self.sidebar.visible) },
            ChromeButton { name: "ИИ-сайдбар".to_owned(), pressed: Some(self.ai_panel.visible) },
            ChromeButton { name: "Загрузки".to_owned(), pressed: Some(self.downloads.visible) },
            ChromeButton { name: "DevTools".to_owned(), pressed: Some(self.devtools_console.visible) },
            ChromeButton { name: "Настройки".to_owned(), pressed: Some(self.settings_panel.visible) },
        ];

        let omnibox_value = if self.address_bar.is_open() {
            self.address_bar.input().to_owned()
        } else {
            self.current_display_url().to_owned()
        };

        ChromeSnapshot { tabs, buttons, omnibox_value }
    }

    /// Chrome AX siblings for [`Self::update_platform_ax_tree`]/
    /// `automation_a11y_tree` — CC-13: these come from the engine-rendered
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
    /// the OS bridge. Called after every full page load and tab switch — the
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

/// BUG-341 S17 — flatten `bind_model_tracked`'s per-node report into the
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

/// `#contentArea` children that [`take_content_area`] keeps in the chrome tree
/// (hit-testing reads them there) and that [`Lumen::relayout_chrome_host`]
/// paints separately, unclipped — BUG-1605.
pub(crate) const SALVAGED_POPOVER_IDS: [&str; 17] = [
    lumen_chrome::ids::FIND_BAR,
    lumen_chrome::ids::DOWNLOADS_PANEL,
    lumen_chrome::ids::CP_OVERLAY,
    lumen_chrome::ids::CERT_OVERLAY,
    lumen_chrome::ids::SHORTCUTS_OVERLAY,
    lumen_chrome::ids::ACCESS_OVERLAY,
    lumen_chrome::ids::PRINT_OVERLAY,
    lumen_chrome::ids::DEVTOOLS,
    lumen_chrome::ids::NETWORK_PANEL,
    lumen_chrome::ids::INSP_PANEL,
    lumen_chrome::ids::READ_LATER_PANEL,
    lumen_chrome::ids::PRIVACY_PANEL,
    lumen_chrome::ids::NOTE_OVERLAY,
    lumen_chrome::ids::TAB_CTX_MENU,
    lumen_chrome::ids::PIP_CARD,
    lumen_chrome::ids::PAGE_CTX_MENU,
    lumen_chrome::ids::WORKSPACE_BAR,
];

/// CC-4/CC-9: removes `#contentArea`'s [`LayoutBox`] from `lb`'s subtree
/// (depth-first, first match) — not just its children, but its own box (and
/// background paint command) too, so the real page painted separately at
/// that rect is never covered by it. Never matches `lb` itself, only
/// descendants — `#contentArea` is never the chrome document's root box.
///
/// CC-9: two of `#contentArea`'s own children — `#findBar`, `#downloadsPanel`
/// — are real popovers this pass *does* want painted (they sit outside the
/// pruned rect via `position:absolute`, CSS Positioned Layout L3 §9.10, so
/// splicing them elsewhere in the tree does not change which stacking
/// context they join: `#contentArea` itself creates none). `salvage_ids`
/// lists which descendant node ids to keep; they're spliced back into `lb`
/// at the exact slot `#contentArea` occupied, preserving both their absolute
/// paint rects (already resolved by the out-of-flow layout pass) and their
/// tree-order position relative to `#contentArea`'s former siblings.
///
/// BUG-341 S22: the pruning is **reversible**. Every removal is recorded in the
/// returned [`ContentAreaDetachment`], and [`restore_content_area`] puts the
/// tree back exactly as it was — which is what lets the next pass take the
/// live tree as its `prev` basis instead of the pipeline copying a pristine
/// one aside on every frame (that copy was the largest single item left in an
/// incremental chrome cycle; see the S22 census in `bugs/BUG-341-FIXED.md`).
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
/// that basis — on an interaction cycle `#contentArea`'s parent is clean, so a
/// basis missing `#contentArea` produces a document missing `#contentArea`,
/// and the next cycle inherits that tree in turn. It is not a slow frame, it
/// is 155 boxes where there should be 318, permanently. Measured, and gated:
/// `bug341_s22_a_restored_basis_carries_the_whole_document_forward`.
pub(crate) struct ContentAreaDetachment {
    /// Child-index path from the tree root down to the box that held
    /// `#contentArea` (empty when the root itself held it).
    holder_path: Vec<usize>,
    /// Index `#contentArea` occupied among that holder's children — also the
    /// slot the salvaged popovers were spliced into.
    slot: usize,
    /// `#contentArea`'s own box, with the salvaged popovers already lifted out
    /// of its subtree.
    removed: LayoutBox,
    /// For each salvaged popover, in removal order, the child-index path
    /// inside [`Self::removed`] it was removed from — the last element is the
    /// index within that box's children. Restoring walks these in **reverse**,
    /// because each path was recorded against the tree state of its own
    /// removal.
    pub(crate) salvage_paths: Vec<Vec<usize>>,
}

/// BUG-341 S22: inverse of [`take_content_area`] — re-inserts the salvaged
/// popovers into `#contentArea`'s subtree and `#contentArea` back into its
/// former slot, reproducing the pre-pruning tree box for box.
///
/// Returns `false` if any recorded path no longer addresses a box (only
/// possible if something mutated the tree between the two calls, which nothing
/// does today — `chrome_layout` is read-only until the next pass replaces it).
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
/// range. Shared with `chrome_float::restore_floating_panel`, which undoes the
/// same kind of recorded detachment.
pub(crate) fn follow_box_path_mut<'a>(b: &'a mut LayoutBox, path: &[usize]) -> Option<&'a mut LayoutBox> {
    let mut cur = b;
    for &i in path {
        cur = cur.children.get_mut(i)?;
    }
    Some(cur)
}

// BUG-1059: `FloatingPanelDetachment`/`take_floating_panel`/
// `restore_floating_panel` used to live here; CC-18 срез 3 moved them to
// `chrome_float.rs`, next to the drag placement that reads the same boxes.

/// Depth-first: removes every descendant of `lb` whose element id is in
/// `salvage_ids`, appending it to `out` in tree order together with the
/// child-index path (relative to the box this recursion started at) it came
/// from. Used by [`take_content_area`] to rescue specific popovers out of
/// `#contentArea` before the rest of its subtree is discarded — and by
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

/// BUG-405 срез 50 (п.85 "вариант (б)"): the already strip-clipped and
/// caret-painted chrome overlay segment ([`Lumen::relayout_chrome_host`]'s
/// doc comment on `chrome_layout` — "painted at the front of `overlay_buf`
/// every frame") — cached across `RedrawRequested` calls instead of rebuilt
/// (up to 4 `chrome_dl.extend_from_slice` copies, one per non-degenerate
/// clip strip around the page host, see the `chrome_mix` diagnostic) when
/// nothing that shapes it changed since the frame this cache was built for.
///
/// Validity is a plain field-equality check against the four inputs the
/// build actually reads (`generation`, `host_rect`, `viewport`, `caret`) —
/// no content hashing needed, because [`Lumen::chrome_layout_generation`] is
/// a safe SUPERSET of "chrome_dl's bytes changed": [`Lumen::relayout_chrome_host`]
/// bumps it on every pass, whether or not the pass actually produced
/// different bytes (срезы 48/49 measured `predict_same`/`actual_same`
/// agreement on a steady-state repeat, but this cache does not need that
/// guarantee — a spurious rebuild on an unrelated bump costs one frame, a
/// false HIT would cost a wrong pixel forever). Never consulted while a
/// chrome CSS transition/animation is live (the caller's `chrome_dl_anim` is
/// `Some`) — an animated frame's content differs every tick by construction.
pub(crate) struct ChromeOverlayFrameCache {
    /// [`Lumen::chrome_layout_generation`] the cache was built under.
    pub(crate) generation: u64,
    /// `Lumen::chrome_page_host_rect` the strips were cut around.
    pub(crate) host_rect: Rect,
    /// `(window width, window height)` in CSS px — the strip geometry's other
    /// input besides `host_rect`.
    pub(crate) viewport: (f32, f32),
    /// The hand-painted omnibox caret's `(rect, color)`, or `None` while
    /// nothing should be drawn — mirrors the condition the caller gates the
    /// caret `FillRect` on.
    pub(crate) caret: Option<(Rect, lumen_layout::Color)>,
    /// How many non-degenerate strips `chrome_dl` was copied into when this
    /// was built — kept only so a cache HIT can still print the `chrome_mix`
    /// diagnostic line (`build: chrome … | chrome WxH=N cmds …`).
    pub(crate) strips_used: usize,
    /// The assembled segment itself — clip strips around `chrome_dl` plus the
    /// caret fill, ready to be prepended to `overlay_buf` with one clone.
    pub(crate) framed: lumen_paint::DisplayList,
    /// [`lumen_paint::display_list::fold_overlay`] of `framed`, computed once
    /// when this cache was built (BUG-405 срез 57, п.85). On a HIT this lets
    /// the caller reuse the digest of every reused command instead of
    /// rehashing it — `fold_overlay` reading the whole `overlay_buf` every
    /// frame was measured (срезы 55/56) as ~100% of `fold_overlay`'s cost on
    /// a typical scroll frame, almost entirely spent on this segment.
    pub(crate) digests: Vec<u64>,
}

/// BUG-405 срез 50: reuse-or-build decision behind [`ChromeOverlayFrameCache`],
/// factored out as a free function (no `&Lumen`/renderer needed) so it is
/// unit-testable directly — срез 49 found building a whole `Lumen` for one
/// diagnostic test disproportionate, and the same reasoning applies here.
///
/// Returns the assembled segment, the strip count (for the `chrome_mix`
/// diagnostic), its digest (BUG-405 срез 57 — always valid for the returned
/// `DisplayList`, cloned from the cache on a HIT, freshly folded on a
/// build), and, when a NEW cache should be remembered, `Some` of it. `None`
/// in that fourth slot means "leave `Lumen::chrome_overlay_frame_cache`
/// exactly as it is" — true both on a HIT (the existing cache is still the
/// right one to keep) and when `cache_enabled` is `false` (nothing to
/// remember, and the caller does not clear a stale entry either — the next
/// eligible frame will simply overwrite it).
#[allow(clippy::too_many_arguments)] // BUG-405 срез 50: same shape as lumen-paint's Renderer::compose_page
pub(crate) fn chrome_overlay_segment(
    chrome_dl: &[lumen_paint::DisplayCommand],
    host: Rect,
    win_w: f32,
    win_h: f32,
    caret_plan: Option<(Rect, lumen_layout::Color)>,
    generation: u64,
    cache_enabled: bool,
    cache: Option<&ChromeOverlayFrameCache>,
) -> (lumen_paint::DisplayList, usize, Vec<u64>, Option<ChromeOverlayFrameCache>) {
    if cache_enabled && let Some(c) = cache.filter(|c| {
        c.generation == generation
            && c.host_rect == host
            && c.viewport == (win_w, win_h)
            && c.caret == caret_plan
    }) {
        return (c.framed.clone(), c.strips_used, c.digests.clone(), None);
    }
    let (framed, strips_used) = build_chrome_overlay_strips(chrome_dl, host, win_w, win_h, caret_plan);
    let digests = lumen_paint::display_list::fold_overlay(&framed);
    let new_cache = cache_enabled.then(|| ChromeOverlayFrameCache {
        generation,
        host_rect: host,
        viewport: (win_w, win_h),
        caret: caret_plan,
        strips_used,
        framed: framed.clone(),
        digests: digests.clone(),
    });
    (framed, strips_used, digests, new_cache)
}

/// BUG-405 срез 50: the actual clip-strip + caret build, unchanged from the
/// code [`chrome_overlay_segment`] replaced inline in `RedrawRequested`.
fn build_chrome_overlay_strips(
    chrome_dl: &[lumen_paint::DisplayCommand],
    host: Rect,
    win_w: f32,
    win_h: f32,
    caret_plan: Option<(Rect, lumen_layout::Color)>,
) -> (lumen_paint::DisplayList, usize) {
    let strips = [
        Rect { x: 0.0, y: 0.0, width: win_w, height: host.y },
        Rect {
            x: 0.0,
            y: host.y + host.height,
            width: win_w,
            height: (win_h - (host.y + host.height)).max(0.0),
        },
        Rect { x: 0.0, y: host.y, width: host.x, height: host.height },
        Rect {
            x: host.x + host.width,
            y: host.y,
            width: (win_w - (host.x + host.width)).max(0.0),
            height: host.height,
        },
    ];
    let mut framed = lumen_paint::DisplayList::new();
    let mut strips_used = 0_usize;
    for strip in strips {
        if strip.width <= 0.0 || strip.height <= 0.0 {
            continue;
        }
        strips_used += 1;
        framed.push(lumen_paint::DisplayCommand::PushClipRect { rect: strip });
        framed.extend_from_slice(chrome_dl);
        framed.push(lumen_paint::DisplayCommand::PopClip);
    }
    if let Some((rect, color)) = caret_plan {
        framed.push(lumen_paint::DisplayCommand::FillRect { rect, color });
    }
    (framed, strips_used)
}

/// `true` if [`ChromeOverlayFrameCache`] reuse is disabled
/// (`LUMEN_NO_CHROME_OVERLAY_CACHE=1`) — A/B lever (`docs/perf-method.md`),
/// same shape as `frame_pacing::fast_scroll_degrade_disabled`.
pub(crate) fn chrome_overlay_cache_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_CHROME_OVERLAY_CACHE").is_ok_and(|v| v == "1"))
}

/// `true` if reusing the chrome segment's digests
/// ([`RenderBackend::set_overlay_digest_reuse`](lumen_paint::backend::RenderBackend::set_overlay_digest_reuse))
/// is disabled (`LUMEN_NO_CHROME_DIGEST_REUSE=1`) — BUG-405 срез 57 A/B
/// lever (`docs/perf-method.md`), same shape as [`chrome_overlay_cache_disabled`].
/// Independent of it: the overlay cache decides whether `chrome_dl` is
/// re-copied into strips, this lever decides whether the RESULT's digest is
/// rehashed by `fold_overlay` regardless. Isolates exactly the mechanism
/// срез 57 measured (`fold_overlay` vs `fold_overlay_with_reuse` on the same
/// overlay) — `chrome_overlay_segment` still folds `framed` on every miss
/// either way (so [`ChromeOverlayFrameCache`] always has a digest ready for
/// the NEXT frame's HIT, regardless of this lever), so disabling reuse here
/// does not reproduce the exact pre-срез-57 instruction count, only the
/// pre-срез-57 cost of `fold_overlay` itself — the one number this lever
/// exists to isolate.
pub(crate) fn chrome_overlay_digest_reuse_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var("LUMEN_NO_CHROME_DIGEST_REUSE").is_ok_and(|v| v == "1"))
}

/// CC-10: which modal `Lumen::chrome_modal_ancestor` resolved a `CloseModal`
/// click to — `#certOverlay` and `#printOverlay` share the same
/// `data-action="close-modal"` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChromeModalKind {
    Cert,
    Print,
    Shortcuts,
    Access,
}
