//! What a left-button click at a viewport position does to the page.
//!
//! `handle_click_at` is the single entry point for both a real winit
//! `MouseInput::Pressed` and an injected `InputCommand::Click`, so the two
//! paths share identical hit-testing and dispatch. It converts to page
//! coordinates (`page_point`), hit-tests the layout tree, and from there
//! reaches link activation, form controls, the spell menu, focus changes and —
//! for a submit control — `run_form_submission`.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Behaviour and the
//! method body are unchanged; only the module path and the visibility of
//! `handle_click_at` differ.

use crate::*;

impl Lumen {
    #[allow(clippy::unwrap_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn handle_click_at(&mut self, x_css: f32, y_css: f32) {
        // Dismiss validation tooltip on any non-scrollbar click.
        self.validation_tooltip = None;
        let scroll_y = self.scroll_y;

        // DevTools inspector: a click pins the box under the cursor and shows
        // its computed style, suppressing normal navigation / JS dispatch.
        if self.dom_inspector.visible {
            let win_w_css = self.viewport_width_css();
            // Click inside the right-docked panel в†’ UI interaction (tab switch).
            if self.dom_inspector.is_panel_click(x_css, win_w_css) {
                if self.dom_inspector.click_tab_at(
                    x_css, y_css, win_w_css,
                    toolbar::CHROME_H,
                ) {
                    self.request_redraw();
                }
                return;
            }
            // Click on the page в†’ pin the box under cursor.
            let (page_x, page_y) = self.page_point(x_css, y_css);
            if let Some(hit) = self
                .layout_box
                .as_ref()
                .and_then(|lb| hit_test(Point::new(page_x, page_y), lb))
            {
                let node = hit.node;
                let label = self
                    .layout_source
                    .as_ref()
                    .map(|src| {
                        devtools::inspector::element_label(&src.document.lock().unwrap(), node)
                    })
                    .unwrap_or_else(|| format!("NodeId({})", node.index()));
                let props = self
                    .layout_box
                    .as_ref()
                    .and_then(|lb| devtools::inspector::find_box(lb, node))
                    .map(devtools::inspector::computed_style_map)
                    .unwrap_or_default();
                let computed_props = self
                    .layout_box
                    .as_ref()
                    .and_then(|lb| devtools::inspector::find_box(lb, node))
                    .map(|lb| {
                        let mut entries: Vec<(String, String)> =
                            computed_style_to_map(&lb.style).into_iter().collect();
                        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                        entries
                    })
                    .unwrap_or_default();
                let styles_rules: Vec<(String, Vec<(String, String)>)> = self
                    .layout_source
                    .as_ref()
                    .map(|src| {
                        let doc = src.document.lock().unwrap();
                        lumen_layout::matched_rules_for_node(&doc, node, &src.stylesheet)
                            .into_iter()
                            .map(|r| (r.selector, r.declarations))
                            .collect()
                    })
                    .unwrap_or_default();
                self.dom_inspector.select(node, label, props, styles_rules, computed_props);
                self.request_redraw();
            }
            return;
        }

        // в”Ђв”Ђ Color picker swatch hit в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Check if click lands on an open color picker swatch.
        // Compute swatch result inside a scoped borrow, then act.
        let picker_swatch_result: Option<(NodeId, [u8; 3])> = {
            let picker_node = self.color_picker_node;
            picker_node.and_then(|pn| {
                let anchor = forms::find_box_rect(
                    self.layout_box.as_ref()?,
                    pn,
                )?;
                let color = forms::hit_color_swatch(
                    anchor, scroll_y, x_css, y_css,
                )?;
                Some((pn, color))
            })
        };
        if let Some((pn, color)) = picker_swatch_result {
            self.color_picker_node = None;
            let css_color = forms::swatch_to_css_color(color);
            if let Some(src) = self.layout_source.as_mut() {
                forms::set_value(&mut src.document.lock().unwrap(), pn, &css_color);
            }
            self.form_state.entry(pn).or_default().value = css_color;
            // ADR-016 M2.2c-3: value already in the document; no post-read в†’ off-thread.
            self.relayout_form();
            return;
        }
        // Any click outside the picker closes it.
        self.color_picker_node = None;

        // в”Ђв”Ђ Date picker hit в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        let date_hit: Option<(NodeId, forms::DatePickerHit)> = {
            let dp_node = self.date_picker_node;
            dp_node.and_then(|dn| {
                let anchor = forms::find_box_rect(self.layout_box.as_ref()?, dn)?;
                let vp_w2 = self.viewport_width_css();
                let hit = forms::hit_date_picker(anchor, scroll_y, vp_w2, self.date_picker_year, self.date_picker_month, x_css, y_css);
                Some((dn, hit))
            })
        };
        if let Some((dn, hit)) = date_hit {
            match hit {
                forms::DatePickerHit::Prev => {
                    let (ny, nm) = forms::advance_month(self.date_picker_year, self.date_picker_month, -1);
                    self.date_picker_year = ny;
                    self.date_picker_month = nm;
                    self.request_redraw();
                    return;
                }
                forms::DatePickerHit::Next => {
                    let (ny, nm) = forms::advance_month(self.date_picker_year, self.date_picker_month, 1);
                    self.date_picker_year = ny;
                    self.date_picker_month = nm;
                    self.request_redraw();
                    return;
                }
                forms::DatePickerHit::Day(day) => {
                    self.date_picker_node = None;
                    let date_str = forms::format_date_value(self.date_picker_year, self.date_picker_month, day);
                    if let Some(src) = self.layout_source.as_mut() {
                        forms::set_value(&mut src.document.lock().unwrap(), dn, &date_str);
                    }
                    self.form_state.entry(dn).or_default().value = date_str;
                    // ADR-016 M2.2c-3: async-safe form mutation вЂ” see color picker.
                    self.relayout_form();
                    return;
                }
                forms::DatePickerHit::None => {}
            }
        }
        // Any click outside the date picker closes it.
        self.date_picker_node = None;

        // в”Ђв”Ђ Select dropdown option hit в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Check if click lands on an open <select> dropdown.
        let select_hit: Option<(NodeId, usize)> = {
            let sel_node = self.select_dropdown_node;
            sel_node.and_then(|sn| {
                let anchor = forms::find_box_rect(self.layout_box.as_ref()?, sn)?;
                let opts_count = self.layout_source.as_ref()
                    .map(|src| forms::collect_select_options(&src.document.lock().unwrap(), sn).len())
                    .unwrap_or(0);
                let vp_h = self.viewport_height_css();
                let vp_w2 = self.viewport_width_css();
                let idx = forms::hit_select_option(anchor, opts_count, scroll_y, vp_w2, vp_h, x_css, y_css)?;
                Some((sn, idx))
            })
        };
        if let Some((sn, idx)) = select_hit {
            self.select_dropdown_node = None;
            if let Some(src) = self.layout_source.as_mut() {
                let mut doc = src.document.lock().unwrap();
                let opts = forms::collect_select_options(&doc, sn);
                if !opts.get(idx).is_some_and(|o| o.disabled) {
                    forms::apply_select_choice(&mut doc, &opts, idx);
                    // Update form_state value so form submission includes the chosen value.
                    if let Some(chosen) = opts.get(idx) {
                        self.form_state.entry(sn).or_default().value = chosen.value.clone();
                    }
                    drop(doc);
                    // ADR-016 M2.2c-3: async-safe <select> choice вЂ” see color picker.
                    self.relayout_form();
                }
            }
            return;
        }
        // Any click outside the dropdown closes it.
        self.select_dropdown_node = None;

        // в”Ђв”Ђ Form control + link click в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
        // Single hit test shared by form dispatch and link navigation.
        //
        // BUG-437: the conversion is [`Self::page_point`], the same one the
        // render-time page transform (`page_offset()`) and the DevTools
        // inspector already use. It used to be open-coded here as
        // `left_dock() width` / `toolbar::CHROME_H`, which stopped matching
        // where the page is actually painted once engine chrome became the
        // default (CC-14): `#contentArea` starts at y=68, not at CHROME_H=72,
        // so every click hit-tested 4 px below the pixel the user aimed at and
        // controls within 4 px of an edge resolved to the wrong node.
        //
        // BUG-480 срез 16: тот же вызов отвечает и на вопрос «не внутри ли
        // фрейма эта точка» — hit-тест страницы по ней делается один раз.
        let (page_x, page_y) = self.page_point(x_css, y_css);
        let target = self.pointer_target(x_css, y_css);
        let frame_target = target.frame;
        let hit_result = target.page;

        // Debug click log вЂ” Р°РєС‚РёРІРёСЂСѓРµС‚СЃСЏ С„Р»Р°РіРѕРј --click-log РёР»Рё LUMEN_CLICK_LOG=1.
        // For click log: report both the hit box node (<p>) and the inline source_node
        // (<a> text node) so the log shows what find_link_href actually searches from.
        let click_log_hit: Option<(u32, String, String, String)> =
            if click_log::is_enabled() {
                hit_result.as_ref().and_then(|r| {
                    self.layout_source.as_ref().map(|src| {
                        let doc = src.document.lock().unwrap();
                        // Use source_node for tag/class info вЂ” it reveals the inline element.
                        let effective_id = r.source_node;
                        let node = doc.get(effective_id);
                        let (tag, id_attr, class_attr) =
                            if let NodeData::Element { name, attrs } = &node.data {
                                let id = attrs.iter()
                                    .find(|a| a.name.local == "id")
                                    .map(|a| a.value.as_str())
                                    .unwrap_or("");
                                let cls = attrs.iter()
                                    .find(|a| a.name.local == "class")
                                    .map(|a| a.value.as_str())
                                    .unwrap_or("");
                                (name.local.to_string(), id.to_owned(), cls.to_owned())
                            } else if let NodeData::Text(t) = &node.data {
                                // Show which text we clicked and note the parent element.
                                let parent_tag = node.parent
                                    .map(|pid| {
                                        let pn = doc.get(pid);
                                        if let NodeData::Element { name, .. } = &pn.data {
                                            format!("<{}>", name.local)
                                        } else {
                                            "?".to_owned()
                                        }
                                    })
                                    .unwrap_or_default();
                                let preview: String = t.chars().take(30).collect();
                                (format!("#text in {parent_tag}"), String::new(), format!("\"{preview}\""))
                            } else {
                                ("#other".to_owned(), String::new(), String::new())
                            };
                        (effective_id.index() as u32, tag, id_attr, class_attr)
                    })
                })
            } else {
                None
            };

        // Track focused node for TypeText injection and CSS :focus matching.
        let new_focused = hit_result.as_ref().map(|r| r.node);
        let focus_changed = new_focused != self.focused_node;
        self.focused_node = new_focused;
        // Trigger relayout if :focus state changed so :focus / :focus-within rules update.
        if focus_changed {
            // ADR-016 M2.2b-7: `focused_node` is set synchronously above, so
            // `:focus`/`:focus-within` re-evaluates on any later relayout. The
            // subsequent JS click dispatch reads the pre-`:focus` `hit_result`
            // (the geometry the user clicked on вЂ” correct), and any DOM mutation
            // from those handlers takes its own generation-guarded relayout, so
            // this pure restyle has no synchronous geometry read and goes off-thread.
            self.relayout_chrome();
            // Notify platform accessibility bridge so screen readers can track focus.
            self.platform_bridge.focused_node_changed(new_focused);
            // Keep JS _lumen_last_focused_nid in sync so showModal() can save/restore it.
            // ADR-016 M2.2c-2d (16): fire-and-forget void `notify_focus_changed` С‡РµСЂРµР·
            // `route_task_js`. `focus_idx` (owned `Option<u32>`) РІС‹С‡РёСЃР»СЏРµС‚СЃСЏ РґРѕ
            // РјР°СЂС€СЂСѓС‚РёР·Р°С†РёРё, Р·Р°РјС‹РєР°РЅРёРµ `Send + 'static`. РџРѕРґ С„Р»Р°РіРѕРј
            // (`LUMEN_ENGINE_THREAD=1`) СѓС…РѕРґРёС‚ off-UI-thread РѕРґРЅРёРј `task`; Р±РµР· С„Р»Р°РіР°
            // (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ, **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ**
            // РїСЂРµР¶РЅРµРјСѓ `js.notify_focus_changed`.
            let focus_idx = new_focused.map(|n| n.index() as u32);
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                js.notify_focus_changed(focus_idx);
            });
        }
        // BUG-480 срез 16: точка внутри содержимого фрейма адресует под-документ,
        // и на этом путь родителя кончается. Ранний возврат — не оптимизация:
        // событие внутри вложенного browsing context родительскому документу
        // ВООБЩЕ не принадлежит (DOM §2.9 строит путь в одном дереве), а до
        // среза родитель получал `click` на самом `<iframe>` — измерено пробой
        // `verify_frame_hit_test.py`. По той же причине пропускаются форма и
        // ссылка: единственный узел страницы под этой точкой — сам `<iframe>`.
        //
        // Фокус выше уже переведён на host-элемент: с точки зрения родителя
        // клик внутрь фрейма фокусирует именно контейнер.
        if let Some(target) = frame_target {
            if click_log::is_enabled() {
                let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                    node_id: *nid, tag, id_attr: id, class_attr: cls,
                });
                click_log::log_click(&click_log::ClickInfo {
                    win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                    hit: hit_ref,
                    outcome: click_log::ClickOutcome::IntoFrame {
                        frame: target.frame,
                        node: target.hit.as_ref().map(|h| h.node.index() as u32),
                        x: target.client.x,
                        y: target.client.y,
                    },
                });
            }
            if let Some(hit) = target.hit.as_ref() {
                #[cfg(feature = "v8")]
                self.frame_mouse_event(
                    target.frame,
                    hit.node.index() as u32,
                    "click",
                    (target.client.x, target.client.y),
                    (0, 1),
                );
                // BUG-480 срез 16 доставлял только СОБЫТИЕ, а собственное
                // поведение элемента (флажок, `<summary>`, ползунок) для
                // ребёнка не исполнял никто. Порядок «сначала dispatch, потом
                // переключение» — тот же, что ниже у страницы: обработчик
                // обязан видеть состояние ДО активации.
                //
                // BUG-480 срез 19: ссылка разбирается ровно там же, где у
                // страницы, — после формы и только если форма клик не забрала
                // (её ветка `FormClickAction::Nothing`). Поиск `<a>` идёт от
                // `source_node`, а не от бокса: у страницы по той же причине —
                // клик попадает в текстовый узел внутри инлайн-элемента.
                if !self.frame_form_click(target.frame, hit.node, target.client) {
                    self.frame_link_click(target.frame, hit.source_node);
                }
            }
            return;
        }

        // Dispatch JS click event (bubbles from hit node to document).
        // Passes viewport coordinates and modifier key state so
        // handlers can read event.clientX/clientY/ctrlKey/etc.
        if let Some(result) = hit_result.as_ref() {
            let mod_flags: u8 =
                (self.modifiers.control_key() as u8)
                | ((self.modifiers.shift_key()  as u8) << 1)
                | ((self.modifiers.alt_key()    as u8) << 2)
                | ((self.modifiers.super_key()  as u8) << 3);
            let script = format!(
                "_lumen_dispatch_mouse_event({}, 'click', {}, {}, 0, 1, {})",
                result.node.index(),
                x_css as i32,
                y_css as i32,
                mod_flags,
            );
            // ADR-016 M2.2c-2d (10): read-after-eval click dispatch вЂ” СЃР°Рј
            // `_lumen_dispatch_mouse_event('click', вЂ¦)` СѓС…РѕРґРёС‚ fire-and-forget С‡РµСЂРµР·
            // `route_eval_js`, Р° РїРѕСЃР»РµРґСѓСЋС‰РёР№ `take_navigate_request` (РЅР°РІРёРіР°С†РёСЏ, С‡С‚Рѕ
            // handler РјРѕРі РїРѕСЃС‚Р°РІРёС‚СЊ) вЂ” С‡РµСЂРµР· `route_query_js`. РџРѕРґ С„Р»Р°РіРѕРј
            // (`LUMEN_ENGINE_THREAD=1`) Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ `query` РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ **РїРѕСЃР»Рµ**
            // РѕС‚РїСЂР°РІР»РµРЅРЅРѕРіРѕ `task`, РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°СЏ read-after-eval РїРѕСЂСЏРґРѕРє; Р±РµР· С„Р»Р°РіР°
            // (РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) вЂ” РїСЂРµР¶РЅРёРµ СЃРёРЅС…СЂРѕРЅРЅС‹Рµ РІС‹Р·РѕРІС‹ РїРѕ UI-С…СЌРЅРґР»Сѓ, Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ
            // (`js_ctx == None` в†’ `None` в†’ РЅР°РІРёРіР°С†РёСЏ РЅРµ СЃС‚Р°РІРёС‚СЃСЏ, РєР°Рє РїСЂРµР¶РЅСЏСЏ
            // РІРµС‚РєР° `Some(ctx)` РЅРµ СЃРјР°С‚С‡РёР»Р°СЃСЊ).
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
            if let Some(Some(nav)) = route_query_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                |j| j.take_navigate_request(),
            ) {
                self.pending_js_navigate = Some(nav);
            }
        }
        let form_action: forms::FormClickAction =
            if let (Some(result), Some(src)) =
                (hit_result.as_ref(), self.layout_source.as_ref())
            {
                forms::classify_click(&src.document.lock().unwrap(), result.node)
            } else {
                forms::FormClickAction::Nothing
            };

        // Log form actions (non-link outcomes).
        if click_log::is_enabled() {
            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                node_id: *nid, tag, id_attr: id, class_attr: cls,
            });
            match &form_action {
                forms::FormClickAction::Nothing => {} // logged in the Nothing branch below
                forms::FormClickAction::ToggleCheckbox(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("ToggleCheckbox"),
                    });
                }
                forms::FormClickAction::ToggleRadio { .. } => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("ToggleRadio"),
                    });
                }
                forms::FormClickAction::OpenColorPicker(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenColorPicker"),
                    });
                }
                forms::FormClickAction::OpenDatePicker(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenDatePicker"),
                    });
                }
                forms::FormClickAction::OpenSelectDropdown(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenSelectDropdown"),
                    });
                }
                forms::FormClickAction::OpenFilePicker(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("OpenFilePicker"),
                    });
                }
                forms::FormClickAction::SubmitForm(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("SubmitForm"),
                    });
                }
                forms::FormClickAction::ToggleDetails(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("ToggleDetails"),
                    });
                }
                forms::FormClickAction::SlideRange(_) => {
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome: click_log::ClickOutcome::FormAction("SlideRange"),
                    });
                }
            }
        }

        match form_action {
            forms::FormClickAction::ToggleCheckbox(id) => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), id);
                }
                // ADR-016 M2.2c-3: the `checked` flip is already in the shared
                // document; no geometry is read after в†’ route the reflow off-thread.
                self.relayout_form();
            }
            forms::FormClickAction::ToggleRadio {
                clicked,
                _group_name: _,
            } => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), clicked);
                }
                // ADR-016 M2.2c-3: async-safe form mutation вЂ” see ToggleCheckbox.
                self.relayout_form();
            }
            forms::FormClickAction::OpenColorPicker(id) => {
                self.color_picker_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenDatePicker(id) => {
                let (y, m) = self.layout_source.as_ref()
                    .and_then(|src| {
                        let doc = src.document.lock().ok()?;
                        let val = doc.control_value(id).into_owned();
                        forms::parse_date_value(&val).map(|(y, m, _)| (y, m))
                    })
                    .unwrap_or_else(forms::today_year_month);
                self.date_picker_node = Some(id);
                self.date_picker_year = y;
                self.date_picker_month = m;
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenSelectDropdown(id) => {
                self.select_dropdown_node = Some(id);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            forms::FormClickAction::OpenFilePicker(id) => {
                self.open_file_picker(id);
            }
            forms::FormClickAction::ToggleDetails(id) => {
                // BUG-851: the flip is the shell's alone. The JS click this
                // method already dispatched used to reach a `click` listener on
                // `document` that flipped `open` a second time, so a real mouse
                // click on a `<summary>` left `<details>` exactly as it found it
                // вЂ” and fired two `toggle` events about the change that did not
                // happen. That listener is gone; JS is only *told* what changed.
                let was_open = self.layout_source.as_ref().is_some_and(|src| {
                    src.document
                        .lock()
                        .is_ok_and(|doc| doc.get(id).get_attr("open").is_some())
                });
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_details_open(&mut src.document.lock().unwrap(), id);
                }
                // HTML LS В§4.11.1 attribute change steps for `open` вЂ” the queued
                // `toggle` event and the exclusive-accordion pass. Routing them
                // through the shim instead of dispatching a bare `Event('toggle')`
                // here is what makes the native click and every scripted write to
                // `open` one mechanism.
                // ADR-016 M2.2c-2d: fire-and-forget С‡РµСЂРµР· РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ вЂ” РїРѕРґ
                // С„Р»Р°РіРѕРј off-UI-thread, Р±РµР· С„Р»Р°РіР° Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ.
                #[cfg(feature = "v8")]
                route_eval_js(
                    self.engine_thread.as_ref(),
                    self.js_ctx.as_ref(),
                    format!(
                        "_lumen_details_native_toggled({}, {})",
                        id.index(),
                        was_open
                    ),
                );
                // ADR-016 M2.2c-3: <details> open flip already applied to the
                // document (the routed `toggle` event above only notifies JS); no
                // geometry is read after в†’ route the reflow off-thread.
                self.relayout_form();
            }
            forms::FormClickAction::SlideRange(id) => {
                if let (Some(src), Some(lb)) =
                    (self.layout_source.as_mut(), self.layout_box.as_ref())
                    && let Some(rect) = forms::find_box_rect(lb, id)
                {
                    forms::apply_range_value(
                        &mut src.document.lock().unwrap(),
                        id,
                        rect,
                        page_x,
                    );
                }
                // ADR-016 M2.2c-3: range value applied to the document (the
                // pre-relayout `find_box_rect` read is against the old layout to map
                // the click x в†’ value); no post-relayout read в†’ off-thread.
                self.relayout_form();
            }
            forms::FormClickAction::SubmitForm(submit_node) => {
                // Phase 3: HTML5 form submission algorithm integration вЂ”
                // constraint validation, encoding and navigation all live in
                // `run_form_submission`, shared with script-initiated submits.
                let form_node = self.layout_source.as_ref().and_then(|src| {
                    let doc = src.document.lock().ok()?;
                    lumen_dom::find_ancestor_form(&doc, submit_node)
                });
                if let Some(form) = form_node {
                    self.run_form_submission(form, Some(submit_node), true);
                }
            }
            forms::FormClickAction::Nothing => {
                // в”Ђв”Ђ Link click в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ
                // No form control was activated вЂ” check if
                // the clicked node is inside an <a href>.
                // Use source_node (text node inside inline element) so find_link_href
                // can walk up and find the <a> parent: text в†’ <a href="вЂ¦"> в†’ found.
                // Falls back to r.node for non-inline boxes.
                let href = hit_result.as_ref().and_then(|r| {
                    self.layout_source
                        .as_ref()
                        .and_then(|src| links::find_link_href(&src.document.lock().unwrap(), r.source_node))
                });
                if let Some(href) = href {
                    if let Some(frag) = links::fragment_only(&href) {
                        if click_log::is_enabled() {
                            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                node_id: *nid, tag, id_attr: id, class_attr: cls,
                            });
                            click_log::log_click(&click_log::ClickInfo {
                                win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                hit: hit_ref,
                                outcome: click_log::ClickOutcome::LinkFragment(frag),
                            });
                        }
                        // Same-page fragment navigation.
                        self.navigate_fragment(frag.to_owned());
                    } else if links::is_navigable_href(&href) {
                        let resolved = self.source.resolve_href(&href);
                        // `about:newtab?...` special links (pin/unpin, "+",
                        // restore-closed, DS-11) are handled in-place, never
                        // as a real navigation.
                        if let Some(action) = newtab::parse_action(&resolved) {
                            self.apply_newtab_action(action);
                        } else if let Some(frag) =
                            links::same_document_fragment(self.current_display_url(), &resolved)
                        {
                            if click_log::is_enabled() {
                                let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                    node_id: *nid, tag, id_attr: id, class_attr: cls,
                                });
                                click_log::log_click(&click_log::ClickInfo {
                                    win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                    hit: hit_ref,
                                    outcome: click_log::ClickOutcome::LinkFragment(&frag),
                                });
                            }
                            self.navigate_fragment(frag);
                        } else {
                            if click_log::is_enabled() {
                                let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                    node_id: *nid, tag, id_attr: id, class_attr: cls,
                                });
                                click_log::log_click(&click_log::ClickInfo {
                                    win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                    hit: hit_ref,
                                    outcome: click_log::ClickOutcome::LinkNavigate {
                                        href: &href,
                                        resolved: &resolved,
                                    },
                                });
                            }
                            let target = PageSource::from_arg(Some(&resolved));
                            self.navigate_to(target);
                        }
                    } else {
                        if click_log::is_enabled() {
                            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                node_id: *nid, tag, id_attr: id, class_attr: cls,
                            });
                            click_log::log_click(&click_log::ClickInfo {
                                win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                hit: hit_ref,
                                outcome: click_log::ClickOutcome::LinkBlocked(&href),
                            });
                        }
                    }
                } else if click_log::is_enabled() {
                    let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                        node_id: *nid, tag, id_attr: id, class_attr: cls,
                    });
                    let outcome = if hit_result.is_none() {
                        click_log::ClickOutcome::NoHit
                    } else {
                        click_log::ClickOutcome::NoLink
                    };
                    click_log::log_click(&click_log::ClickInfo {
                        win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                        hit: hit_ref,
                        outcome,
                    });
                }
            }
        }
    }
}
