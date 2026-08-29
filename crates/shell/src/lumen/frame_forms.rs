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
    /// `true` — DOM ребёнка изменён и пересчитан. Вызывается ПОСЛЕ рассылки
    /// `click` в JS ребёнка, тем же порядком, что у страницы: обработчик видит
    /// состояние ДО переключения (HTML LS §4.10.5.5 меняет его в activation
    /// behavior, то есть после dispatch).
    pub(crate) fn frame_form_click(&mut self, idx: usize, node: NodeId, at: Point) -> bool {
        let Some(handle) = self.frames.get(idx) else { return false };
        let Ok(doc) = handle.doc.lock() else { return false };
        let action = forms::classify_click(&doc, node);
        drop(doc);

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
            // Оверлеи (`<select>`, палитра, календарь, файловый диалог) и
            // отправка формы в срез не входят и молча ничего не делают, потому
            // что каждый упирается в СВОЙ механизм, а не в этот: всплывающие
            // окна рисуются по прямоугольнику из layout СТРАНИЦЫ
            // (`self.layout_box`) и адресуются одним `NodeId` без фрейма, а
            // отправка формы — это навигация фрейма, отдельный пункт очереди
            // среза. Лог, а не тишина: иначе клик по `<select>` внутри фрейма
            // выглядит как потерянное событие.
            forms::FormClickAction::OpenSelectDropdown(_)
            | forms::FormClickAction::OpenColorPicker(_)
            | forms::FormClickAction::OpenDatePicker(_)
            | forms::FormClickAction::OpenFilePicker(_)
            | forms::FormClickAction::SubmitForm(_) => {
                eprintln!(
                    "iframe: элемент управления {action:?} внутри фрейма пока не поддержан (BUG-480 срез 18)"
                );
                false
            }
            forms::FormClickAction::Nothing => false,
        };
        if changed {
            self.relayout_frame(idx);
        }
        changed
    }

    /// Мутация дерева под-документа под коротким локом. `false` — лок отравлен
    /// (паника чужого потока): тогда пересчитывать нечего.
    fn with_frame_doc(&mut self, idx: usize, edit: impl FnOnce(&mut Document)) -> bool {
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

    /// Показать результат мутации: пересчитать содержимое фрейма и пересобрать
    /// display list страницы.
    ///
    /// Список страницы собирается ЦЕЛИКОМ по той же причине, что и при
    /// прокрутке фрейма ([`Lumen::apply_frame_scroll`]): вклейка содержимого
    /// живёт в `set_display_list`, а точечного патча у неё нет — содержимое
    /// ребёнка приезжает отдельным списком, а не слоем внутри страничного.
    ///
    /// `layout_box` временно ВЫНИМАЕТСЯ: пересчёт фрейма читает layout
    /// страницы (там стоит host-бокс) и одновременно пишет в `self.frames`.
    fn relayout_frame(&mut self, idx: usize) {
        let Some(page_layout) = self.layout_box.take() else { return };
        frames::relayout_frame_content(&mut self.frames, idx, &page_layout);
        self.layout_box = Some(page_layout);
        let rebuilt = self.layout_box.as_ref().map(paint_ordered);
        if let Some(new_dl) = rebuilt {
            self.tile_grid.update_from_diff(&self.display_list, &new_dl);
            self.set_display_list(new_dl);
        }
        self.request_redraw();
    }
}
