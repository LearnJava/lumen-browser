//! Нативное поведение элементов управления формы ВНУТРИ фрейма
//! (BUG-480 срез 18).
//!
//! Срез 16 довёл клик мышью до под-документа, но только как СОБЫТИЕ: путь
//! родителя обрывается ранним возвратом в [`Lumen::handle_click_at`], а вместе
//! с ним отпадают и разбор клика формой, и ссылка — единственный узел
//! СТРАНИЦЫ под этой точкой сам `<iframe>`. Значит собственное поведение
//! элемента (флажок, радиокнопка, `<summary>`, ползунок) для ребёнка не
//! исполнял никто.
//!
//! Здесь та же пара «классифицировать → применить → пересчитать», что у
//! страницы в [`crate::lumen::click`], но против ДРУГОГО документа и с
//! пересчётом ДРУГОГО дерева: [`crate::forms`] принимает любой `&Document`,
//! поэтому переиспользуется целиком — расходиться правилам «что делает клик по
//! `<summary>`» внутри одного движка нельзя.

use crate::*;

impl Lumen {
    /// Исполнить нативное поведение элемента `node` под-документа фрейма `idx`.
    ///
    /// `at` — точка клика в системе координат РЕБЁНКА (то же, что уходит в его
    /// `clientX`/`clientY`). Из неё берётся только `x` — для ползунка;
    /// горизонтальной прокрутки у под-документа нет, поэтому вьюпортная и
    /// документная системы по этой оси совпадают (срез 17).
    ///
    /// `true` — клик РАЗОБРАН формой, то есть ссылку по нему искать уже не
    /// нужно; это не то же самое, что «дерево изменилось» (`<select>` открыл бы
    /// оверлей, ничего не меняя). Гейт тот же, что у страницы, где ссылка
    /// разбирается в ветке `FormClickAction::Nothing`.
    ///
    /// Вызывается ПОСЛЕ рассылки `click` в JS ребёнка, тем же порядком, что у
    /// страницы: обработчик видит состояние ДО переключения (HTML LS §4.10.5.5
    /// меняет его в activation behavior, то есть после dispatch).
    pub(crate) fn frame_form_click(&mut self, idx: usize, node: NodeId, at: Point) -> bool {
        let Some(handle) = self.frames.get(idx) else { return false };
        let Ok(doc) = handle.doc.lock() else { return false };
        let action = forms::classify_click(&doc, node);
        drop(doc);
        let handled = !matches!(action, forms::FormClickAction::Nothing);

        let changed = match action {
            // Радиокнопка проходит тем же путём, что флажок, — ровно как у
            // страницы: снятия отметки с соседей по группе шелл не делает ни
            // там, ни здесь, и заводить это расхождение во фрейме нельзя.
            forms::FormClickAction::ToggleCheckbox(id)
            | forms::FormClickAction::ToggleRadio { clicked: id, .. } => {
                self.with_frame_doc(idx, |doc| forms::toggle_checkbox(doc, id))
            }
            forms::FormClickAction::ToggleDetails(id) => self.frame_toggle_details(idx, id),
            forms::FormClickAction::SlideRange(id) => self.frame_slide_range(idx, id, at.x),
            // Отправка формы (срез 20) — это навигация фрейма, поэтому она
            // ничего не перерисовывает здесь: `run_frame_form_submission`
            // заменяет под-документ целиком и обновляет экран сама.
            forms::FormClickAction::SubmitForm(submit_node) => {
                let form = self
                    .frames
                    .get(idx)
                    .and_then(|h| h.doc.lock().ok())
                    .and_then(|doc| lumen_dom::find_ancestor_form(&doc, submit_node));
                if let Some(form) = form {
                    self.run_frame_form_submission(idx, form, Some(submit_node), true);
                }
                false
            }
            // Оверлеи (`<select>`, палитра, календарь, файловый диалог) —
            // FRAME-6: те же три поля `Lumen`, что у страницы
            // (`color_picker_node`/`date_picker_node`/`select_dropdown_node`),
            // только `(индекс фрейма, NodeId)` — see `Lumen::frame_color_picker`
            // doc comment (`state.rs`) for why. Открытие само по себе не меняет
            // дерево ребёнка — `changed` остаётся `false`, как у страничных
            // веток в `click.rs::handle_click_at_inner`; попадание в сам
            // оверлей разбирается ТАМ ЖЕ, где страничное — в начале функции,
            // до hit-теста, потому что оверлей рисуется viewport-locked поверх
            // всего, а не внутри фрейма.
            forms::FormClickAction::OpenColorPicker(id) => {
                self.frame_color_picker = Some((idx, id));
                self.request_redraw();
                false
            }
            forms::FormClickAction::OpenDatePicker(id) => {
                let (y, m) = self
                    .frames
                    .get(idx)
                    .and_then(|h| h.doc.lock().ok())
                    .and_then(|doc| {
                        let val = doc.control_value(id).into_owned();
                        forms::parse_date_value(&val).map(|(y, m, _)| (y, m))
                    })
                    .unwrap_or_else(forms::today_year_month);
                self.frame_date_picker = Some((idx, id));
                self.frame_date_picker_year = y;
                self.frame_date_picker_month = m;
                self.request_redraw();
                false
            }
            forms::FormClickAction::OpenSelectDropdown(id) => {
                self.frame_select_dropdown = Some((idx, id));
                self.request_redraw();
                false
            }
            forms::FormClickAction::OpenFilePicker(id) => {
                self.open_frame_file_picker(idx, id);
                false
            }
            forms::FormClickAction::Nothing => false,
        };
        if changed {
            self.refresh_frames(Some(idx));
        }
        handled
    }

    /// Мутация дерева под-документа под коротким локом. `false` — лок отравлен
    /// (паника чужого потока): тогда пересчитывать нечего.
    ///
    /// `pub(crate)`, а не приватный модулю: тем же приёмом пользуется срез 22
    /// (`frame_text_input.rs`) для записи значения typeable-поля.
    pub(crate) fn with_frame_doc(&mut self, idx: usize, edit: impl FnOnce(&mut Document)) -> bool {
        let Some(handle) = self.frames.get(idx) else { return false };
        let Ok(mut doc) = handle.doc.lock() else { return false };
        edit(&mut doc);
        true
    }

    /// HTML LS §4.11.1 для `<details>` ребёнка: перевернуть `open` и сообщить
    /// об этом ЕГО рантайму.
    ///
    /// Сообщение уходит прямым `eval_js` по хэндлу фрейма, а не через
    /// `route_eval_js`: тот знает только `self.js_ctx` — контекст страницы, —
    /// а событие `toggle` принадлежит документу ребёнка (срез 16, та же
    /// причина у [`Lumen::frame_mouse_event`]).
    fn frame_toggle_details(&mut self, idx: usize, id: NodeId) -> bool {
        let was_open = self
            .frames
            .get(idx)
            .and_then(|h| h.doc.lock().ok())
            .is_some_and(|doc| doc.get(id).get_attr("open").is_some());
        if !self.with_frame_doc(idx, |doc| forms::toggle_details_open(doc, id)) {
            return false;
        }
        #[cfg(feature = "v8")]
        if let Some(js) = self.frames.get(idx).and_then(|h| h.js.as_ref()) {
            js.eval_js(&format!(
                "_lumen_details_native_toggled({}, {})",
                id.index(),
                was_open
            ));
        }
        true
    }

    /// Поставить ползунок ребёнка в позицию, соответствующую `x`.
    ///
    /// Прямоугольник ищется в layout ПОД-ДОКУМЕНТА: `NodeId` уникален лишь
    /// внутри своего документа, и поиск в layout страницы нашёл бы либо
    /// ничего, либо чужой бокс с совпавшим индексом (та же причина, по которой
    /// [`crate::frames::sync_frame_viewports`] ходит по глубинам).
    fn frame_slide_range(&mut self, idx: usize, id: NodeId, x: f32) -> bool {
        let Some(rect) = self
            .frames
            .get(idx)
            .and_then(|h| h.layout.as_ref())
            .and_then(|lb| forms::find_box_rect(lb, id))
        else {
            return false;
        };
        self.with_frame_doc(idx, |doc| forms::apply_range_value(doc, id, rect, x))
    }

    /// Интерактивное состояние под-документов одним значением (BUG-480 срез 23).
    ///
    /// Три поля `Lumen` — единственный источник истины; [`crate::frames::FrameHandle::interactive`]
    /// хранит лишь то, с чем ребёнок был посчитан в последний раз, и служит
    /// гейтом пересчёта, как `viewport` рядом с ним.
    pub(crate) fn frame_interactive(&self) -> frames::FrameInteractive {
        frames::FrameInteractive {
            hovered: self.hovered_frame,
            focused: self.focused_frame,
            active: self.active_frame,
        }
    }

    /// Показать результат изменения содержимого фреймов: пересчитать их
    /// вьюпорты/списки и пересобрать display list страницы.
    ///
    /// `relayout`: `Some(idx)` — дерево ЭТОГО фрейма изменилось при неизменном
    /// вьюпорте (нативное переключение элемента управления, срез 18), и гейт
    /// «размер хоста не менялся» пропустил бы правку молча; `None` — состав
    /// или адрес фреймов изменился и пересчитать нужно всё (навигация фрейма,
    /// срез 19: после замены хэндла прежний индекс адресует не тот фрейм).
    ///
    /// Список страницы собирается ЦЕЛИКОМ по той же причине, что и при
    /// прокрутке фрейма ([`Lumen::apply_frame_scroll`]): вклейка содержимого
    /// живёт в `set_display_list`, а точечного патча у неё нет — содержимое
    /// ребёнка приезжает отдельным списком, а не слоем внутри страничного.
    ///
    /// `layout_box` временно ВЫНИМАЕТСЯ: пересчёт фрейма читает layout
    /// страницы (там стоит host-бокс) и одновременно пишет в `self.frames`.
    pub(crate) fn refresh_frames(&mut self, relayout: Option<usize>) {
        let Some(page_layout) = self.layout_box.take() else { return };
        let interactive = self.frame_interactive();
        match relayout {
            Some(idx) => {
                frames::relayout_frame_content(&mut self.frames, idx, &page_layout, interactive)
            }
            None => frames::sync_frame_viewports(&mut self.frames, &page_layout, interactive),
        }
        // FRAME-5 срез 2: same as `Lumen::apply_relayout_result` — this path
        // has no page-commit step for a freshly lazy-loaded frame image to
        // ride into the renderer, so register it explicitly.
        self.register_frame_lazy_images();
        self.layout_box = Some(page_layout);
        let rebuilt = self.layout_box.as_ref().map(paint_ordered);
        if let Some(new_dl) = rebuilt {
            self.tile_grid.update_from_diff(&self.display_list, &new_dl);
            self.set_display_list(new_dl);
        }
        self.request_redraw();
    }

    /// Прямоугольник контрола `node` под-документа фрейма `idx` в координатах
    /// СТРАНИЦЫ (FRAME-6) — тот же приём, что уже возит подсказку валидации
    /// ([`super::frame_form_submit::Lumen::show_frame_validation_tooltip`]):
    /// бокс ищется в layout РЕБЁНКА (`NodeId` уникален лишь внутри своего
    /// документа), затем переводится [`frames::frame_page_origin`].
    /// `build_color_picker`/`build_date_picker`/`build_select_dropdown`
    /// (`forms.rs`) сами вычитают `self.scroll_y` при отрисовке — ровно так
    /// же, как уже делает `build_validation_tooltip` для страничного anchor.
    pub(crate) fn frame_overlay_anchor(&self, idx: usize, node: NodeId) -> Option<Rect> {
        let rect = self.frames.get(idx).and_then(|h| {
            let lb = h.layout.as_ref()?;
            forms::find_box_rect(lb, node)
        })?;
        let (ox, oy) = frames::frame_page_origin(&self.frames, idx)?;
        Some(Rect {
            x: rect.x + ox,
            y: rect.y + oy,
            width: rect.width,
            height: rect.height,
        })
    }

    /// OS-диалог `<input type="file">` ВНУТРИ под-документа фрейма (FRAME-6).
    ///
    /// Зеркало [`Lumen::open_file_picker`] страницы (`file_picker.rs`), но
    /// токен регистрируется на origin РЕБЁНКА, не через
    /// `lumen_js::file_input::active_document_origin()`: тот глобал держит
    /// origin, с которым `install_dom` устанавливал биндинги ПОСЛЕДНИМ (у
    /// него нет памяти «для какого документа»), а на странице с фреймом это
    /// не обязательно origin документа, из которого кликнули. `origin_for_url`
    /// — та же функция, которой сам `install_dom` вычисляет `page_origin`
    /// перед `install_file_input_bindings_v8` (`crates/js/src/v8_runtime.rs`),
    /// так что для ЭТОГО фрейма она даёт байт-в-байт то значение, которым его
    /// собственные read-биндинги были установлены, независимо от того, что
    /// сейчас лежит в глобале.
    pub(crate) fn open_frame_file_picker(&mut self, idx: usize, id: NodeId) {
        let Some((accept, multiple)) = self.frames.get(idx).and_then(|h| {
            let doc = h.doc.lock().ok()?;
            let n = doc.get(id);
            Some((
                n.get_attr("accept").unwrap_or("").to_string(),
                n.get_attr("multiple").is_some(),
            ))
        }) else {
            return;
        };
        let entries = platform::file_dialog::open_file_dialog(&accept, multiple);
        if entries.is_empty() {
            // Пользователь отменил — событие не летит (HTML LS §4.10.5.1.16.3 шаг 3).
            return;
        }
        #[cfg(feature = "v8")]
        {
            let Some(handle) = self.frames.get(idx) else { return };
            let Some(js) = handle.js.clone() else { return };
            let origin = lumen_js::file_input::origin_for_url(&handle.url);
            let tokens: Vec<String> = entries
                .iter()
                .map(|e| lumen_js::file_input::register_file_token(&e.path, &origin))
                .collect();
            let json = platform::file_dialog::entries_to_json_with_tokens(&entries, &tokens);
            js.eval_js(&format!("_lumen_deliver_file_list({}, {})", id.index(), json));
        }
        #[cfg(not(feature = "v8"))]
        let _ = entries;
    }
}
