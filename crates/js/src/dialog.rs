//! Модальные диалоги страницы: `alert`/`confirm`/`prompt` и запрос `beforeunload`
//! (UX-DIALOGS).
//!
//! Нативная `_lumen_dialog` (V8-установка, `install/platform.rs`) вызывается на
//! потоке JS и **блокирует** его, пока пользователь не ответит в chrome-модалке.
//! Как у `clipboard`, посредник — process-global: диалогу не нужно состояние
//! рантайма, а протаскивать канал через каждую точку установки дороже пользы.
//!
//! Обмен: JS-поток кладёт [`DialogRequest`] в очередь и будит UI через
//! `wake`; UI забирает запрос [`take_pending`], показывает модалку и отвечает
//! [`answer`]. Пока хоть один запрос не отвечен, [`js_blocked`] истинно — UI
//! обязан не ходить в движковый поток блокирующими чтениями (они бы стояли до
//! `QUERY_TIMEOUT`, не давая нарисовать модалку).
//!
//! Без UI-стороны ([`install_ui`] не вызывался: тесты, dump-режимы) и при
//! вызове с самого UI-потока (блокировка была бы вечной) диалог не показывается и
//! отвечает значением «по умолчанию»: `alert` — принят, остальные — отклонены.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::ThreadId;
use std::time::Duration;

/// Вид диалога.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Alert,
    Confirm,
    Prompt,
    /// Запрос подтверждения ухода со страницы (`beforeunload`).
    BeforeUnload,
}

/// Запрос диалога от страницы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogRequest {
    pub id: u64,
    pub kind: DialogKind,
    pub message: String,
    /// Начальное значение поля `prompt`.
    pub default_value: String,
    /// Заголовок: источник страницы («example.com»).
    pub origin: String,
}

/// Ответ пользователя.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogReply {
    /// `true` — «OK» (для `beforeunload` — «Уйти»).
    pub accepted: bool,
    /// Введённый текст `prompt`.
    pub text: String,
}

impl DialogReply {
    pub fn dismissed() -> Self {
        Self { accepted: false, text: String::new() }
    }
}

type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct State {
    ui_thread: Option<ThreadId>,
    wake: Option<Wake>,
    queue: VecDeque<(DialogRequest, Sender<DialogReply>)>,
    active: Option<(u64, Sender<DialogReply>)>,
    next_id: u64,
    /// Запросы, чьи JS-потоки сейчас стоят в [`request`].
    waiting: usize,
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(|| Mutex::new(State::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Подключает UI-сторону. Вызывать с UI-потока; `wake` будит его цикл событий.
pub fn install_ui(wake: Arc<dyn Fn() + Send + Sync>) {
    let mut st = state();
    st.ui_thread = Some(std::thread::current().id());
    st.wake = Some(wake);
}

/// Отключает UI: все ожидающие диалоги отклоняются, JS-потоки просыпаются.
pub fn uninstall_ui() {
    let mut st = state();
    st.ui_thread = None;
    st.wake = None;
    for (_, tx) in st.queue.drain(..) {
        let _ = tx.send(DialogReply::dismissed());
    }
    if let Some((_, tx)) = st.active.take() {
        let _ = tx.send(DialogReply::dismissed());
    }
}

/// Истинно, пока какой-либо JS-поток ждёт ответа пользователя.
pub fn js_blocked() -> bool {
    state().waiting > 0
}

/// Ответ без UI: `alert` принят, остальное отклонено.
fn headless_reply(kind: DialogKind) -> DialogReply {
    DialogReply { accepted: kind == DialogKind::Alert, text: String::new() }
}

/// Показывает диалог и ждёт ответа. Вызывается на потоке JS.
pub fn request(kind: DialogKind, message: &str, default_value: &str, origin: &str) -> DialogReply {
    let (tx, rx): (Sender<DialogReply>, Receiver<DialogReply>) = mpsc::channel();
    let wake = {
        let mut st = state();
        let ui_here = st.ui_thread == Some(std::thread::current().id());
        if st.ui_thread.is_none() || ui_here {
            return headless_reply(kind);
        }
        st.next_id += 1;
        let req = DialogRequest {
            id: st.next_id,
            kind,
            message: message.to_owned(),
            default_value: default_value.to_owned(),
            origin: origin.to_owned(),
        };
        st.queue.push_back((req, tx));
        st.waiting += 1;
        st.wake.clone()
    };
    if let Some(w) = wake {
        w();
    }
    let reply = loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(r) => break r,
            Err(RecvTimeoutError::Timeout) => {
                if state().ui_thread.is_none() {
                    break headless_reply(kind);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break DialogReply::dismissed(),
        }
    };
    state().waiting -= 1;
    reply
}

/// UI забирает следующий неотвеченный запрос. Пока предыдущий не получил
/// [`answer`], новый не выдаётся.
pub fn take_pending() -> Option<DialogRequest> {
    let mut st = state();
    if st.active.is_some() {
        return None;
    }
    let (req, tx) = st.queue.pop_front()?;
    st.active = Some((req.id, tx));
    Some(req)
}

/// Ответ пользователя на выданный [`take_pending`] запрос `id`.
pub fn answer(id: u64, reply: DialogReply) {
    let mut st = state();
    if st.active.as_ref().is_some_and(|(aid, _)| *aid == id)
        && let Some((_, tx)) = st.active.take()
    {
        let _ = tx.send(reply);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn no_ui_answers_defaults() {
        let _g = serial();
        uninstall_ui();
        assert!(request(DialogKind::Alert, "a", "", "o").accepted);
        assert!(!request(DialogKind::Confirm, "a", "", "o").accepted);
        assert!(!request(DialogKind::Prompt, "a", "d", "o").accepted);
    }

    #[test]
    fn ui_thread_call_does_not_deadlock() {
        let _g = serial();
        install_ui(Arc::new(|| {}));
        assert!(!request(DialogKind::Confirm, "a", "", "o").accepted);
        uninstall_ui();
    }

    #[test]
    fn js_thread_blocks_until_answered() {
        let _g = serial();
        install_ui(Arc::new(|| {}));
        let js = std::thread::spawn(|| request(DialogKind::Prompt, "name?", "def", "example.com"));
        let req = loop {
            if let Some(r) = take_pending() {
                break r;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(req.message, "name?");
        assert_eq!(req.default_value, "def");
        assert!(js_blocked());
        answer(req.id, DialogReply { accepted: true, text: "Anna".into() });
        let reply = js.join().expect("js thread");
        assert_eq!(reply, DialogReply { accepted: true, text: "Anna".into() });
        assert!(!js_blocked());
        uninstall_ui();
    }

    #[test]
    fn uninstall_releases_waiting_js() {
        let _g = serial();
        install_ui(Arc::new(|| {}));
        let js = std::thread::spawn(|| request(DialogKind::Confirm, "x", "", "o"));
        while !js_blocked() {
            std::thread::sleep(Duration::from_millis(5));
        }
        uninstall_ui();
        assert!(!js.join().expect("js thread").accepted);
    }
}
