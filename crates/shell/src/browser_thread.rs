//! Поток браузера (ADR-032, срез 2): состояние [`Lumen`] живёт не на главном
//! потоке процесса, а на отдельном, а главный поток остаётся тонким
//! переходником winit.
//!
//! Главный поток качает сообщения ОС и больше ничего: каждое `WindowEvent`,
//! `DeviceEvent`, `resumed` и пользовательское событие уходит в канал
//! [`UiMsg`] в порядке прихода. Операции, которым нужен `ActiveEventLoop`
//! (выход, создание окна), поток браузера просит у главного через
//! `EventLoopProxy` — это варианты [`LoadEvent::MainExit`] и
//! [`LoadEvent::MainCreateWindow`]. `Lumen` строится на потоке браузера и
//! границ потоков не пересекает, поэтому `Send` ему не нужен.
//!
//! `LUMEN_NO_BROWSER_THREAD=1` возвращает прежнюю схему: `Lumen` сам
//! `ApplicationHandler` на главном потоке, а [`MainHandle::Direct`] оборачивает
//! настоящий `ActiveEventLoop`.

use std::cell::Cell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use crate::*;

/// Что главный поток передаёт потоку браузера.
pub(crate) enum UiMsg {
    Resumed,
    Window(WindowId, WindowEvent),
    Device(DeviceEvent),
    User(LoadEvent),
    /// Рендер-поток вернул смещение страницы (ADR-032, срез 3).
    ScrollFeedback,
}

/// Запрос на создание окна, исполняемый главным потоком.
pub(crate) struct CreateWindowRequest {
    pub(crate) attrs: winit::window::WindowAttributes,
    pub(crate) reply: Sender<Result<lumen_paint::SurfaceWindow, String>>,
}

/// Создаёт окно и сразу снимает его сырые дескрипторы — это возможно только на
/// главном потоке (см. `lumen_paint::SurfaceWindow`).
fn create_surface_window(
    el: &ActiveEventLoop,
    attrs: winit::window::WindowAttributes,
) -> Result<lumen_paint::SurfaceWindow, String> {
    let window = el.create_window(attrs).map_err(|e| e.to_string())?;
    lumen_paint::SurfaceWindow::new(window).map_err(|e| e.to_string())
}

/// Рычаг отката: прежняя схема без потока браузера.
pub(crate) fn browser_thread_disabled() -> bool {
    std::env::var_os("LUMEN_NO_BROWSER_THREAD").is_some_and(|v| v != "0")
}

/// Замена `&ActiveEventLoop` для кода, живущего на потоке браузера.
pub(crate) enum MainHandle<'a> {
    /// Прежний режим: код выполняется внутри колбэка winit на главном потоке.
    Direct(&'a ActiveEventLoop),
    /// Поток браузера: операции уходят главному потоку через прокси.
    Remote(RemoteMain),
}

pub(crate) struct RemoteMain {
    proxy: EventLoopProxy<LoadEvent>,
    exit: Cell<bool>,
    flow: Cell<ControlFlow>,
}

impl MainHandle<'static> {
    pub(crate) fn remote(proxy: EventLoopProxy<LoadEvent>) -> Self {
        MainHandle::Remote(RemoteMain {
            proxy,
            exit: Cell::new(false),
            flow: Cell::new(ControlFlow::Wait),
        })
    }
}

impl MainHandle<'_> {
    /// Завершить цикл: сразу в прежнем режиме, после возврата из текущего
    /// обработчика — в режиме потока браузера.
    pub(crate) fn exit(&self) {
        match self {
            MainHandle::Direct(el) => el.exit(),
            MainHandle::Remote(r) => r.exit.set(true),
        }
    }

    pub(crate) fn exit_requested(&self) -> bool {
        matches!(self, MainHandle::Remote(r) if r.exit.get())
    }

    /// В режиме потока браузера блокируется, пока главный поток не ответит;
    /// главный поток на поток браузера никогда не ждёт, так что взаимной
    /// блокировки нет.
    pub(crate) fn create_window(
        &self,
        attrs: winit::window::WindowAttributes,
    ) -> Result<lumen_paint::SurfaceWindow, String> {
        match self {
            MainHandle::Direct(el) => create_surface_window(el, attrs),
            MainHandle::Remote(r) => {
                let (reply, rx) = mpsc::channel();
                let req = Box::new(CreateWindowRequest { attrs, reply });
                if r.proxy.send_event(LoadEvent::MainCreateWindow(req)).is_err() {
                    return Err("главный поток уже завершил цикл".to_owned());
                }
                rx.recv().unwrap_or_else(|_| Err("главный поток не ответил".to_owned()))
            }
        }
    }

    pub(crate) fn set_control_flow(&self, flow: ControlFlow) {
        match self {
            MainHandle::Direct(el) => el.set_control_flow(flow),
            MainHandle::Remote(r) => r.flow.set(flow),
        }
    }

    pub(crate) fn control_flow(&self) -> ControlFlow {
        match self {
            MainHandle::Direct(el) => el.control_flow(),
            MainHandle::Remote(r) => r.flow.get(),
        }
    }
}

impl Lumen {
    /// Одна точка входа для обоих режимов: колбэки winit (прежний режим) и
    /// цикл потока браузера.
    pub(crate) fn dispatch_ui_msg(&mut self, h: &MainHandle<'_>, msg: UiMsg) {
        match msg {
            UiMsg::Resumed => self.on_resumed(h),
            UiMsg::Window(id, ev) => self.on_window_event(h, id, ev),
            UiMsg::Device(ev) => self.on_device_event(ev),
            UiMsg::User(ev) => self.on_user_event(ev),
            UiMsg::ScrollFeedback => {
                self.adopt_scroll_feedback();
            }
        }
    }

    /// Цикл потока браузера: заменяет пампинг winit. Холостой ход — `recv`
    /// без таймаута или до ближайшего дедлайна `about_to_wait`, так что
    /// простой CPU остаётся ≈ 0 % (ADR-016, инвариант 6).
    pub(crate) fn run_browser_loop(&mut self, h: &MainHandle<'_>, rx: &Receiver<UiMsg>) {
        loop {
            let first = match h.control_flow() {
                ControlFlow::Wait => match rx.recv() {
                    Ok(m) => Some(m),
                    Err(_) => break,
                },
                ControlFlow::WaitUntil(t) => {
                    match rx.recv_timeout(t.saturating_duration_since(Instant::now())) {
                        Ok(m) => Some(m),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                ControlFlow::Poll => rx.try_recv().ok(),
            };
            if let Some(m) = first {
                self.dispatch_ui_msg(h, m);
                while !h.exit_requested()
                    && let Ok(m) = rx.try_recv()
                {
                    self.dispatch_ui_msg(h, m);
                }
            }
            if h.exit_requested() {
                break;
            }
            self.on_about_to_wait(h);
            self.publish_scroll_snapshot();
            if h.exit_requested() {
                break;
            }
        }
    }
}

/// Главный поток: только пересылка. Любое событие, которому нужен
/// `ActiveEventLoop`, обслуживается здесь же.
pub(crate) struct MainForwarder {
    pub(crate) tx: Sender<UiMsg>,
    /// Колесо над страницей — мимо потока браузера (ADR-032, срез 3).
    pub(crate) wheel: crate::wheel_scroll::WheelRouter,
}

impl MainForwarder {
    fn send(&self, el: &ActiveEventLoop, msg: UiMsg) {
        if self.tx.send(msg).is_err() {
            // Поток браузера завершился (штатно или паникой) — держать цикл незачем.
            el.exit();
        }
    }
}

impl ApplicationHandler<LoadEvent> for MainForwarder {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.send(el, UiMsg::Resumed);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: LoadEvent) {
        match event {
            LoadEvent::MainExit => el.exit(),
            LoadEvent::MainCreateWindow(req) => {
                let CreateWindowRequest { attrs, reply } = *req;
                let _ = reply.send(create_surface_window(el, attrs));
            }
            other => self.send(el, UiMsg::User(other)),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.wheel.observe(&event);
        if let WindowEvent::MouseWheel { delta, phase, .. } = &event
            && self.wheel.route(*delta, *phase)
        {
            return;
        }
        self.send(el, UiMsg::Window(id, event));
    }

    fn device_event(&mut self, el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        self.send(el, UiMsg::Device(event));
    }
}

/// Сторож потока браузера: при любом выходе (в том числе панике) просит
/// главный поток закончить цикл, иначе окно осталось бы жить без хозяина.
pub(crate) struct ExitMainOnDrop(pub(crate) EventLoopProxy<LoadEvent>);

impl Drop for ExitMainOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send_event(LoadEvent::MainExit);
    }
}
