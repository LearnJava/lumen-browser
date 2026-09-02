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
    /// Клик по точке вьюпорта — и, если он сдвинул фокус ВНУТРИ фрейма,
    /// пересчёт под-документа (BUG-480 срез 23).
    ///
    /// Обёртка, а не строка в конце тела: у `handle_click_at_inner` два
    /// выхода — ранний `return` ветки фрейма и обычный конец страничной, — и
    /// проверка в одном из них молча не покрывала бы второй. Страница платит
    /// за смену `:focus` тем же самым (`relayout_chrome` в теле ниже).
    pub(crate) fn handle_click_at(&mut self, x_css: f32, y_css: f32) {
        let before = self.focused_frame;
        self.handle_click_at_inner(x_css, y_css);
        if self.focused_frame != before {
            self.notify_frame_focus(before, self.focused_frame);
            self.refresh_frames(None);
        }
    }

    /// Сообщить JS-контекстам фреймов о смене фокуса внутри под-документа —
    /// `document.activeElement` ребёнка (BUG-480 срез 23).
    ///
    /// Страница делает то же самое `notify_focus_changed` по своему контексту;
    /// здесь адресат — контекст РЕБЁНКА, поэтому вызов уходит прямым `eval_js`
    /// по его хэндлу, а не через `route_task_js` (тот знает только контекст
    /// страницы — та же причина, что у `frame_mouse_event` и у клавиатурных
    /// событий среза 22).
    ///
    /// Уведомляются ОБА фрейма — покинутый и получивший фокус: без первого
    /// `document.activeElement` ушедшего остался бы указывать на свой узел, и
    /// два под-документа одновременно считали бы себя сфокусированными.
    #[allow(unused_variables)] // хэндл читается только под feature = "v8"
    pub(crate) fn notify_frame_focus(
        &mut self,
        before: Option<(usize, NodeId)>,
        after: Option<(usize, NodeId)>,
    ) {
        #[cfg(feature = "v8")]
        {
            let notify = |idx: usize, nid: Option<NodeId>| {
                if let Some(js) = self.frames.get(idx).and_then(|h| h.js.clone()) {
                    js.notify_focus_changed(nid.map(|n| n.index() as u32));
                }
            };
            if let Some((idx, _)) = before
                && before.map(|(i, _)| i) != after.map(|(i, _)| i)
            {
                notify(idx, None);
            }
            if let Some((idx, nid)) = after {
                notify(idx, Some(nid));
            }
        }
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn handle_click_at_inner(&mut self, x_css: f32, y_css: f32) {
        // Dismiss validation tooltip on any non-scrollbar click.
        self.validation_tooltip = None;
        let scroll_y = self.scroll_y;

        // DevTools inspector: a click pins the box under the cursor and shows
        // its computed style, suppressing normal navigation / JS dispatch.
        if self.dom_inspector.visible {
            let win_w_css = self.viewport_width_css();
            // Click inside the right-docked panel → UI interaction (tab switch).
            if self.dom_inspector.is_panel_click(x_css, win_w_css) {
                if self.dom_inspector.click_tab_at(
                    x_css, y_css, win_w_css,
                    toolbar::CHROME_H,
                ) {
                    self.request_redraw();
                }
                return;
            }
            // Click on the page → pin the box under cursor.
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

        // ── Color picker swatch hit ──────────────────────
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
            // ADR-016 M2.2c-3: value already in the document; no post-read → off-thread.
            self.relayout_form();
            return;
        }
        // Any click outside the picker closes it.
        self.color_picker_node = None;

        // ── Frame color picker swatch hit (FRAME-6) ───────
        // Зеркало страничной проверки выше, но anchor ищется в layout
        // РЕБЁНКА и переводится в координаты страницы `frame_overlay_anchor`
        // (`frame_forms.rs`) — оверлей рисуется viewport-locked поверх всего,
        // как и страничный, так что `x_css`/`y_css` сравниваются напрямую.
        let frame_picker_swatch_result: Option<(usize, NodeId, [u8; 3])> = self
            .frame_color_picker
            .and_then(|(idx, pn)| {
                let anchor = self.frame_overlay_anchor(idx, pn)?;
                let color = forms::hit_color_swatch(anchor, scroll_y, x_css, y_css)?;
                Some((idx, pn, color))
            });
        if let Some((idx, pn, color)) = frame_picker_swatch_result {
            self.frame_color_picker = None;
            let css_color = forms::swatch_to_css_color(color);
            self.with_frame_doc(idx, |doc| forms::set_value(doc, pn, &css_color));
            self.refresh_frames(Some(idx));
            return;
        }
        // Any click outside the frame picker closes it.
        self.frame_color_picker = None;

        // ── Date picker hit ──────────────────────────────
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
                    // ADR-016 M2.2c-3: async-safe form mutation — see color picker.
                    self.relayout_form();
                    return;
                }
                forms::DatePickerHit::None => {}
            }
        }
        // Any click outside the date picker closes it.
        self.date_picker_node = None;

        // ── Frame date picker hit (FRAME-6) ───────────────
        let frame_date_hit: Option<(usize, NodeId, forms::DatePickerHit)> =
            self.frame_date_picker.and_then(|(idx, dn)| {
                let anchor = self.frame_overlay_anchor(idx, dn)?;
                let vp_w2 = self.viewport_width_css();
                let hit = forms::hit_date_picker(
                    anchor, scroll_y, vp_w2,
                    self.frame_date_picker_year, self.frame_date_picker_month,
                    x_css, y_css,
                );
                Some((idx, dn, hit))
            });
        if let Some((idx, dn, hit)) = frame_date_hit {
            match hit {
                forms::DatePickerHit::Prev => {
                    let (ny, nm) = forms::advance_month(
                        self.frame_date_picker_year, self.frame_date_picker_month, -1,
                    );
                    self.frame_date_picker_year = ny;
                    self.frame_date_picker_month = nm;
                    self.request_redraw();
                    return;
                }
                forms::DatePickerHit::Next => {
                    let (ny, nm) = forms::advance_month(
                        self.frame_date_picker_year, self.frame_date_picker_month, 1,
                    );
                    self.frame_date_picker_year = ny;
                    self.frame_date_picker_month = nm;
                    self.request_redraw();
                    return;
                }
                forms::DatePickerHit::Day(day) => {
                    self.frame_date_picker = None;
                    let date_str = forms::format_date_value(
                        self.frame_date_picker_year, self.frame_date_picker_month, day,
                    );
                    self.with_frame_doc(idx, |doc| forms::set_value(doc, dn, &date_str));
                    self.refresh_frames(Some(idx));
                    return;
                }
                forms::DatePickerHit::None => {}
            }
        }
        // Any click outside the frame date picker closes it.
        self.frame_date_picker = None;

        // ── Select dropdown option hit ───────────────────
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
                    // ADR-016 M2.2c-3: async-safe <select> choice — see color picker.
                    self.relayout_form();
                }
            }
            return;
        }
        // Any click outside the dropdown closes it.
        self.select_dropdown_node = None;

        // ── Frame select dropdown option hit (FRAME-6) ────
        let frame_select_hit: Option<(usize, NodeId, usize)> =
            self.frame_select_dropdown.and_then(|(idx, sn)| {
                let anchor = self.frame_overlay_anchor(idx, sn)?;
                let opts_count = self
                    .frames
                    .get(idx)
                    .and_then(|h| h.doc.lock().ok())
                    .map(|doc| forms::collect_select_options(&doc, sn).len())
                    .unwrap_or(0);
                let vp_h = self.viewport_height_css();
                let vp_w2 = self.viewport_width_css();
                let opt_idx =
                    forms::hit_select_option(anchor, opts_count, scroll_y, vp_w2, vp_h, x_css, y_css)?;
                Some((idx, sn, opt_idx))
            });
        if let Some((idx, sn, opt_idx)) = frame_select_hit {
            self.frame_select_dropdown = None;
            // Scoped so the `MutexGuard` (and the `self.frames` borrow behind
            // it) drop before `refresh_frames` needs `&mut self` again.
            let changed = self.frames.get(idx).is_some_and(|handle| {
                let Ok(mut doc) = handle.doc.lock() else { return false };
                let opts = forms::collect_select_options(&doc, sn);
                if opts.get(opt_idx).is_some_and(|o| o.disabled) {
                    return false;
                }
                forms::apply_select_choice(&mut doc, &opts, opt_idx);
                true
            });
            if changed {
                self.refresh_frames(Some(idx));
            }
            return;
        }
        // Any click outside the frame dropdown closes it.
        self.frame_select_dropdown = None;

        // ── Form control + link click ────────────────────
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

        // Debug click log — активируется флагом --click-log или LUMEN_CLICK_LOG=1.
        // For click log: report both the hit box node (<p>) and the inline source_node
        // (<a> text node) so the log shows what find_link_href actually searches from.
        let click_log_hit: Option<(u32, String, String, String)> =
            if click_log::is_enabled() {
                hit_result.as_ref().and_then(|r| {
                    self.layout_source.as_ref().map(|src| {
                        let doc = src.document.lock().unwrap();
                        // Use source_node for tag/class info — it reveals the inline element.
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
            // (the geometry the user clicked on — correct), and any DOM mutation
            // from those handlers takes its own generation-guarded relayout, so
            // this pure restyle has no synchronous geometry read and goes off-thread.
            self.relayout_chrome();
            // Notify platform accessibility bridge so screen readers can track focus.
            self.platform_bridge.focused_node_changed(new_focused);
            // Keep JS _lumen_last_focused_nid in sync so showModal() can save/restore it.
            // ADR-016 M2.2c-2d (16): fire-and-forget void `notify_focus_changed` через
            // `route_task_js`. `focus_idx` (owned `Option<u32>`) вычисляется до
            // маршрутизации, замыкание `Send + 'static`. Под флагом
            // (`LUMEN_ENGINE_THREAD=1`) уходит off-UI-thread одним `task`; без флага
            // (по умолчанию) — синхронный вызов по UI-хэндлу, **байт-идентично**
            // прежнему `js.notify_focus_changed`.
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
                // BUG-480 срез 22: помимо host-фокуса страницы (уже
                // переведён выше), typeable-поле ВНУТРИ фрейма запоминается
                // отдельно — `focused_node` не может его адресовать
                // (`NodeId` уникален только в своём документе, та же причина,
                // что у `hovered_frame`). Не typeable-узел → предыдущий
                // адресат клавиатуры фрейма забывается, как у страницы
                // (`focused_node` тоже переустанавливается на КАЖДЫЙ клик).
                //
                // Срез 23: сюда пишется ЛЮБОЙ узел под точкой, а не только
                // typeable-поле — `:focus` внутри фрейма обязан вести себя
                // как на странице, где `focused_node` тоже принимает любой
                // узел. Обе точки ввода текста перепроверяют typeable-ность
                // на месте использования, так что срез 22 от этого не страдает.
                self.focused_frame = Some((target.frame, hit.node));
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
            } else {
                // Точка во фрейме, но ни на один его бокс не попала (пустой
                // под-документ, зазор под содержимым) — адресата клавиатуры
                // нет, как у страницы при клике мимо любого узла.
                self.focused_frame = None;
            }
            return;
        }
        // Клик вне содержимого любого фрейма — прежний фокус фрейма, если
        // был, больше не адресат клавиатуры (страница ведёт себя так же:
        // `focused_node` выше переустановлен на КАЖДЫЙ клик).
        self.focused_frame = None;

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
            // ADR-016 M2.2c-2d (10): read-after-eval click dispatch — сам
            // `_lumen_dispatch_mouse_event('click', …)` уходит fire-and-forget через
            // `route_eval_js`, а последующий `take_navigate_request` (навигация, что
            // handler мог поставить) — через `route_query_js`. Под флагом
            // (`LUMEN_ENGINE_THREAD=1`) блокирующий `query` встаёт в очередь **после**
            // отправленного `task`, восстанавливая read-after-eval порядок; без флага
            // (по умолчанию) — прежние синхронные вызовы по UI-хэндлу, байт-идентично
            // (`js_ctx == None` → `None` → навигация не ставится, как прежняя
            // ветка `Some(ctx)` не сматчилась).
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
                // document; no geometry is read after → route the reflow off-thread.
                self.relayout_form();
            }
            forms::FormClickAction::ToggleRadio {
                clicked,
                _group_name: _,
            } => {
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_checkbox(&mut src.document.lock().unwrap(), clicked);
                }
                // ADR-016 M2.2c-3: async-safe form mutation — see ToggleCheckbox.
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
                // — and fired two `toggle` events about the change that did not
                // happen. That listener is gone; JS is only *told* what changed.
                let was_open = self.layout_source.as_ref().is_some_and(|src| {
                    src.document
                        .lock()
                        .is_ok_and(|doc| doc.get(id).get_attr("open").is_some())
                });
                if let Some(src) = self.layout_source.as_mut() {
                    forms::toggle_details_open(&mut src.document.lock().unwrap(), id);
                }
                // HTML LS §4.11.1 attribute change steps for `open` — the queued
                // `toggle` event and the exclusive-accordion pass. Routing them
                // through the shim instead of dispatching a bare `Event('toggle')`
                // here is what makes the native click and every scripted write to
                // `open` one mechanism.
                // ADR-016 M2.2c-2d: fire-and-forget через маршрутизатор — под
                // флагом off-UI-thread, без флага байт-идентично.
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
                // geometry is read after → route the reflow off-thread.
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
                // the click x → value); no post-relayout read → off-thread.
                self.relayout_form();
            }
            forms::FormClickAction::SubmitForm(submit_node) => {
                // Phase 3: HTML5 form submission algorithm integration —
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
                // ── Link click ───────────────────────────
                // No form control was activated — check if
                // the clicked node is inside an <a href>.
                // Use source_node (text node inside inline element) so find_link_href
                // can walk up and find the <a> parent: text → <a href="…"> → found.
                // Falls back to r.node for non-inline boxes.
                // The `target` attribute rides along with the href (BUG-480
                // slice 24): a page link can name an existing frame the same
                // way a frame link already does (frame_links.rs), and reading
                // it in this one lock keeps the two answers from a link that
                // mutates between two separate walks.
                let link = hit_result.as_ref().and_then(|r| {
                    self.layout_source.as_ref().and_then(|src| {
                        let doc = src.document.lock().unwrap();
                        links::find_link(&doc, r.source_node).map(|(anchor, href)| {
                            let target = doc.get(anchor).get_attr("target").unwrap_or_default().to_owned();
                            (href, target)
                        })
                    })
                });
                if let Some((href, target_attr)) = link {
                    let t = target_attr.trim();
                    let named_frame = (!t.is_empty() && !t.eq_ignore_ascii_case("_self"))
                        .then(|| self.find_frame_by_name(t))
                        .flatten();
                    if let Some(idx) = named_frame
                        && links::is_navigable_href(&href)
                    {
                        // The link lives in the PAGE, so its own base is the
                        // page's (`frame_env.page_base`), not the target
                        // frame's — same rule `_parent` already follows in
                        // `Lumen::link_destination`.
                        if click_log::is_enabled() {
                            let hit_ref = click_log_hit.as_ref().map(|(nid, tag, id, cls)| click_log::HitInfo {
                                node_id: *nid, tag, id_attr: id, class_attr: cls,
                            });
                            click_log::log_click(&click_log::ClickInfo {
                                win_x: x_css, win_y: y_css, page_x, page_y, scroll_y,
                                hit: hit_ref,
                                outcome: click_log::ClickOutcome::LinkIntoNamedFrame { frame: idx, href: &href },
                            });
                        }
                        if let Some(nav_base) = self.frame_env.as_ref().map(|e| e.page_base.clone()) {
                            self.navigate_frame_to(idx, &href, &nav_base);
                        }
                    } else if let Some(frag) = links::fragment_only(&href) {
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
