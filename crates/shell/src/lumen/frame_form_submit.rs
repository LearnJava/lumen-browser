//! Отправка формы под-документа фрейма (BUG-480 срез 20).
//!
//! Срез 18 довёл до ребёнка собственное поведение элементов управления и
//! отклонил отправку явно — «это навигация фрейма, отдельный пункт очереди»;
//! срез 19 навигацию фрейма дал. Здесь эти две половины соединяются: правила
//! САМОЙ отправки (валидация, сбор полей, enctype, кодирование, событие
//! `submit`) переиспользуются целиком — это вопросы про форму, а не про то,
//! в каком документе она лежит, — а ново только то, куда уходит результат.
//!
//! Оба входа, что есть у страницы, есть и здесь, и по той же причине ведут в
//! одно тело: нативный клик по submit-кнопке и `form.submit()`/
//! `requestSubmit()` из скрипта самого ребёнка. Отличие ровно одно — кто уже
//! разослал событие `submit` (§4.10.21.3), и оно вынесено в параметр, как у
//! [`Lumen::run_form_submission`].

use crate::*;
use crate::lumen::frame_links::LinkTarget;

impl Lumen {
    /// HTML LS §4.10.21.4 для формы `form` под-документа фрейма `idx`.
    ///
    /// Зеркало [`Lumen::run_form_submission`] и с теми же аргументами:
    /// `submitter` — активированная кнопка (`None` — `form.submit()` из
    /// скрипта), `fire_submit_event` — нужен ли шаг 11 (нативный клик передаёт
    /// `true`, скриптовые пути `false`, потому что `requestSubmit()` уже
    /// разослал событие на JS-стороне, а `submit()` его пропускает по спеке).
    ///
    /// Всё, ради чего нужен лок дерева, читается ОДНИМ коротким заимствованием
    /// до любого JS: рассылка `submit` входит в рантайм ребёнка, который берёт
    /// лок того же самого документа — держать `doc` через этот вызов значило бы
    /// заклинить UI-поток (урок BUG-437 на странице, здесь он ровно тот же).
    pub(crate) fn run_frame_form_submission(
        &mut self,
        idx: usize,
        form: NodeId,
        submitter: Option<NodeId>,
        fire_submit_event: bool,
    ) {
        let Some(handle) = self.frames.get(idx) else { return };
        // База КЛОНИРУЕТСЯ: адрес `action` написан ребёнком и резолвится его
        // базой, а ниже по коду `self.frames` берётся изменяемо (срез 19).
        let nav_base = handle.base.clone();
        let prepared = {
            let Ok(doc) = handle.doc.lock() else { return };
            let submit_event = lumen_dom::submit_form(&doc, form);
            let enctype = forms::enctype_of_form(&doc, form);
            let dialog_node = lumen_dom::find_ancestor_dialog(&doc, submitter.unwrap_or(form));
            // `target` читается с ФОРМЫ, а не с кнопки: `formtarget` (как и
            // `formaction`/`formmethod`) страница не учитывает нигде, и заводить
            // это расхождение во фрейме нельзя — отклонение записано в
            // bugs/BUG-480-OPEN.md.
            let target = doc.get(form).get_attr("target").unwrap_or_default().to_owned();
            (submit_event, enctype, dialog_node, target)
        };
        let (submit_event, enctype, dialog_node, target) = prepared;
        match submit_event {
            lumen_dom::FormSubmitEvent::Valid { action, method, fields } => {
                if fire_submit_event
                    && let Some(sub) = submitter
                    && !self.frame_dispatch_submit_event(idx, form, sub)
                {
                    return;
                }
                let body = if enctype == "multipart/form-data" {
                    let boundary = "----LumenFormBoundary0000000000000000";
                    let (_ct, bytes) = forms::encode_form_fields_multipart(&fields, boundary);
                    String::from_utf8_lossy(&bytes).into_owned()
                } else {
                    forms::encode_form_fields(&fields)
                };
                use lumen_core::event::{Event, TabId};
                self.event_sink.emit(&Event::FormSubmit {
                    tab_id: TabId(0),
                    action: action.clone(),
                    method: method.clone(),
                    body: body.clone(),
                });
                match method.as_str() {
                    // §4.10.18.3: закрыть ближайший `<dialog>`. Сообщение уходит
                    // прямым вызовом по хэндлу фрейма, а не через `route_task_js`:
                    // тот знает только `self.js_ctx` — контекст СТРАНИЦЫ, — а
                    // `<dialog>` принадлежит документу ребёнка.
                    "dialog" => {
                        let rv = fields
                            .iter()
                            .find(|(n, _)| n.is_empty() || n == "value")
                            .map(|(_, v)| v.as_str())
                            .unwrap_or("");
                        if let Some(dnid) = dialog_node
                            && let Some(js) = self.frames.get(idx).and_then(|h| h.js.clone())
                        {
                            js.fire_dialog_close(dnid.index() as u32, rv);
                        }
                    }
                    "get" => {
                        let url_body = if enctype == "multipart/form-data" {
                            forms::encode_form_fields(&fields)
                        } else {
                            body.clone()
                        };
                        let get_url = forms::make_get_url(&action, &url_body);
                        self.frame_submit_navigate(idx, &get_url, &target, &nav_base);
                    }
                    _ => {
                        // POST не отправляет и страница (`run_form_submission`) —
                        // сетевая половина там не написана вовсе. Расхождения
                        // «во фрейме умеем, на странице нет» быть не может.
                        eprintln!(
                            "[forms] iframe POST {action} enctype={enctype} body-len={}",
                            body.len()
                        );
                    }
                }
            }
            lumen_dom::FormSubmitEvent::Invalid { invalid_controls } => {
                // §4.10.21.4 шаг 4 отклоняет отправку ДО шага 11, поэтому
                // событие `submit` здесь не рассылается — так же, как у страницы.
                if let Some(&first_invalid) = invalid_controls.first() {
                    self.show_frame_validation_tooltip(idx, first_invalid);
                }
                eprintln!(
                    "forms: iframe submit blocked — {} control(s) failed constraint validation",
                    invalid_controls.len()
                );
            }
        }
    }

    /// Шаг 11 — отменяемое событие `submit` в контексте РЕБЁНКА.
    ///
    /// `false` — обработчик страницы-ребёнка вызвал `preventDefault()`. Как и у
    /// страницы, отсутствие рантайма и ошибка самого вызова дают `true`:
    /// документ без скриптов обязан отправляться, а сломанная рассылка не имеет
    /// права молча съесть настоящую отправку.
    fn frame_dispatch_submit_event(&mut self, idx: usize, form: NodeId, submitter: NodeId) -> bool {
        let Some(js) = self.frames.get(idx).and_then(|h| h.js.clone()) else {
            return true;
        };
        let script = format!(
            "_lumen_dispatch_submit_event({}, {})",
            form.index(),
            submitter.index(),
        );
        // `_lumen_dispatch_rich` отдаёт `!event.defaultPrevented`, JSON-ом —
        // значит отменяет только литеральный `false`.
        //
        // Навигацию, которую обработчик мог поставить в очередь
        // (`location.href = …`), здесь не забирают: скриптовая навигация фрейма
        // отклоняется срезом 1 и дренируется общим пампом в `about_to_wait`,
        // так что забрать её тут значило бы исполнить её на СТРАНИЦЕ.
        match js.eval_js_value(&script) {
            Ok(json) => json.trim() != "false",
            Err(_) => true,
        }
    }

    /// Куда уходит результат GET-отправки: `_self`/пустой `target` — сам фрейм,
    /// `_top` — страница, `_parent` — фрейм уровнем выше (срез 19 отвечает на
    /// этот вопрос для ссылок, а §4.10.21.4 шаг 15 ссылается на тот же §4.6.3).
    ///
    /// Фрагментной ветки здесь нет намеренно: GET-отправка заменяет query
    /// целиком, поэтому её адрес не может быть «тем же документом с другим
    /// `#id`» — путь страницы (`run_form_submission`) по той же причине зовёт
    /// `navigate_to` напрямую, минуя `fragment_only`.
    fn frame_submit_navigate(
        &mut self,
        idx: usize,
        get_url: &str,
        target: &str,
        nav_base: &ResourceBase,
    ) {
        match self.link_destination(idx, target) {
            LinkTarget::NewWindow => {
                eprintln!(
                    "iframe: отправка формы с target='{target}' — вспомогательные окна не поддержаны (BUG-883)"
                );
            }
            LinkTarget::Page => {
                if !links::is_navigable_href(get_url) {
                    eprintln!("iframe: action '{get_url}' с target=_top не навигабелен — пропуск");
                    return;
                }
                // Адрес разрешается базой РЕБЁНКА, а уходит наверх: форму
                // написал ребёнок, а меняется документ страницы.
                let resolved = nav_base.resolve_str(get_url);
                self.navigate_to(PageSource::from_arg(Some(&resolved)));
            }
            LinkTarget::Frame(target_idx) => {
                if !links::is_navigable_href(get_url) {
                    eprintln!("iframe: action '{get_url}' внутри фрейма не навигабелен — пропуск");
                    return;
                }
                self.navigate_frame_to(target_idx, get_url, nav_base);
            }
        }
    }

    /// Подсказка о непройденной валидации для контрола под-документа.
    ///
    /// Прямоугольник ищется в layout РЕБЁНКА (`NodeId` уникален лишь внутри
    /// своего документа), а рисуется оверлей в координатах документа СТРАНИЦЫ —
    /// поэтому найденный бокс переводится в них
    /// [`frames::frame_page_origin`]. Оверлей намеренно не подрезается окном
    /// фрейма: подсказка валидации и в настоящем браузере рисуется поверх всего.
    fn show_frame_validation_tooltip(&mut self, idx: usize, control: NodeId) {
        let found = self.frames.get(idx).and_then(|h| {
            let doc = h.doc.lock().ok()?;
            let lb = h.layout.as_ref()?;
            forms::find_control_rect_and_error(lb, &doc, control)
        });
        let Some((rect, msg)) = found else { return };
        let Some((ox, oy)) = frames::frame_page_origin(&self.frames, idx) else {
            return;
        };
        let anchor = Rect {
            x: rect.x + ox,
            y: rect.y + oy,
            width: rect.width,
            height: rect.height,
        };
        self.validation_tooltip = Some((anchor, msg));
        self.request_redraw();
    }
}
