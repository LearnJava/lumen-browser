//! Колесо над страницей мимо потока браузера (ADR-032, срез 3).
//!
//! Главный поток (тонкий переходник winit) решает по [`ScrollSnapshot`], можно
//! ли отдать колесо/тачпад прямо рендер-потоку, и если можно — шлёт ему
//! [`WheelInput`], не трогая поток браузера. Рендер-поток ведёт смещение
//! страницы сам (кривая щелчка, инерция, клэмп) и возвращает его потоку
//! браузера через [`ScrollShared`] (последнее значение побеждает). Всё, что
//! снимок не покрывает — панели, split view, overflow-контейнеры, фреймы,
//! scroll-snap, область вне страницы, — уходит потоку браузера как раньше.
//!
//! Снимок публикует поток браузера (`Lumen::publish_scroll_snapshot`) после
//! каждой пачки сообщений, если он изменился. Обратная связь несёт номер
//! поколения: поток браузера усыновляет его (`Lumen::adopt_scroll_feedback`),
//! а кадры, снятые до усыновления, рендер-поток дорисовывает со своим
//! смещением, а не со старым смещением потока браузера.
//!
//! `LUMEN_NO_WHEEL_ROUTE=1` отключает маршрутизацию (колесо идёт потоку
//! браузера, как в срезе 2).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use lumen_layout::ScrollContainer;
use winit::event::{MouseScrollDelta, TouchPhase, WindowEvent};

use crate::browser_thread::UiMsg;
use crate::render_thread::RenderLink;

/// Шаг одного щелчка колеса, CSS px (как в `on_mouse_wheel`).
pub(crate) const LINE_STEP_PX: f32 = 40.0;

/// Рычаг отката маршрутизации колеса.
pub(crate) fn wheel_route_disabled() -> bool {
    static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DISABLED.get_or_init(|| std::env::var_os("LUMEN_NO_WHEEL_ROUTE").is_some_and(|v| v != "0"))
}

/// Неизменяемый снимок того, что нужно главному потоку для решения «колесо
/// идёт рендер-потоку или потоку браузера».
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct ScrollSnapshot {
    /// `false` — всё колесо идёт потоку браузера (панель, split view, snap…).
    pub(crate) enabled: bool,
    /// Коэффициент масштаба окна: курсор приходит в физических пикселях.
    pub(crate) dpr: f32,
    /// Область страницы в CSS px окна: `[x0, y0, x1, y1]`.
    pub(crate) viewport: [f32; 4],
    /// Начало документа в CSS px окна (`page_offset`).
    pub(crate) origin: (f32, f32),
    /// Предел вертикального смещения страницы, CSS px.
    pub(crate) max_y: f32,
    /// Предел горизонтального смещения страницы, CSS px.
    pub(crate) max_x: f32,
    /// Прямоугольники документа `[x, y, w, h]` фреймов: колесо над ними крутит
    /// под-документ, это решает поток браузера.
    pub(crate) blockers: Vec<[f32; 4]>,
    /// Overflow-контейнеры страницы (ADR-032, срез 4): цель колеса и цепочка
    /// прокрутки решаются по ним на рендер-потоке, который ведёт их смещения.
    pub(crate) containers: Vec<ScrollContainer>,
    /// Поколение смещения рендер-потока, которое поток браузера усыновил к
    /// моменту снимка: смещения контейнеров, усыновленные раньше, уже в `containers`.
    pub(crate) adopted_gen: u64,
    /// Зарезервировано под области с неpassive-слушателем `wheel` (ADR-032,
    /// правило 9, BUG-865): JS-событий `wheel` пока нет, поле всегда пусто.
    pub(crate) wheel_listeners: Vec<[f32; 4]>,
}

fn rect_has(r: &[f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
}

impl ScrollSnapshot {
    /// Курсор (физические px окна) над страницей и вне областей, которые
    /// решает поток браузера. `offset` — текущее смещение страницы.
    pub(crate) fn page_wheel_at(&self, cursor: (f64, f64), offset: (f32, f32)) -> bool {
        if !self.enabled || self.dpr <= 0.0 {
            return false;
        }
        let cx = cursor.0 as f32 / self.dpr;
        let cy = cursor.1 as f32 / self.dpr;
        let [x0, y0, x1, y1] = self.viewport;
        if cx < x0 || cx >= x1 || cy < y0 || cy >= y1 {
            return false;
        }
        let dx = cx - self.origin.0 + offset.0;
        let dy = cy - self.origin.1 + offset.1;
        !self.blockers.iter().any(|r| rect_has(r, dx, dy))
            && !self.wheel_listeners.iter().any(|r| rect_has(r, dx, dy))
    }
}

/// Что главный поток передаёт рендер-потоку. Дельты уже в CSS px, со знаком
/// «вниз/вправо — плюс» и с применённым Shift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum WheelInput {
    /// Щелчок колеса мыши: кривая по Y, мгновенный сдвиг по X.
    Notch { dx: f32, dy: f32 },
    /// Тачпад: палец лёг.
    TouchStart { dx: f32, dy: f32 },
    /// Тачпад: палец движется.
    TouchMove { dx: f32, dy: f32 },
    /// Тачпад: палец снят — запустить инерцию, если она есть.
    TouchEnd,
    /// Тачпад: жест отменён.
    TouchCancel,
}

/// Смещение страницы, которое рендер-поток вернул потоку браузера.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScrollFeedback {
    /// Поколение: растёт с каждым изменением, принадлежащим рендер-потоку.
    pub(crate) gen_id: u64,
    /// Эпоха последней программной прокрутки, которую рендер-поток уже принял
    /// (ADR-032, правило 7): связь старее эпохи потока браузера устарела.
    pub(crate) cmd_epoch: u64,
    pub(crate) y: f32,
    pub(crate) x: f32,
    /// Смещения overflow-контейнеров, которые ведёт рендер-поток и поток
    /// браузера ещё не усыновил: `(id слоя, x, y)`.
    pub(crate) containers: Vec<(u32, f32, f32)>,
}

/// Смещение overflow-контейнера, которое ведёт рендер-поток (ADR-032, срез 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ContainerOffset {
    /// `PushScrollLayer::id` — индекс узла контейнера.
    pub(crate) id: u32,
    pub(crate) x: f32,
    pub(crate) y: f32,
    /// Поколение обратной связи, с которым смещение ушло потоку браузера.
    pub(crate) gen_id: u64,
}

/// Цель дельты колеса среди контейнеров снимка с учётом смещений, которыми
/// владеет рендер-поток: тот же ход по цепочке прокрутки и тот же
/// `overscroll-behavior`, что у потока браузера (правило 8 ADR-032 — одна
/// чистая функция, `lumen_layout::resolve_scroll_chain_target`). `doc` — точка
/// документа. `None` — контейнера нет и колесо крутит страницу.
pub(crate) fn resolve_container_wheel(
    snap: &ScrollSnapshot,
    owned: &[ContainerOffset],
    doc: (f32, f32),
    dx: f32,
    dy: f32,
) -> Option<lumen_layout::ScrollChainTarget> {
    if snap.containers.is_empty() {
        return None;
    }
    let with_offsets: Vec<ScrollContainer> = snap
        .containers
        .iter()
        .map(|c| {
            let mut c = c.clone();
            if let Some(o) = owned.iter().find(|o| o.id == c.node.index() as u32) {
                c.scroll_x = o.x;
                c.scroll_y = o.y;
            }
            c
        })
        .collect();
    lumen_layout::resolve_scroll_chain_target(&with_offsets, doc.0, doc.1, dx, dy)
}

/// Состояние, разделяемое главным потоком, потоком браузера и рендер-потоком.
pub(crate) struct ScrollShared {
    snapshot: Mutex<Arc<ScrollSnapshot>>,
    link: Mutex<Option<RenderLink>>,
    /// Последнее смещение страницы, известное рендер-потоку — нужно главному
    /// потоку, чтобы перевести курсор в координаты документа.
    offset: Mutex<(f32, f32)>,
    feedback: Mutex<Option<ScrollFeedback>>,
    /// Потоку браузера уже послано пробуждение и он ещё не забрал обратную связь.
    pending: AtomicBool,
    wake: Sender<UiMsg>,
}

impl ScrollShared {
    pub(crate) fn new(wake: Sender<UiMsg>) -> Arc<Self> {
        Arc::new(Self {
            snapshot: Mutex::new(Arc::new(ScrollSnapshot::default())),
            link: Mutex::new(None),
            offset: Mutex::new((0.0, 0.0)),
            feedback: Mutex::new(None),
            pending: AtomicBool::new(false),
            wake,
        })
    }

    pub(crate) fn publish_snapshot(&self, s: ScrollSnapshot) {
        let s = Arc::new(s);
        if let Ok(mut g) = self.snapshot.lock() {
            *g = Arc::clone(&s);
        }
        // Рендер-поток решает цель колеса по контейнерам снимка сам.
        if let Some(link) = self.link() {
            link.send_snapshot(s);
        }
    }

    fn snapshot(&self) -> Arc<ScrollSnapshot> {
        self.snapshot.lock().map(|g| Arc::clone(&g)).unwrap_or_default()
    }

    pub(crate) fn set_link(&self, link: Option<RenderLink>) {
        if let Ok(mut g) = self.link.lock() {
            *g = link;
        }
    }

    fn link(&self) -> Option<RenderLink> {
        self.link.lock().ok().and_then(|g| g.clone())
    }

    /// Рендер-поток: запомнить смещение, с которым он рисует.
    pub(crate) fn set_offset(&self, y: f32, x: f32) {
        if let Ok(mut g) = self.offset.lock() {
            *g = (x, y);
        }
    }

    fn offset(&self) -> (f32, f32) {
        self.offset.lock().map(|g| *g).unwrap_or_default()
    }

    /// Рендер-поток: вернуть смещение потоку браузера и разбудить его, если
    /// предыдущая обратная связь уже забрана.
    pub(crate) fn post_feedback(&self, fb: ScrollFeedback) {
        self.set_offset(fb.y, fb.x);
        if let Ok(mut g) = self.feedback.lock() {
            *g = Some(fb);
        }
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.wake.send(UiMsg::ScrollFeedback);
        }
    }

    /// Поток браузера: забрать последнюю обратную связь. Флаг ожидания
    /// сбрасывается раньше чтения, поэтому поздняя публикация разбудит снова.
    pub(crate) fn take_feedback(&self) -> Option<ScrollFeedback> {
        self.pending.store(false, Ordering::Release);
        self.feedback.lock().ok().and_then(|mut g| g.take())
    }
}

/// Куда ушёл жест тачпада — чтобы все его фазы шли одному адресату.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Sink {
    Render,
    Browser,
}

/// Главный поток: следит за курсором и модификаторами и решает судьбу колеса.
pub(crate) struct WheelRouter {
    shared: Arc<ScrollShared>,
    cursor: Option<(f64, f64)>,
    shift: bool,
    gesture: Option<Sink>,
}

impl WheelRouter {
    pub(crate) fn new(shared: Arc<ScrollShared>) -> Self {
        Self { shared, cursor: None, shift: false, gesture: None }
    }

    /// Запоминает курсор и Shift; событие всё равно уходит потоку браузера.
    pub(crate) fn observe(&mut self, ev: &WindowEvent) {
        match ev {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::ModifiersChanged(m) => self.shift = m.state().shift_key(),
            _ => {}
        }
    }

    /// Курсор в CSS px от начала области страницы: рендер-поток прибавляет своё
    /// смещение и получает точку документа для хит-теста контейнеров.
    fn viewport_point(&self, snap: &ScrollSnapshot) -> Option<(f32, f32)> {
        let (cx, cy) = self.cursor?;
        let dpr = snap.dpr.max(1e-6);
        Some((cx as f32 / dpr - snap.origin.0, cy as f32 / dpr - snap.origin.1))
    }

    /// `true` — колесо отдано рендер-потоку, потоку браузера слать не нужно.
    pub(crate) fn route(&mut self, delta: MouseScrollDelta, phase: TouchPhase) -> bool {
        if wheel_route_disabled() {
            return false;
        }
        let snap = self.shared.snapshot();
        let Some(link) = self.shared.link() else {
            return false;
        };
        let at_page = |cursor: Option<(f64, f64)>| {
            cursor.is_some_and(|c| snap.page_wheel_at(c, self.shared.offset()))
        };
        let swap = |dx: f32, dy: f32, shift: bool| if shift { (dy, 0.0) } else { (dx, dy) };
        match delta {
            MouseScrollDelta::LineDelta(cols, lines) => {
                self.gesture = None;
                if !at_page(self.cursor) {
                    return false;
                }
                crate::present_log::wheel(lines);
                let (dx, dy) = swap(-cols * LINE_STEP_PX, -lines * LINE_STEP_PX, self.shift);
                link.send_wheel(WheelInput::Notch { dx, dy }, snap.max_y, snap.max_x, self.viewport_point(&snap));
                true
            }
            MouseScrollDelta::PixelDelta(p) => {
                let dpr = snap.dpr.max(1e-6);
                let (dx, dy) = swap(-(p.x as f32) / dpr, -(p.y as f32) / dpr, self.shift);
                let sink = match phase {
                    TouchPhase::Started => {
                        let s = if at_page(self.cursor) { Sink::Render } else { Sink::Browser };
                        self.gesture = Some(s);
                        s
                    }
                    TouchPhase::Moved => *self.gesture.get_or_insert_with(|| {
                        if at_page(self.cursor) { Sink::Render } else { Sink::Browser }
                    }),
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.gesture.take().unwrap_or(Sink::Browser)
                    }
                };
                if sink != Sink::Render {
                    return false;
                }
                crate::present_log::wheel(p.y as f32);
                let input = match phase {
                    TouchPhase::Started => WheelInput::TouchStart { dx, dy },
                    TouchPhase::Moved => WheelInput::TouchMove { dx, dy },
                    TouchPhase::Ended => WheelInput::TouchEnd,
                    TouchPhase::Cancelled => WheelInput::TouchCancel,
                };
                link.send_wheel(input, snap.max_y, snap.max_x, self.viewport_point(&snap));
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> ScrollSnapshot {
        ScrollSnapshot {
            enabled: true,
            dpr: 2.0,
            viewport: [0.0, 100.0, 800.0, 700.0],
            origin: (0.0, 100.0),
            max_y: 5000.0,
            max_x: 0.0,
            blockers: vec![[50.0, 1000.0, 200.0, 100.0]],
            containers: Vec::new(),
            adopted_gen: 0,
            wheel_listeners: vec![],
        }
    }

    #[test]
    fn page_area_accepts_wheel() {
        // CSS (400, 300) → физические (800, 600).
        assert!(snap().page_wheel_at((800.0, 600.0), (0.0, 0.0)));
    }

    #[test]
    fn chrome_area_goes_to_browser_thread() {
        // CSS y = 50 лежит над областью страницы (тулбар).
        assert!(!snap().page_wheel_at((800.0, 100.0), (0.0, 0.0)));
    }

    #[test]
    fn frame_under_cursor_goes_to_browser_thread() {
        // Документная точка (100, 1050): окно (100, 100 + 1050 - 1000 = 150) при scroll 1000.
        let s = snap();
        assert!(!s.page_wheel_at((200.0, 300.0), (0.0, 1000.0)));
        // Без прокрутки та же точка окна — обычная страница.
        assert!(s.page_wheel_at((200.0, 300.0), (0.0, 0.0)));
    }

    #[test]
    fn disabled_snapshot_never_routes() {
        let mut s = snap();
        s.enabled = false;
        assert!(!s.page_wheel_at((800.0, 600.0), (0.0, 0.0)));
    }

    #[test]
    fn wheel_listener_region_goes_to_browser_thread() {
        let mut s = snap();
        s.wheel_listeners.push([0.0, 0.0, 800.0, 400.0]);
        assert!(!s.page_wheel_at((800.0, 600.0), (0.0, 0.0)));
    }

    #[test]
    fn feedback_wakes_once_until_taken() {
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = ScrollShared::new(tx);
        shared.post_feedback(ScrollFeedback { gen_id: 1, cmd_epoch: 0, y: 10.0, x: 0.0, containers: Vec::new() });
        shared.post_feedback(ScrollFeedback { gen_id: 2, cmd_epoch: 0, y: 20.0, x: 0.0, containers: Vec::new() });
        assert!(matches!(rx.try_recv(), Ok(UiMsg::ScrollFeedback)));
        assert!(rx.try_recv().is_err(), "второе пробуждение не нужно");
        let fb = shared.take_feedback().expect("feedback");
        assert_eq!(fb.gen_id, 2, "побеждает последнее значение");
        shared.post_feedback(ScrollFeedback { gen_id: 3, cmd_epoch: 0, y: 30.0, x: 0.0, containers: Vec::new() });
        assert!(matches!(rx.try_recv(), Ok(UiMsg::ScrollFeedback)));
    }
}
