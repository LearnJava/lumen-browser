//! Модальные диалоги страницы (UX-DIALOGS): `alert`/`confirm`/`prompt` и
//! подтверждение ухода со страницы.
//!
//! JS-поток стоит в `lumen_js::dialog::request`, пока UI не ответит; здесь
//! запрос забирается из очереди, показывается как `#dialogBox` (плавающая
//! панель хрома) и отвечается кнопками или клавишами. Пока диалог открыт,
//! страница и остальной хром не получают ввода.

use crate::*;
use lumen_js::dialog::{self, DialogKind, DialogReply, DialogRequest};

/// Открытый диалог и введённый текст `prompt`.
pub(crate) struct PageDialog {
    pub(crate) req: DialogRequest,
    pub(crate) input: String,
}

/// Уход со страницы, отложенный до ответа на `beforeunload`-диалог.
pub(crate) enum PendingLeave {
    Navigate(PageSource),
    Back,
    Forward,
    CloseTab(usize),
    CloseWindow,
}

/// Идентификатор запроса, который шелл создаёт сам (не из `lumen_js::dialog`,
/// чьи номера начинаются с 1): ответ уходит не JS-потоку, а в [`PendingLeave`].
const LOCAL_REQUEST_ID: u64 = 0;

/// Как долго UI ждёт ответа движка на `beforeunload`; дальше — уходим без вопроса,
/// чтобы занятый JS-поток не вешал навигацию.
const BEFOREUNLOAD_WAIT: std::time::Duration = std::time::Duration::from_millis(150);

impl Lumen {
    /// Подключает посредника к циклу событий этого окна (один раз) и забирает
    /// новый запрос, если диалога сейчас нет. Зовётся из `about_to_wait`.
    pub(crate) fn poll_page_dialog(&mut self) {
        if !self.page_dialog_wired {
            self.page_dialog_wired = true;
            let wake = self.load_proxy.clone();
            dialog::install_ui(std::sync::Arc::new(move || {
                let _ = wake.send_event(LoadEvent::AutomationWake);
            }));
        }
        if self.page_dialog.is_some() {
            return;
        }
        if let Some(req) = dialog::take_pending() {
            let input = req.default_value.clone();
            self.page_dialog = Some(PageDialog { req, input });
            self.relayout_chrome_host();
            self.request_redraw();
        }
    }

    /// Модель `#dialogBox` для хрома.
    pub(crate) fn page_dialog_model(&self) -> lumen_chrome::ChromeDialogModel {
        let Some(d) = self.page_dialog.as_ref() else {
            return lumen_chrome::ChromeDialogModel::default();
        };
        let origin = if d.req.origin.is_empty() { "Страница".to_owned() } else { d.req.origin.clone() };
        match d.req.kind {
            DialogKind::Alert => lumen_chrome::ChromeDialogModel {
                open: true,
                title: format!("{origin} сообщает"),
                message: d.req.message.clone(),
                input: None,
                show_cancel: false,
                ok_label: "OK".into(),
                cancel_label: String::new(),
            },
            DialogKind::Confirm => lumen_chrome::ChromeDialogModel {
                open: true,
                title: format!("{origin} спрашивает"),
                message: d.req.message.clone(),
                input: None,
                show_cancel: true,
                ok_label: "OK".into(),
                cancel_label: "Отмена".into(),
            },
            DialogKind::Prompt => lumen_chrome::ChromeDialogModel {
                open: true,
                title: format!("{origin} спрашивает"),
                message: d.req.message.clone(),
                input: Some(format!("{}|", d.input)),
                show_cancel: true,
                ok_label: "OK".into(),
                cancel_label: "Отмена".into(),
            },
            DialogKind::BeforeUnload => lumen_chrome::ChromeDialogModel {
                open: true,
                title: "Покинуть сайт?".into(),
                message: if d.req.message.is_empty() {
                    "Внесённые изменения могут не сохраниться.".into()
                } else {
                    d.req.message.clone()
                },
                input: None,
                show_cancel: true,
                ok_label: "Уйти".into(),
                cancel_label: "Остаться".into(),
            },
        }
    }

    /// Закрывает диалог и отвечает странице (или продолжает отложенный уход).
    pub(crate) fn answer_page_dialog(&mut self, accepted: bool, event_loop: &crate::browser_thread::MainHandle<'_>) {
        let Some(d) = self.page_dialog.take() else { return };
        self.relayout_chrome_host();
        self.request_redraw();
        if d.req.id == LOCAL_REQUEST_ID {
            if let Some(leave) = self.pending_leave.take()
                && accepted
            {
                self.leave_confirmed = true;
                match leave {
                    PendingLeave::Navigate(source) => self.navigate_to(source),
                    PendingLeave::Back => self.navigate_back(),
                    PendingLeave::Forward => self.navigate_forward(),
                    PendingLeave::CloseTab(idx) => self.close_tab(idx, event_loop),
                    PendingLeave::CloseWindow => {
                        self.save_session_on_close();
                        self.save_full_session();
                        event_loop.exit();
                    }
                }
                self.leave_confirmed = false;
            }
            return;
        }
        let text = if accepted && d.req.kind == DialogKind::Prompt { d.input } else { String::new() };
        dialog::answer(d.req.id, DialogReply { accepted, text });
        // Следующий запрос из очереди показываем сразу, не дожидаясь тика.
        self.poll_page_dialog();
    }

    /// «Prompt to unload» (HTML LS §7.4.5) перед уходом со страницы: страница
    /// получает `beforeunload` один раз; если она просит остаться и пользователь с
    /// ней взаимодействовал, открывается диалог, а `leave` ждёт ответа.
    ///
    /// `None` — уходить нельзя (диалог открыт, вызывающий возвращается).
    /// `Some(fired)` — уходить можно; `fired` — `beforeunload` уже доставлен, и
    /// повторять его в последовательности выгрузки не нужно.
    pub(crate) fn beforeunload_gate(&mut self, leave: PendingLeave) -> Option<bool> {
        if std::mem::take(&mut self.leave_confirmed) {
            return Some(true);
        }
        let asks = match self.engine_thread.as_ref() {
            Some(engine) => engine.query_within(BEFOREUNLOAD_WAIT, |state| {
                state.js.as_ref().map(|j| j.beforeunload_wants_prompt())
            }),
            None => self.js_ctx.as_ref().map(|j| Some(j.beforeunload_wants_prompt())),
        };
        let Some(Some(asks)) = asks else { return Some(false) };
        if !asks {
            return Some(true);
        }
        let origin = self
            .source
            .url_str()
            .and_then(|u| u.split("://").nth(1))
            .and_then(|rest| rest.split('/').next())
            .unwrap_or_default()
            .to_owned();
        self.pending_leave = Some(leave);
        self.page_dialog = Some(PageDialog {
            req: DialogRequest {
                id: LOCAL_REQUEST_ID,
                kind: DialogKind::BeforeUnload,
                message: String::new(),
                default_value: String::new(),
                origin,
            },
            input: String::new(),
        });
        self.relayout_chrome_host();
        self.request_redraw();
        None
    }

    /// Клавиши модального диалога: Enter — OK, Esc — отмена, текст — в поле
    /// `prompt`. Возвращает `true` всегда, пока диалог открыт: ввод не уходит
    /// ни странице, ни остальному хрому.
    pub(crate) fn handle_page_dialog_key(
        &mut self,
        code: KeyCode,
        key_event: &KeyEvent,
        event_loop: &crate::browser_thread::MainHandle<'_>,
    ) -> bool {
        let Some(d) = self.page_dialog.as_mut() else { return false };
        if key_event.state != ElementState::Pressed {
            return true;
        }
        let is_prompt = d.req.kind == DialogKind::Prompt;
        match code {
            KeyCode::Enter | KeyCode::NumpadEnter => self.answer_page_dialog(true, event_loop),
            KeyCode::Escape => {
                let cancels = d.req.kind != DialogKind::Alert;
                self.answer_page_dialog(!cancels, event_loop);
            }
            KeyCode::Backspace if is_prompt => {
                d.input.pop();
                self.relayout_chrome_host();
                self.request_redraw();
            }
            _ if is_prompt
                && !self.modifiers.control_key()
                && let Some(text) = key_event.text.as_ref()
                && !text.is_empty()
                && !text.chars().any(char::is_control) =>
            {
                d.input.push_str(text);
                self.relayout_chrome_host();
                self.request_redraw();
            }
            _ => {}
        }
        true
    }
}
