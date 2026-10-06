//! Threaded render backend (ADR-016 M1 — spike / первый срез).
//!
//! [`ThreadedRenderBackend`] реализует [`RenderBackend`] и работает как
//! **прокси** к настоящему GPU-бэкенду (femtovg/wgpu), который создаётся и
//! живёт на выделенном рендер-потоке. Главный поток шлёт команды через канал:
//! кадры (`render`) — по модели «последний выигрывает» (latest-wins,
//! coalescing — устаревшие кадры отбрасываются, а не копятся в очереди),
//! управляющие вызовы (resize / scale / изображения / шрифты) — в строгом
//! порядке. Present (`swap_buffers`, который блокируется на vsync) уходит с
//! UI-потока — это и есть суть M1.
//!
//! # Почему прокси через сам трейт, а не отдельный путь в shell
//!
//! `RenderBackend` — уже стабильная граница движка. Реализовав его прокси, мы
//! переносим бэкенд на отдельный поток **без единой правки** в 12k-строчном
//! горячем блоке `RedrawRequested` (`main.rs`): shell по-прежнему держит
//! `Box<dyn RenderBackend>` и вызывает те же методы. Это ровно тот
//! «backend-owning boundary», о котором просит ADR-016 (не рефакторить shell).
//!
//! # Инварианты ADR-016, которые соблюдает этот срез
//!
//! - **Cross-thread data = immutable snapshots.** Кадр посылается как
//!   владеющая копия команд (`Vec<DisplayCommand>`); рендер-поток не разделяет
//!   мутабельное состояние с main.
//! - **Latest-wins, queue depth 1, coalescing.** Дренаж канала оставляет только
//!   последний кадр в пачке; промежуточные отбрасываются.
//! - **Idle = parked on condvar.** Поток спит на блокирующем `recv()`; без
//!   команд не крутит CPU (инвариант 6 — сохраняется ~0% idle из BUG-271).
//! - **Render thread never waits for the engine.** `render()` на прокси —
//!   fire-and-forget: кладёт кадр и сразу возвращает `Ok`.
//!
//! # Известные ограничения этого среза (честно, до дальнейших M1/M2)
//!
//! - Momentum-скролл (M1.3) рендер-поток продолжает сам, когда UI-поток
//!   застопорился (см. ниже). Прочие анимации (CSS/GIF/rAF) по-прежнему тикают
//!   на main, и события ввода, пришедшие во время застоя, обрабатываются только
//!   после него — полная независимость ввода — M2.
//! - `register_image` / `register_snapshot` — fire-and-forget: результат
//!   загрузки в GPU не возвращается синхронно (round-trip на каждый пиксель
//!   дорог), прокси всегда отдаёт `Ok`; настоящий бэкенд логирует ошибку сам.
//! - `is_layer_promoted` всегда `false` (нет синхронного round-trip); layer
//!   promotion под femtovg — no-op в текущем движке, так что регрессии нет.
//! - Каждый кадр копирует display-list (`to_vec`) для передачи владения потоку.
//!   Это O(n)-клон; тайловый blit-скролл M3 его устранит.
//!
//! # GL-context handoff (M1.2, Windows, 2026-07-10)
//!
//! ADR-016 требовал сначала «спайкнуть» создание GL-контекста вне главного
//! потока. Замер на этой машине (winit 0.30 + glutin + femtovg) в M1.1 показал:
//! **создание бэкенда прямо на рендер-потоке падает** с `the underlying handle
//! is not available` — winit отдаёт Win32 window handle только на потоке, где
//! окно создано (главном). M1.2 реализует правильную передачу: контекст
//! **создаётся на main** (где handle валиден) через `FemtovgBackend::new`,
//! открепляется там же (`detach_gl_context` → `make_not_current`), а затем
//! конкретный `FemtovgBackend` (Send через ручной `unsafe impl`) переносится в
//! замыкание-`ctor` и на рендер-потоке привязывается к нему
//! (`attach_gl_context` → `make_current`). После этого present и swap_buffers
//! идут вне UI-потока. Сборку/открепление на main делает
//! `backend_factory::create_threaded_femtovg`. Если что-то не удалось —
//! прокси корректно **откатывается на in-process** путь (сообщение в stderr,
//! регрессии нет — окно рисует как обычно).
//!
//! # Render-side momentum (M1.3, 2026-07-10)
//!
//! Даже с present-ом вне UI-потока (M1.2) инерция замерзала при застое main:
//! кадры производит main, и долгий JS-тик/relayout останавливал их поток. M1.3
//! отдаёт momentum рендер-потоку. UI-поток при `TouchPhase::Ended` шлёт
//! [`RenderMsg::StartRenderMomentum`] (скорость + экстенты клампа) и продолжает
//! слать кадры как обычно. Рендер-поток удерживает последний закоммиченный кадр
//! ([`RenderState`]) и, если при активном momentum за `MOMENTUM_TICK` от main
//! не пришло ни одного сообщения (таймаут `recv_timeout` = UI-поток
//! застопорился), **сам** пересчитывает скролл из последнего якоря и повторно
//! презентует кадр — плавность держится на vsync. Пока main жив и шлёт кадры,
//! они (latest-wins) ведут презентацию и обновляют якорь; self-tick включается
//! только на голодание. Физика momentum вычисляется stateless-функциями
//! [`momentum_anim::velocity_at`]/[`displacement_since`] по локальным часам
//! потока, поэтому UI- и рендер-сторона не расходятся. Инвариант 6 сохранён:
//! без активного momentum поток по-прежнему паркуется на блокирующем `recv()`.
//! Полная независимость ввода (события во время застоя) — по-прежнему M2.
//!
//! [`displacement_since`]: momentum_anim::displacement_since

use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lumen_core::ext::{FontProvider, MemoryPressureLevel};
use lumen_core::geom::Size;
use lumen_image::Image;
use lumen_layout::Color;
use lumen_paint::{DisplayCommand, RenderBackend, RenderError};

use crate::momentum_anim;
use crate::wheel_scroll::{
    ContainerOffset, ScrollFeedback, ScrollShared, ScrollSnapshot, WheelInput, resolve_container_wheel,
};

/// Бюджет кадра для self-tick momentum (~60 fps). При активном render-side
/// momentum поток ждёт сообщения не дольше этого; таймаут = UI-поток ничего не
/// прислал за интервал → он застопорился → продолжаем инерцию сами.
const MOMENTUM_TICK: Duration = Duration::from_millis(16);

/// Нижняя граница интервала между тиками кривой/инерции колеса, которой
/// владеет рендер-поток (мс). Темп задаёт сам `render`: он блокируется на
/// vsync, поэтому следующий тик наступает сразу после возврата, как у кадров
/// потока браузера. Таймер «раз в 16,7 мс» с этим не уживается: фаза плывёт
/// относительно vsync и презентации идут парами «3 мс, 30 мс». Граница
/// спасает лишь бэкенды, у которых `render` не блокируется.
const OWNED_MIN_TICK_MS: f64 = 8.0;

/// Фора кривой, начатой из покоя (мс): первый кадр показывается сразу, а не
/// через таймер (`recv_timeout` на Windows квантуется ~15,6 мс, и первая
/// презентация приходила на 23 мс позже щелчка против 8 мс у потока браузера),
/// поэтому кривая стартует «задним числом» на время, за которое поток браузера
/// успевал бы выдать первый кадр.
const OWNED_HEAD_START_MS: f64 = 8.0;

/// Один кадр, переданный рендер-потоку. Владеющая копия — снапшот, который
/// поток рисует независимо от main (ADR-016 инвариант 1).
struct FrameCommit {
    /// Команды страницы (уже с применённым scroll на стороне рендера).
    content: Arc<Vec<DisplayCommand>>,
    /// Команды поверх страницы (tab bar, панели, pop-up'ы).
    overlay: Vec<DisplayCommand>,
    /// Текущий вертикальный скролл в CSS px.
    scroll_y: f32,
    /// Текущий горизонтальный скролл в CSS px.
    scroll_x: f32,
    /// Монотонный идентификатор коммита (для диагностики / frame-log).
    commit_id: u64,
    /// Какое поколение смещения рендер-потока поток браузера уже усыновил к
    /// моменту кадра (ADR-032, срез 3); [`ACK_BROWSER_SET`] — поток браузера
    /// сам сдвинул страницу (навигация, клавиатура, `scrollTo`) и его смещение
    /// главнее.
    ack_gen: u64,
    /// Поколение, которое поток браузера усыновил к моменту кадра, как есть:
    /// в отличие от `ack_gen` не подменяется [`ACK_BROWSER_SET`]. Смещения
    /// контейнеров с поколением не старше этого уже в списке кадра (срез 4).
    adopted_gen: u64,
}

/// `FrameCommit::ack_gen` кадра, чьё смещение задал сам поток браузера.
const ACK_BROWSER_SET: u64 = u64::MAX;

/// Что поток браузера усыновил из обратной связи рендер-потока:
/// `(поколение, y, x)`.
type Adopted = Arc<Mutex<(u64, f32, f32)>>;

/// Ручка к рендер-потоку, которую можно отдать другим потокам: канал команд и
/// запись усыновлённого смещения. `Clone + Send`.
#[derive(Clone)]
pub(crate) struct RenderLink {
    tx: Sender<RenderMsg>,
    adopted: Adopted,
}

impl RenderLink {
    /// Колесо прямо рендер-потоку (с главного потока, мимо потока браузера).
    pub(crate) fn send_wheel(
        &self,
        input: WheelInput,
        max_y: f32,
        max_x: f32,
        at: Option<(f32, f32)>,
    ) {
        let _ = self.tx.send(RenderMsg::Wheel { input, max_y, max_x, at });
    }

    /// Свежий снимок прокрутки: по его контейнерам рендер-поток выбирает цель колеса.
    pub(crate) fn send_snapshot(&self, snap: Arc<ScrollSnapshot>) {
        let _ = self.tx.send(RenderMsg::Snapshot(snap));
    }

    /// Подключить обратную связь: рендер-поток начнёт возвращать смещение.
    pub(crate) fn attach(&self, shared: Arc<ScrollShared>) {
        let _ = self.tx.send(RenderMsg::AttachScrollLink(shared));
    }

    /// Поток браузера усыновил смещение поколения `gen_id`.
    pub(crate) fn adopt(&self, gen_id: u64, y: f32, x: f32) {
        if let Ok(mut g) = self.adopted.lock() {
            *g = (gen_id, y, x);
        }
    }
}

thread_local! {
    /// Ручка последнего рендер-потока, созданного на этом потоке — владелец
    /// окна забирает её сразу после `create_backend`.
    static LAST_LINK: std::cell::RefCell<Option<RenderLink>> = const { std::cell::RefCell::new(None) };
}

/// Забирает ручку рендер-потока, созданного на этом потоке последним.
pub(crate) fn take_last_link() -> Option<RenderLink> {
    LAST_LINK.with(|l| l.borrow_mut().take())
}

/// Сообщение рендер-потоку. Кадры коалесцируются (latest-wins); все прочие —
/// управляющие, применяются в строгом порядке поступления.
enum RenderMsg {
    /// Новый кадр (latest-wins).
    Frame(FrameCommit),
    /// Изменение физического размера поверхности.
    Resize { width: u32, height: u32 },
    /// Изменение HiDPI scale factor.
    SetScaleFactor(f64),
    /// Фон канвы (CSS Backgrounds §3.11.1).
    SetCanvasBackground(Option<Color>),
    /// Превью-масштаб зума (ADR-016 M0.3).
    SetPreviewScale(f32),
    /// Фиксированное смещение страницы (ADR-016 M0.4).
    SetPageOffset { x: f32, y: f32 },
    /// Регистрация изображения под ключом. `Arc<Image>` (BUG-272 срез 17):
    /// пересылка в render-поток клонирует указатель, а не пиксельный буфер.
    RegisterImage { src: String, image: Arc<Image> },
    /// Сброс всех зарегистрированных изображений.
    ClearImages,
    /// Регистрация offscreen-снимка слоя (View Transitions).
    RegisterSnapshot { id: u64, image: Image },
    /// Сброс всех снимков слоёв.
    ClearSnapshots,
    /// Смена провайдера шрифтов.
    SetFontProvider(Option<Arc<dyn FontProvider>>),
    /// Предзагрузка curated-fallback шрифтов.
    PreloadCuratedFallbacks,
    /// Memory-pressure для layer-cache.
    LayerMemoryPressure(MemoryPressureLevel),
    /// Memory-pressure для glyph atlas.
    AtlasMemoryPressure(MemoryPressureLevel),
    /// Promote узла в собственный GPU-слой (will-change).
    PromoteLayer { node_id: u32, width: u32, height: u32 },
    /// Demote узла обратно.
    DemoteLayer { node_id: u32 },
    /// Старт render-side momentum (ADR-016 M1.3): поток сам продолжает инерцию
    /// при застопорившемся UI-потоке.
    StartRenderMomentum {
        /// Вертикальная скорость, CSS px/ms.
        vel_y: f32,
        /// Горизонтальная скорость, CSS px/ms.
        vel_x: f32,
        /// Максимальный вертикальный скролл (клампинг).
        max_scroll_y: f32,
        /// Максимальный горизонтальный скролл (клампинг).
        max_scroll_x: f32,
    },
    /// Отмена render-side momentum и анимации щелчка (новый жест / навигация).
    StopRenderMomentum,
    /// Старт render-side анимации щелчка колеса (THREAD-6).
    StartRenderScrollAnim { start_y: f32, target_y: f32 },
    /// Колесо/тачпад с главного потока (ADR-032, срез 3): смещение страницы
    /// ведёт рендер-поток. `max_*` — пределы из снимка прокрутки.
    /// `at` — курсор в CSS px от начала области страницы (хит-тест контейнеров).
    Wheel { input: WheelInput, max_y: f32, max_x: f32, at: Option<(f32, f32)> },
    /// Снимок прокрутки потока браузера (ADR-032, срез 4).
    Snapshot(Arc<ScrollSnapshot>),
    /// Подключение обратной связи со смещением (ADR-032, срез 3).
    AttachScrollLink(Arc<ScrollShared>),
    /// Завершение потока (шлётся из `Drop`).
    Shutdown,
}

/// Возможности бэкенда, снятые синхронно при старте потока, чтобы прокси мог
/// отвечать на `supports_page_offset` / `viewport_size` / `scale_factor` без
/// round-trip на каждый запрос.
struct BackendCaps {
    supports_page_offset: bool,
    scale: f64,
    phys_w: u32,
    phys_h: u32,
}

/// Прокси-бэкенд: реализует [`RenderBackend`], но настоящий GPU-бэкенд живёт на
/// выделенном рендер-потоке (ADR-016 M1). См. модульную документацию.
pub struct ThreadedRenderBackend {
    /// Упорядоченный канал команд рендер-потоку.
    tx: Sender<RenderMsg>,
    /// Handle рендер-потока для join при shutdown.
    join: Option<JoinHandle<()>>,
    /// Зеркало HiDPI scale (обновляется на `set_scale_factor`).
    scale: f64,
    /// Зеркало физической ширины поверхности (обновляется на `resize`).
    phys_w: u32,
    /// Зеркало физической высоты поверхности (обновляется на `resize`).
    phys_h: u32,
    /// Кэш `supports_page_offset` настоящего бэкенда (снят при старте).
    supports_page_offset: bool,
    /// Монотонный счётчик кадров.
    commit_counter: u64,
    /// Версия retained-списка, объявленная `set_content_epoch` для ближайшего
    /// кадра; `0` — список производный, версии нет. Гасится после `render`.
    content_epoch: u64,
    /// Последний переданный рендер-потоку контент с его версией (THREAD-8):
    /// пока версия не сменилась, кадр делит тот же `Arc`, без копии списка.
    sent_content: Option<(u64, Arc<Vec<DisplayCommand>>)>,
    /// Усыновлённое потоком браузера смещение рендер-потока (ADR-032, срез 3).
    adopted: Adopted,
    /// Последнее поколение из `adopted`, которое прокси уже учёл.
    seen_adopted_gen: u64,
    /// Смещение, которое поток браузера считает своим: сравнение с ним отличает
    /// «браузер сдвинул страницу сам» от «браузер просто перерисовал».
    browser_scroll: Option<(f32, f32)>,
}

impl ThreadedRenderBackend {
    /// Запускает рендер-поток и возвращает прокси.
    ///
    /// `ctor` вызывается **на рендер-потоке** и возвращает настоящий бэкенд,
    /// готовый к рендеру на этом потоке. В M1.2 бэкенд (femtovg `Canvas` +
    /// GL-контекст) создаётся на **главном** потоке (window handle доступен
    /// только там), контекст откреплён (`make_not_current`) и перенесён сюда —
    /// поэтому `ctor` лишь привязывает контекст к рендер-потоку
    /// (`attach_gl_context` → `make_current`). Возвращает `Err(msg)`, если
    /// бэкенд не готов — вызывающая сторона откатывается на in-process путь.
    ///
    /// # Errors
    /// Возвращает строку с описанием, если конструктор бэкенда вернул ошибку
    /// (например, `make_current` не удался на рендер-потоке — тогда shell
    /// использует обычный однопоточный бэкенд).
    pub fn new<F>(ctor: F) -> Result<Self, String>
    where
        F: FnOnce() -> Result<Box<dyn RenderBackend>, String> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<RenderMsg>();
        let adopted: Adopted = Arc::new(Mutex::new((0, 0.0, 0.0)));
        // Одноразовый handshake-канал: поток отдаёт caps или ошибку создания.
        let (caps_tx, caps_rx) = mpsc::sync_channel::<Result<BackendCaps, String>>(1);

        let join = thread::Builder::new()
            .name("lumen-render".to_owned())
            .spawn(move || render_thread_main(ctor, rx, caps_tx))
            .map_err(|e| format!("не удалось запустить рендер-поток: {e}"))?;

        // Ждём результат создания бэкенда на потоке.
        let caps = match caps_rx.recv() {
            Ok(Ok(caps)) => caps,
            Ok(Err(e)) => {
                let _ = join.join();
                return Err(e);
            }
            Err(_) => {
                let _ = join.join();
                return Err("рендер-поток завершился до handshake".to_owned());
            }
        };

        LAST_LINK.with(|l| {
            *l.borrow_mut() = Some(RenderLink { tx: tx.clone(), adopted: Arc::clone(&adopted) });
        });
        Ok(Self {
            tx,
            adopted,
            seen_adopted_gen: 0,
            browser_scroll: None,
            join: Some(join),
            scale: caps.scale,
            phys_w: caps.phys_w,
            phys_h: caps.phys_h,
            supports_page_offset: caps.supports_page_offset,
            commit_counter: 0,
            content_epoch: 0,
            sent_content: None,
        })
    }

    /// `ack_gen` для кадра со смещением `(y, x)`: поколение, усыновленное
    /// потоком браузера, либо [`ACK_BROWSER_SET`], если он сдвинул страницу сам.
    fn ack_for_frame(&mut self, y: f32, x: f32) -> (u64, u64) {
        let (gen_id, ay, ax) = self.adopted.lock().map(|g| *g).unwrap_or((0, 0.0, 0.0));
        if gen_id != self.seen_adopted_gen {
            self.seen_adopted_gen = gen_id;
            self.browser_scroll = Some((ay, ax));
        }
        let baseline = self.browser_scroll.unwrap_or((y, x));
        self.browser_scroll = Some((y, x));
        (if baseline == (y, x) { gen_id } else { ACK_BROWSER_SET }, gen_id)
    }

    /// Отправляет управляющее сообщение; молча игнорирует, если поток уже мёртв
    /// (при штатном shutdown это ожидаемо).
    fn send(&self, msg: RenderMsg) {
        let _ = self.tx.send(msg);
    }
}

impl RenderBackend for ThreadedRenderBackend {
    fn render(
        &mut self,
        content: &[DisplayCommand],
        overlay: &[DisplayCommand],
        scroll_y: f32,
        scroll_x: f32,
    ) -> Result<(), RenderError> {
        self.commit_counter = self.commit_counter.wrapping_add(1);
        // Владеющий снапшот кадра — рендер-поток рисует его независимо от main.
        // THREAD-8: версия retained-списка не изменилась — рендер-поток уже
        // держит этот же список, копировать его заново (O(команд) на UI в
        // каждом кадре прокрутки) незачем. Версия бампается при каждой правке
        // списка на месте (`bump_display_list_epoch`); длину сверяем как
        // страховку от рассинхрона.
        let epoch = std::mem::take(&mut self.content_epoch);
        let content = match &self.sent_content {
            Some((e, arc)) if epoch != 0 && *e == epoch && arc.len() == content.len() => {
                Arc::clone(arc)
            }
            _ => {
                let arc = Arc::new(content.to_vec());
                self.sent_content = (epoch != 0).then(|| (epoch, Arc::clone(&arc)));
                arc
            }
        };
        let (ack_gen, adopted_gen) = self.ack_for_frame(scroll_y, scroll_x);
        let frame = FrameCommit {
            content,
            overlay: overlay.to_vec(),
            scroll_y,
            scroll_x,
            commit_id: self.commit_counter,
            ack_gen,
            adopted_gen,
        };
        self.send(RenderMsg::Frame(frame));
        // Fire-and-forget latest-wins: main не ждёт present (ADR-016 инвариант 4).
        Ok(())
    }

    fn set_content_epoch(&mut self, epoch: u64) {
        self.content_epoch = epoch;
    }

    fn set_preview_scale(&mut self, scale: f32) {
        self.send(RenderMsg::SetPreviewScale(scale));
    }

    fn set_page_offset(&mut self, x: f32, y: f32) {
        self.send(RenderMsg::SetPageOffset { x, y });
    }

    fn supports_page_offset(&self) -> bool {
        self.supports_page_offset
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.phys_w = width;
        self.phys_h = height;
        self.send(RenderMsg::Resize { width, height });
    }

    fn set_scale_factor(&mut self, scale: f64) {
        self.scale = scale;
        self.send(RenderMsg::SetScaleFactor(scale));
    }

    fn register_image(&mut self, src: String, image: Arc<Image>) -> Result<(), String> {
        // Fire-and-forget: результат загрузки в GPU не возвращается синхронно.
        // `image` — уже `Arc`, пересылаем указатель (BUG-272 срез 17).
        self.send(RenderMsg::RegisterImage { src, image });
        Ok(())
    }

    fn clear_images(&mut self) {
        self.send(RenderMsg::ClearImages);
    }

    fn register_snapshot(&mut self, id: u64, image: &Image) -> Result<(), String> {
        self.send(RenderMsg::RegisterSnapshot { id, image: image.clone() });
        Ok(())
    }

    fn clear_snapshots(&mut self) {
        self.send(RenderMsg::ClearSnapshots);
    }

    fn set_font_provider(&mut self, provider: Option<Arc<dyn FontProvider>>) {
        self.send(RenderMsg::SetFontProvider(provider));
    }

    fn set_canvas_background(&mut self, color: Option<Color>) {
        self.send(RenderMsg::SetCanvasBackground(color));
    }

    fn viewport_size(&self) -> Size {
        // То же вычисление, что в FemtovgBackend::viewport_size — зеркало phys/scale.
        Size {
            width: (self.phys_w as f64 / self.scale) as f32,
            height: (self.phys_h as f64 / self.scale) as f32,
        }
    }

    fn scale_factor(&self) -> f64 {
        self.scale
    }

    fn preload_curated_fallbacks(&mut self) {
        self.send(RenderMsg::PreloadCuratedFallbacks);
    }

    fn on_layer_memory_pressure(&mut self, level: MemoryPressureLevel) {
        self.send(RenderMsg::LayerMemoryPressure(level));
    }

    fn on_atlas_memory_pressure(&mut self, level: MemoryPressureLevel) {
        self.send(RenderMsg::AtlasMemoryPressure(level));
    }

    fn promote_layer(&mut self, node_id: u32, width: u32, height: u32) {
        self.send(RenderMsg::PromoteLayer { node_id, width, height });
    }

    fn is_layer_promoted(&self, _node_id: u32) -> bool {
        // Нет синхронного round-trip; femtovg layer promotion — no-op, регрессии нет.
        false
    }

    fn demote_layer(&mut self, node_id: u32) {
        self.send(RenderMsg::DemoteLayer { node_id });
    }

    fn start_render_momentum(
        &mut self,
        vel_y: f32,
        vel_x: f32,
        max_scroll_y: f32,
        max_scroll_x: f32,
    ) {
        self.send(RenderMsg::StartRenderMomentum { vel_y, vel_x, max_scroll_y, max_scroll_x });
    }

    fn stop_render_momentum(&mut self) {
        self.send(RenderMsg::StopRenderMomentum);
    }

    fn start_render_scroll_anim(&mut self, start_y: f32, target_y: f32) {
        self.send(RenderMsg::StartRenderScrollAnim { start_y, target_y });
    }

    fn debug_mem_report(&self) -> String {
        "threaded backend (mem report on render thread)".to_owned()
    }
}

impl Drop for ThreadedRenderBackend {
    fn drop(&mut self) {
        self.send(RenderMsg::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Тело рендер-потока: создаёт бэкенд, отдаёт caps, затем крутит цикл коалесцинга.
fn render_thread_main<F>(
    ctor: F,
    rx: Receiver<RenderMsg>,
    caps_tx: SyncSender<Result<BackendCaps, String>>,
) where
    F: FnOnce() -> Result<Box<dyn RenderBackend>, String>,
{
    // ADR-016 M1.2: `ctor` привязывает уже созданный на main GL-контекст к
    // этому потоку (`attach_gl_context` → `make_current`); сам Canvas/контекст
    // femtovg создан на главном потоке и перенесён сюда откреплённым.
    let mut backend = match ctor() {
        Ok(b) => b,
        Err(e) => {
            let _ = caps_tx.send(Err(e));
            return;
        }
    };

    let caps = BackendCaps {
        supports_page_offset: backend.supports_page_offset(),
        scale: backend.scale_factor(),
        phys_w: (backend.viewport_size().width as f64 * backend.scale_factor()).round() as u32,
        phys_h: (backend.viewport_size().height as f64 * backend.scale_factor()).round() as u32,
    };
    if caps_tx.send(Ok(caps)).is_err() {
        // Прокси не дождался (сразу дропнут) — выходим.
        return;
    }

    run_render_loop(&mut backend, &rx);
}

/// Активный render-side momentum (ADR-016 M1.3). Все времена — по локальным
/// часам рендер-потока (`Instant`), поэтому вычисления самосогласованы и не
/// зависят от epoch UI-потока.
struct RenderMomentum {
    /// Начальная вертикальная скорость (CSS px/ms).
    v0_y: f32,
    /// Начальная горизонтальная скорость (CSS px/ms).
    v0_x: f32,
    /// Время старта momentum (ms от старта рендер-потока).
    t0_ms: f64,
    /// Максимальный вертикальный скролл (клампинг).
    max_y: f32,
    /// Максимальный горизонтальный скролл (клампинг).
    max_x: f32,
}

/// Список страницы со смещениями контейнеров: (исходный список, версия смещений, результат).
type DrawnContent = (Arc<Vec<DisplayCommand>>, u64, Arc<Vec<DisplayCommand>>);

/// Удержанное между пачками состояние рендер-потока (ADR-016 M1.3). Позволяет
/// продолжать momentum-презентацию из последнего закоммиченного кадра, когда
/// UI-поток застопорился и новых кадров нет.
struct RenderState {
    /// Последний закоммиченный контент страницы (для повторной презентации).
    last_content: Arc<Vec<DisplayCommand>>,
    /// Последний закоммиченный overlay.
    last_overlay: Vec<DisplayCommand>,
    /// Вертикальный скролл последнего кадра — якорь momentum.
    anchor_scroll_y: f32,
    /// Горизонтальный скролл последнего кадра — якорь momentum.
    anchor_scroll_x: f32,
    /// Время последнего кадра (ms от старта рендер-потока).
    anchor_t_ms: f64,
    /// `commit_id` последнего закоммиченного кадра — для аннотации self-tick
    /// презентаций в `LUMEN_FRAME_LOG` (ADR-016 M1): self-tick перерисовывает
    /// именно этот удержанный кадр, поэтому его id и логируется.
    anchor_commit_id: u64,
    /// Активный momentum, если есть.
    momentum: Option<RenderMomentum>,
    /// Активная анимация щелчка колеса (THREAD-6); время старта — по часам
    /// рендер-потока.
    scroll_anim: Option<crate::scroll_anim::ScrollAnim>,
    /// Обратная связь со смещением (ADR-032, срез 3); `None` — маршрутизация
    /// колеса выключена.
    shared: Option<Arc<ScrollShared>>,
    /// Смещение страницы, с которым рендер-поток рисует сейчас.
    cur_y: f32,
    cur_x: f32,
    /// Поколение смещения, которым владеет рендер-поток: растёт с каждым
    /// изменением от колеса и уходит потоку браузера обратной связью.
    gen_id: u64,
    /// Смещением сейчас владеет рендер-поток (колесо пришло мимо потока
    /// браузера и тот ещё не догнал).
    owned: bool,
    /// Пределы смещения из последнего снимка прокрутки.
    max_y: f32,
    max_x: f32,
    /// EWMA-скорость пальца на тачпаде (CSS px/ms) и время последнего события.
    touch_vel: (f32, f32),
    touch_t_ms: f64,
    /// В текущей пачке уже была презентация (кадр или колесо).
    presented: bool,
    /// Дедлайн следующего тика колеса (ms от старта рендер-потока).
    next_tick_ms: f64,
    /// Последний снимок прокрутки потока браузера (контейнеры и их геометрия).
    snap: Arc<ScrollSnapshot>,
    /// Смещения overflow-контейнеров, которыми владеет рендер-поток, пока поток
    /// браузера не усыновил их и не вернул в списке кадра (ADR-032, срез 4).
    owned_containers: Vec<ContainerOffset>,
    /// Растёт при каждом изменении `owned_containers`.
    containers_version: u64,
    /// Усыновленное потоком браузера поколение по последнему кадру.
    frame_adopted: u64,
    /// Кэш списка с подставленными смещениями: (исходный список, версия, результат).
    drawn: Option<DrawnContent>,
}

impl RenderState {
    /// Пустое состояние: кадров ещё не было, momentum неактивен.
    fn new() -> Self {
        Self {
            last_content: Arc::new(Vec::new()),
            last_overlay: Vec::new(),
            anchor_scroll_y: 0.0,
            anchor_scroll_x: 0.0,
            anchor_t_ms: 0.0,
            anchor_commit_id: 0,
            momentum: None,
            scroll_anim: None,
            shared: None,
            cur_y: 0.0,
            cur_x: 0.0,
            gen_id: 0,
            owned: false,
            max_y: 0.0,
            max_x: 0.0,
            touch_vel: (0.0, 0.0),
            touch_t_ms: 0.0,
            presented: false,
            next_tick_ms: 0.0,
            snap: Arc::new(ScrollSnapshot::default()),
            owned_containers: Vec::new(),
            containers_version: 0,
            frame_adopted: 0,
            drawn: None,
        }
    }

    /// Идёт ли анимация или инерция, которую ведёт рендер-поток.
    fn driving(&self) -> bool {
        self.scroll_anim.is_some() || self.momentum.is_some()
    }

    /// Продвигает смещение по активной кривой или инерции до `now_ms`.
    /// Возвращает `true`, если смещение изменилось.
    fn advance(&mut self, now_ms: f64) -> bool {
        let (ny, nx) = if let Some(anim) = self.scroll_anim {
            let (y, done) = anim.sample(now_ms);
            if done {
                self.scroll_anim = None;
            }
            (y, self.cur_x)
        } else if let Some(m) = self.momentum.as_ref() {
            let (y, x, done) = momentum_scroll_at(
                m,
                self.anchor_scroll_y,
                self.anchor_scroll_x,
                self.anchor_t_ms,
                now_ms,
            );
            if done {
                self.momentum = None;
            }
            (y, x)
        } else {
            return false;
        };
        let changed = (ny, nx) != (self.cur_y, self.cur_x);
        self.cur_y = ny;
        self.cur_x = nx;
        changed
    }

    /// Отдаёт текущее смещение потоку браузера под новым поколением.
    fn publish(&mut self) {
        self.gen_id += 1;
        if let Some(sh) = self.shared.as_ref() {
            let containers = self.owned_containers.iter().map(|o| (o.id, o.x, o.y)).collect();
            sh.post_feedback(ScrollFeedback {
                gen_id: self.gen_id,
                y: self.cur_y,
                x: self.cur_x,
                containers,
            });
        }
    }

    /// Плавный сдвиг по Y на `dy`: новая кривая к цели с учётом идущей
    /// (повтор колеса дописывает дельту к цели, а не откатывает назад).
    fn smooth_by(&mut self, dy: f32, now_ms: f64) {
        if dy == 0.0 {
            return;
        }
        let base = self.scroll_anim.map_or(self.cur_y, |a| a.target());
        let target = (base + dy).clamp(0.0, self.max_y.max(0.0));
        if (target - self.cur_y).abs() <= f32::EPSILON {
            self.scroll_anim = None;
            return;
        }
        let start_y = self.scroll_anim.map_or(self.cur_y, |a| a.sample(now_ms).0);
        self.scroll_anim = Some(crate::scroll_anim::ScrollAnim {
            start_y,
            target_y: target,
            start_time_ms: now_ms,
        });
    }

    /// Мгновенный сдвиг по X. `true`, если сдвинулось.
    fn shift_x(&mut self, dx: f32) -> bool {
        if dx == 0.0 {
            return false;
        }
        let nx = (self.cur_x + dx).clamp(0.0, self.max_x.max(0.0));
        let moved = (nx - self.cur_x).abs() > f32::EPSILON;
        self.cur_x = nx;
        moved
    }

    /// Применяет ввод колеса. `true` — смещение изменилось сразу (не кривой).
    #[cfg(test)]
    fn apply_wheel(&mut self, input: WheelInput, max_y: f32, max_x: f32, now_ms: f64) -> bool {
        self.apply_wheel_at(input, max_y, max_x, None, now_ms)
    }

    /// `at` — курсор в CSS px от начала области страницы; с ним дельту может
    /// забрать overflow-контейнер под курсором (срез 4), а не страница.
    fn apply_wheel_at(
        &mut self,
        input: WheelInput,
        max_y: f32,
        max_x: f32,
        at: Option<(f32, f32)>,
        now_ms: f64,
    ) -> bool {
        let was_driving = self.owned && self.driving();
        if self.advance(now_ms) {
            self.publish();
        }
        self.owned = true;
        self.max_y = max_y;
        self.max_x = max_x;
        if let Some(consumed) = self.try_container(input, at) {
            return consumed;
        }
        let moved = self.apply_input(input, max_y, max_x, now_ms);
        if !was_driving && self.driving() {
            if let Some(a) = self.scroll_anim.as_mut() {
                a.start_time_ms -= OWNED_HEAD_START_MS;
            }
            self.next_tick_ms = now_ms;
        }
        moved
    }

    /// Колесо над overflow-контейнером: `Some(true)` — контейнер сдвинут,
    /// `Some(false)` — жест погашен на границе (`overscroll-behavior`), `None` —
    /// контейнера нет, дельта идёт странице. Контейнер сдвигается сразу, без
    /// кривой, как у потока браузера (BUG-822).
    fn try_container(&mut self, input: WheelInput, at: Option<(f32, f32)>) -> Option<bool> {
        let at = at?;
        let (dx, dy) = match input {
            WheelInput::Notch { dx, dy }
            | WheelInput::TouchStart { dx, dy }
            | WheelInput::TouchMove { dx, dy } => (dx, dy),
            WheelInput::TouchEnd | WheelInput::TouchCancel => return None,
        };
        let doc = (at.0 + self.cur_x, at.1 + self.cur_y);
        let chain = resolve_container_wheel(&self.snap, &self.owned_containers, doc, dx, dy)?;
        if !chain.moved {
            return Some(false);
        }
        let id = chain.node.index() as u32;
        // Поколение, под которым это смещение уйдёт обратной связью: `publish`
        // сразу следует за возвратом `true`.
        let gen_id = self.gen_id + 1;
        let off = ContainerOffset { id, x: chain.new_x, y: chain.new_y, gen_id };
        match self.owned_containers.iter_mut().find(|o| o.id == id) {
            Some(o) => *o = off,
            None => self.owned_containers.push(off),
        }
        self.containers_version += 1;
        Some(true)
    }

    /// Забывает смещения контейнеров, которые поток браузера уже усыновил и
    /// которые поэтому есть и в списке кадра, и в снимке.
    fn prune_containers(&mut self) {
        let limit = self.frame_adopted.min(self.snap.adopted_gen);
        let before = self.owned_containers.len();
        self.owned_containers.retain(|o| o.gen_id > limit);
        if self.owned_containers.len() != before {
            self.containers_version += 1;
        }
    }

    /// Список для рисования: `raw` со смещениями контейнеров, которыми владеет
    /// рендер-поток. Без них — тот же `Arc`; иначе копия с подставленными
    /// смещениями (кэшируется до смены списка или смещений).
    fn drawable(&mut self, raw: &Arc<Vec<DisplayCommand>>) -> Arc<Vec<DisplayCommand>> {
        if self.owned_containers.is_empty() {
            return Arc::clone(raw);
        }
        if let Some((r, v, out)) = &self.drawn
            && Arc::ptr_eq(r, raw)
            && *v == self.containers_version
        {
            return Arc::clone(out);
        }
        let overrides: Vec<lumen_paint::ScrollLayerOverride> = self
            .owned_containers
            .iter()
            .filter_map(|o| {
                let c = self.snap.containers.iter().find(|c| c.node.index() as u32 == o.id)?;
                Some(lumen_paint::ScrollLayerOverride {
                    id: o.id,
                    scroll_x: o.x,
                    scroll_y: o.y,
                    max_x: (c.scroll_width - c.clip_rect.width).max(0.0),
                    max_y: (c.scroll_height - c.clip_rect.height).max(0.0),
                })
            })
            .collect();
        let mut list: Vec<DisplayCommand> = raw.as_ref().clone();
        lumen_paint::apply_scroll_overrides(&mut list, &overrides);
        let out = Arc::new(list);
        self.drawn = Some((Arc::clone(raw), self.containers_version, Arc::clone(&out)));
        out
    }

    fn apply_input(&mut self, input: WheelInput, max_y: f32, max_x: f32, now_ms: f64) -> bool {
        match input {
            WheelInput::Notch { dx, dy } => {
                self.momentum = None;
                self.touch_vel = (0.0, 0.0);
                self.smooth_by(dy, now_ms);
                self.shift_x(dx)
            }
            WheelInput::TouchStart { dx, dy } => {
                self.momentum = None;
                self.scroll_anim = None;
                self.touch_vel = (0.0, 0.0);
                self.touch_t_ms = now_ms;
                self.smooth_by(dy, now_ms);
                self.shift_x(dx)
            }
            WheelInput::TouchMove { dx, dy } => {
                let dt = (now_ms - self.touch_t_ms).max(1.0) as f32;
                self.touch_t_ms = now_ms;
                // Те же α = 0.6, что у потока браузера: быстро следует за
                // движением, сглаживает дрожание.
                const ALPHA: f32 = 0.6;
                let (vx, vy) = self.touch_vel;
                self.touch_vel = (ALPHA * dx / dt + (1.0 - ALPHA) * vx, ALPHA * dy / dt + (1.0 - ALPHA) * vy);
                self.smooth_by(dy, now_ms);
                self.shift_x(dx)
            }
            WheelInput::TouchEnd => {
                let (vx, vy) = self.touch_vel;
                self.touch_vel = (0.0, 0.0);
                if vx.abs() + vy.abs() >= momentum_anim::MIN_VELOCITY_PX_MS {
                    self.scroll_anim = None;
                    self.anchor_scroll_y = self.cur_y;
                    self.anchor_scroll_x = self.cur_x;
                    self.anchor_t_ms = now_ms;
                    self.momentum = Some(RenderMomentum {
                        v0_y: vy,
                        v0_x: vx,
                        t0_ms: now_ms,
                        max_y,
                        max_x,
                    });
                }
                false
            }
            WheelInput::TouchCancel => {
                self.touch_vel = (0.0, 0.0);
                false
            }
        }
    }

    /// Выбирает смещение кадра потока браузера (ADR-032, срез 3): пока
    /// рендер-поток владеет смещением, кадры, снятые до усыновления, рисуются с
    /// его смещением; сдвиг, заданный самим потоком браузера, главнее.
    fn resolve_frame_scroll(&mut self, frame: &mut FrameCommit, now_ms: f64) {
        if self.owned {
            if frame.ack_gen == ACK_BROWSER_SET {
                self.scroll_anim = None;
                self.momentum = None;
                self.owned = false;
            } else {
                // Шаг, сделанный здесь, тоже уходит потоку браузера: иначе
                // последний шаг кривой терялся, а кадр с усыновлённым
                // прежним смещением откатывал бы страницу на долю пикселя.
                if self.advance(now_ms) {
                    self.publish();
                }
                if frame.ack_gen < self.gen_id || self.driving() {
                    frame.scroll_y = self.cur_y;
                    frame.scroll_x = self.cur_x;
                } else {
                    self.owned = false;
                }
            }
        } else {
            frame.scroll_y = frame_scroll_y(self, frame.scroll_y, now_ms);
        }
        self.cur_y = frame.scroll_y;
        self.cur_x = frame.scroll_x;
        if let Some(sh) = self.shared.as_ref() {
            sh.set_offset(self.cur_y, self.cur_x);
        }
    }
}

/// Абсолютный скролл под momentum в момент `now_ms`: якорный скролл плюс
/// смещение со скоростью, корректно затухшей от старта до якоря. Закламплено в
/// `[0, max]`. Возвращает `(scroll_y, scroll_x, done)`; `done` — скорость упала
/// ниже порога остановки (тот же критерий, что на UI-стороне).
fn momentum_scroll_at(
    m: &RenderMomentum,
    anchor_y: f32,
    anchor_x: f32,
    anchor_t_ms: f64,
    now_ms: f64,
) -> (f32, f32, bool) {
    let vel_y = momentum_anim::velocity_at(m.v0_y, m.t0_ms, anchor_t_ms);
    let vel_x = momentum_anim::velocity_at(m.v0_x, m.t0_ms, anchor_t_ms);
    let dy = momentum_anim::displacement_since(vel_y, anchor_t_ms, now_ms);
    let dx = momentum_anim::displacement_since(vel_x, anchor_t_ms, now_ms);
    let scroll_y = (anchor_y + dy).clamp(0.0, m.max_y.max(0.0));
    let scroll_x = (anchor_x + dx).clamp(0.0, m.max_x.max(0.0));
    let cur_v = momentum_anim::velocity_at(m.v0_y, m.t0_ms, now_ms).abs()
        + momentum_anim::velocity_at(m.v0_x, m.t0_ms, now_ms).abs();
    let done = cur_v < momentum_anim::MIN_VELOCITY_PX_MS;
    (scroll_y, scroll_x, done)
}

/// Цикл рендер-потока: блокирующий `recv()` (idle-park, инвариант 6) без
/// momentum; с активным momentum — `recv_timeout(MOMENTUM_TICK)`, и таймаут
/// (UI-поток ничего не прислал за интервал → застопорился) запускает self-tick
/// момента (ADR-016 M1.3). Полученная пачка коалесцируется (latest-wins) с
/// строгим порядком управляющих сообщений.
fn run_render_loop(backend: &mut Box<dyn RenderBackend>, rx: &Receiver<RenderMsg>) {
    let clock = Instant::now();
    let mut state = RenderState::new();
    // Начало прошлой порции работы (батч или self-tick). Таймаут тика
    // отсчитывается от него, а не от конца: `render` блокируется на vsync
    // (≈ период), и `recv_timeout(MOMENTUM_TICK)` после него давал бы шаг
    // «период + период» = ~30 мс, то есть каждый второй vsync пропущен.
    let mut last_work = Instant::now();
    loop {
        let first = if state.owned && state.driving() {
            let wait_ms = (state.next_tick_ms - clock.elapsed().as_secs_f64() * 1000.0).max(0.0);
            match rx.recv_timeout(Duration::from_secs_f64(wait_ms / 1000.0)) {
                Ok(m) => Some(m),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else if state.momentum.is_some() || state.scroll_anim.is_some() {
            match rx.recv_timeout(MOMENTUM_TICK.saturating_sub(last_work.elapsed())) {
                Ok(m) => Some(m),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            // Паркуемся до первого сообщения (без polling).
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return, // канал закрыт — прокси дропнут
            }
        };

        last_work = Instant::now();
        match first {
            Some(first) => {
                let mut batch = vec![first];
                // Дренируем всё, что уже в очереди, одним махом.
                loop {
                    match rx.try_recv() {
                        Ok(m) => batch.push(m),
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => break,
                    }
                }
                let now_ms = clock.elapsed().as_secs_f64() * 1000.0;
                state.presented = false;
                if process_batch(backend, batch, &mut state, now_ms) {
                    return; // получен Shutdown
                }
                // Колесо идёт чаще тика: без этого таймаут не наступал бы и
                // кривая стояла, пока сообщения не кончатся.
                let now_ms = clock.elapsed().as_secs_f64() * 1000.0;
                if state.owned
                    && state.driving()
                    && !state.presented
                    && now_ms >= state.next_tick_ms
                {
                    self_tick_owned(backend, &mut state, now_ms);
                }
            }
            None => {
                // Таймаут при активном momentum: UI-поток молчит — тикаем сами.
                let now_ms = clock.elapsed().as_secs_f64() * 1000.0;
                if state.owned {
                    self_tick_owned(backend, &mut state, now_ms);
                } else {
                    self_tick_momentum(backend, &mut state, now_ms);
                    self_tick_scroll_anim(backend, &mut state, now_ms);
                }
            }
        }
    }
}

/// Презентует последний закоммиченный кадр с текущим смещением рендер-потока.
fn present_owned(backend: &mut Box<dyn RenderBackend>, state: &mut RenderState) {
    if state.last_content.is_empty() {
        return; // кадров ещё не было — нечего презентовать
    }
    backend.set_frame_commit_id(state.anchor_commit_id, true);
    let content = state.drawable(&Arc::clone(&state.last_content));
    if let Err(err) = backend.render(&content, &state.last_overlay, state.cur_y, state.cur_x) {
        eprintln!("[render-thread] ошибка презентации колеса: {err:?}");
    }
    crate::present_log::present(state.anchor_commit_id, true);
    state.presented = true;
}

/// Тик кривой/инерции колеса, которым владеет рендер-поток (ADR-032, срез 3):
/// продвигает смещение, презентует и возвращает его потоку браузера.
fn self_tick_owned(backend: &mut Box<dyn RenderBackend>, state: &mut RenderState, now_ms: f64) {
    state.next_tick_ms = now_ms + OWNED_MIN_TICK_MS;
    if state.advance(now_ms) {
        state.publish();
        present_owned(backend, state);
    }
}

/// Self-tick momentum при застопорившемся UI-потоке (ADR-016 M1.3):
/// пересчитывает скролл из удержанного якоря и повторно презентует последний
/// закоммиченный кадр. Завершившийся momentum сбрасывается.
fn self_tick_momentum(
    backend: &mut Box<dyn RenderBackend>,
    state: &mut RenderState,
    now_ms: f64,
) {
    let Some(m) = state.momentum.as_ref() else {
        return;
    };
    if state.last_content.is_empty() {
        return; // кадров ещё не было — нечего презентовать
    }
    let (scroll_y, scroll_x, done) = momentum_scroll_at(
        m,
        state.anchor_scroll_y,
        state.anchor_scroll_x,
        state.anchor_t_ms,
        now_ms,
    );
    // ADR-016 M1: помечаем кадр как self-tick — презентация продолжается, пока
    // UI-поток стоит; в LUMEN_FRAME_LOG это видно как `commit N self-tick`.
    backend.set_frame_commit_id(state.anchor_commit_id, true);
    let content = state.drawable(&Arc::clone(&state.last_content));
    if let Err(err) = backend.render(&content, &state.last_overlay, scroll_y, scroll_x) {
        eprintln!("[render-thread] ошибка self-tick momentum: {err:?}");
    }
    crate::present_log::present(state.anchor_commit_id, true);
    if done {
        state.momentum = None;
    }
}

/// Self-tick анимации щелчка колеса (THREAD-6): сэмплирует кривую по часам
/// рендер-потока и презентует последний закоммиченный кадр с новым `scroll_y`.
/// Вызывается только по таймауту (UI-поток молчит ≥ один тик).
fn self_tick_scroll_anim(
    backend: &mut Box<dyn RenderBackend>,
    state: &mut RenderState,
    now_ms: f64,
) {
    let Some(anim) = state.scroll_anim else {
        return;
    };
    if state.last_content.is_empty() {
        return;
    }
    let (scroll_y, done) = anim.sample(now_ms);
    backend.set_frame_commit_id(state.anchor_commit_id, true);
    let content = state.drawable(&Arc::clone(&state.last_content));
    if let Err(err) =
        backend.render(&content, &state.last_overlay, scroll_y, state.anchor_scroll_x)
    {
        eprintln!("[render-thread] ошибка self-tick scroll-anim: {err:?}");
    }
    crate::present_log::present(state.anchor_commit_id, true);
    if done {
        state.scroll_anim = None;
    }
}

/// Обрабатывает одну пачку сообщений: применяет управляющие в порядке, рисует
/// только последний кадр пачки (устаревшие кадры отброшены) и обновляет
/// удержанное состояние (M1.3-якорь momentum). Возвращает `true`, если в пачке
/// был `Shutdown` (поток должен выйти).
///
/// Порядок строго сохраняется: кадр рисуется на своей позиции в пачке, поэтому
/// управляющие сообщения до кадра (canvas_bg / page_offset / scale) применяются
/// раньше него, а пришедшие после — уже к следующему кадру.
fn process_batch(
    backend: &mut Box<dyn RenderBackend>,
    batch: Vec<RenderMsg>,
    state: &mut RenderState,
    now_ms: f64,
) -> bool {
    let last_frame_idx = last_frame_index(&batch);
    for (i, msg) in batch.into_iter().enumerate() {
        match msg {
            RenderMsg::Frame(frame) => {
                // Рисуем только последний кадр пачки; ранние отброшены (latest-wins).
                if Some(i) == last_frame_idx {
                    // THREAD-6 срез 6: пока идёт кривая щелчка, положение ведёт
                    // рендер-поток — `scroll_y` кадра UI мог быть снят до того,
                    // как UI-поток встал в долгий кадр, и тащил бы страницу назад.
                    let mut frame = frame;
                    state.resolve_frame_scroll(&mut frame, now_ms);
                    state.frame_adopted = frame.adopted_gen;
                    state.prune_containers();
                    // Пока кривая или инерция колеса идёт, презентует тик
                    // рендер-потока: кадр потока браузера со прежним списком
                    // лишь обновляет overlay. Вторая презентация на тик
                    // упёрлась бы в vsync и вдвое сбила бы темп. Новый список
                    // рисуется сразу: бэкенд кэширует разницу между
                    // соседними `render`, и пропуск версий давал полную
                    // перерисовку по ~90 мс на lenta.ru.
                    let scroll_only = Arc::ptr_eq(&frame.content, &state.last_content);
                    if !(state.owned && state.driving() && scroll_only) {
                        state.presented = true;
                        // ADR-016 M1: аннотируем кадр в LUMEN_FRAME_LOG (не self-tick).
                        backend.set_frame_commit_id(frame.commit_id, false);
                        let content = state.drawable(&frame.content);
                        if let Err(err) = backend.render(
                            &content,
                            &frame.overlay,
                            frame.scroll_y,
                            frame.scroll_x,
                        ) {
                            eprintln!(
                                "[render-thread] ошибка рендера (commit {}): {err:?}",
                                frame.commit_id
                            );
                        }
                        crate::present_log::present(frame.commit_id, false);
                    }
                    // Удерживаем кадр как якорь momentum (M1.3): UI-поток жив и
                    // ведёт презентацию — обновляем базу, чтобы при последующем
                    // застое продолжить инерцию с актуальной позиции.
                    state.last_content = frame.content;
                    state.last_overlay = frame.overlay;
                    state.anchor_scroll_y = frame.scroll_y;
                    state.anchor_scroll_x = frame.scroll_x;
                    state.anchor_t_ms = now_ms;
                    state.anchor_commit_id = frame.commit_id;
                }
            }
            RenderMsg::StartRenderMomentum { vel_y, vel_x, max_scroll_y, max_scroll_x } => {
                state.owned = false;
                state.momentum = Some(RenderMomentum {
                    v0_y: vel_y,
                    v0_x: vel_x,
                    t0_ms: now_ms,
                    max_y: max_scroll_y,
                    max_x: max_scroll_x,
                });
            }
            RenderMsg::StopRenderMomentum => {
                state.momentum = None;
                state.scroll_anim = None;
                state.owned = false;
            }
            RenderMsg::Snapshot(snap) => {
                state.snap = snap;
                state.prune_containers();
            }
            RenderMsg::Wheel { input, max_y, max_x, at } => {
                if state.apply_wheel_at(input, max_y, max_x, at, now_ms) {
                    state.publish();
                    present_owned(backend, state);
                }
            }
            RenderMsg::AttachScrollLink(shared) => state.shared = Some(shared),
            RenderMsg::StartRenderScrollAnim { start_y, target_y } => {
                state.owned = false;
                state.momentum = None;
                state.scroll_anim = Some(crate::scroll_anim::ScrollAnim {
                    start_y,
                    target_y,
                    start_time_ms: now_ms,
                });
            }
            RenderMsg::Resize { width, height } => backend.resize(width, height),
            RenderMsg::SetScaleFactor(s) => backend.set_scale_factor(s),
            RenderMsg::SetCanvasBackground(c) => backend.set_canvas_background(c),
            RenderMsg::SetPreviewScale(s) => backend.set_preview_scale(s),
            RenderMsg::SetPageOffset { x, y } => backend.set_page_offset(x, y),
            RenderMsg::RegisterImage { src, image } => {
                if let Err(e) = backend.register_image(src, image) {
                    eprintln!("[render-thread] register_image: {e}");
                }
            }
            RenderMsg::ClearImages => backend.clear_images(),
            RenderMsg::RegisterSnapshot { id, image } => {
                if let Err(e) = backend.register_snapshot(id, &image) {
                    eprintln!("[render-thread] register_snapshot: {e}");
                }
            }
            RenderMsg::ClearSnapshots => backend.clear_snapshots(),
            RenderMsg::SetFontProvider(p) => backend.set_font_provider(p),
            RenderMsg::PreloadCuratedFallbacks => backend.preload_curated_fallbacks(),
            RenderMsg::LayerMemoryPressure(l) => backend.on_layer_memory_pressure(l),
            RenderMsg::AtlasMemoryPressure(l) => backend.on_atlas_memory_pressure(l),
            RenderMsg::PromoteLayer { node_id, width, height } => {
                backend.promote_layer(node_id, width, height);
            }
            RenderMsg::DemoteLayer { node_id } => backend.demote_layer(node_id),
            RenderMsg::Shutdown => return true,
        }
    }
    false
}

/// `scroll_y` для кадра UI: при активной кривой щелчка — её значение по часам
/// рендер-потока (завершённая кривая сбрасывается), иначе — значение кадра.
fn frame_scroll_y(state: &mut RenderState, frame_y: f32, now_ms: f64) -> f32 {
    let Some(anim) = state.scroll_anim else {
        return frame_y;
    };
    let (y, done) = anim.sample(now_ms);
    if done {
        state.scroll_anim = None;
    }
    y
}

/// Индекс последнего кадра в пачке (latest-wins): только он рисуется.
fn last_frame_index(batch: &[RenderMsg]) -> Option<usize> {
    batch
        .iter()
        .rposition(|m| matches!(m, RenderMsg::Frame(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(commit_id: u64) -> RenderMsg {
        RenderMsg::Frame(FrameCommit {
            content: Arc::new(Vec::new()),
            overlay: Vec::new(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            commit_id,
            ack_gen: 0,
            adopted_gen: 0,
        })
    }

    fn fill() -> DisplayCommand {
        DisplayCommand::FillRect {
            rect: lumen_core::geom::Rect::new(0.0, 0.0, 1.0, 1.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        }
    }

    #[test]
    fn same_content_epoch_shares_snapshot_without_copy() {
        let mut b = ThreadedRenderBackend::new(|| {
            Ok(Box::new(crate::no_paint_backend::NoPaintBackend::new(100, 100, 1.0))
                as Box<dyn RenderBackend>)
        })
        .expect("spawn");
        let list = vec![fill(), fill()];
        b.set_content_epoch(5);
        b.render(&list, &[], 0.0, 0.0).unwrap();
        let first = Arc::clone(&b.sent_content.as_ref().unwrap().1);
        b.set_content_epoch(5);
        b.render(&list, &[], 10.0, 0.0).unwrap();
        assert!(Arc::ptr_eq(&first, &b.sent_content.as_ref().unwrap().1));
        // Новая версия — новый снимок.
        b.set_content_epoch(6);
        b.render(&list, &[], 10.0, 0.0).unwrap();
        assert!(!Arc::ptr_eq(&first, &b.sent_content.as_ref().unwrap().1));
        // Версия не объявлена (производный список) — копия, кэша нет.
        b.render(&list, &[], 10.0, 0.0).unwrap();
        assert!(b.sent_content.is_none());
    }

    #[test]
    fn last_frame_index_picks_latest_frame() {
        // Пачка: control, frame#1, control, frame#2, control — рисуется frame#2.
        let batch = vec![
            RenderMsg::ClearImages,
            frame(1),
            RenderMsg::Resize { width: 800, height: 600 },
            frame(2),
            RenderMsg::SetScaleFactor(2.0),
        ];
        assert_eq!(last_frame_index(&batch), Some(3));
    }

    #[test]
    fn last_frame_index_none_without_frames() {
        let batch = vec![RenderMsg::ClearImages, RenderMsg::ClearSnapshots];
        assert_eq!(last_frame_index(&batch), None);
    }

    #[test]
    fn last_frame_index_single_frame() {
        let batch = vec![frame(7)];
        assert_eq!(last_frame_index(&batch), Some(0));
    }

    #[test]
    fn last_frame_index_coalesces_many_frames() {
        // Десять кадров подряд без управляющих — рисуется только последний.
        let batch: Vec<RenderMsg> = (0..10).map(frame).collect();
        assert_eq!(last_frame_index(&batch), Some(9));
    }

    fn momentum(v0_y: f32, max_y: f32) -> RenderMomentum {
        RenderMomentum { v0_y, v0_x: 0.0, t0_ms: 0.0, max_y, max_x: 0.0 }
    }

    #[test]
    fn momentum_scroll_advances_downward() {
        // Инерция вниз из позиции 100 продвигает скролл вперёд.
        let m = momentum(1.0, 10_000.0);
        let (y0, _, _) = momentum_scroll_at(&m, 100.0, 0.0, 0.0, 0.0);
        let (y1, _, _) = momentum_scroll_at(&m, 100.0, 0.0, 0.0, 100.0);
        assert!((y0 - 100.0).abs() < 0.01, "y0={y0}");
        assert!(y1 > y0, "y1={y1} должно быть > y0={y0}");
    }

    #[test]
    fn momentum_scroll_clamps_at_bottom() {
        // Клампится в max, не улетает за край.
        let m = momentum(5.0, 50.0);
        let (y, _, _) = momentum_scroll_at(&m, 40.0, 0.0, 0.0, 1000.0);
        assert!(y <= 50.0 + f32::EPSILON, "y={y}");
    }

    #[test]
    fn momentum_scroll_clamps_at_top_for_negative_velocity() {
        // Инерция вверх не уводит скролл ниже нуля.
        let m = RenderMomentum { v0_y: -5.0, v0_x: 0.0, t0_ms: 0.0, max_y: 1000.0, max_x: 0.0 };
        let (y, _, _) = momentum_scroll_at(&m, 20.0, 0.0, 0.0, 1000.0);
        assert!(y >= 0.0, "y={y}");
    }

    #[test]
    fn momentum_scroll_reports_done_when_decayed() {
        // За большое время скорость падает ниже порога → done.
        let m = momentum(1.0, 10_000.0);
        let (_, _, done_early) = momentum_scroll_at(&m, 0.0, 0.0, 0.0, 1.0);
        let (_, _, done_late) = momentum_scroll_at(&m, 0.0, 0.0, 0.0, 5_000.0);
        assert!(!done_early);
        assert!(done_late);
    }

    #[test]
    fn momentum_scroll_continues_from_anchor() {
        // Якорь позже старта: скорость уже затухла, но смещение всё ещё вперёд.
        let m = momentum(2.0, 100_000.0);
        let (y, _, _) = momentum_scroll_at(&m, 500.0, 0.0, 200.0, 250.0);
        assert!(y > 500.0, "y={y} должно продолжать от якоря 500");
    }

    fn tick_state() -> (Box<dyn RenderBackend>, RenderState) {
        let backend: Box<dyn RenderBackend> =
            Box::new(crate::no_paint_backend::NoPaintBackend::new(100, 100, 1.0));
        let mut state = RenderState::new();
        state.last_content = Arc::new(vec![DisplayCommand::FillRect {
            rect: lumen_core::geom::Rect::new(0.0, 0.0, 1.0, 1.0),
            color: Color { r: 0, g: 0, b: 0, a: 255 },
        }]);
        (backend, state)
    }

    #[test]
    fn scroll_anim_self_tick_runs_to_completion_and_clears() {
        let (mut backend, mut state) = tick_state();
        let batch = vec![RenderMsg::StartRenderScrollAnim { start_y: 0.0, target_y: 100.0 }];
        process_batch(&mut backend, batch, &mut state, 10.0);
        assert!(state.scroll_anim.is_some());
        self_tick_scroll_anim(&mut backend, &mut state, 50.0);
        assert!(state.scroll_anim.is_some(), "анимация ещё идёт");
        self_tick_scroll_anim(&mut backend, &mut state, 10.0 + crate::scroll_anim::DURATION_MS + 1.0);
        assert!(state.scroll_anim.is_none(), "по завершении сбрасывается");
    }

    #[test]
    fn frame_scroll_y_follows_active_anim() {
        let mut state = RenderState::new();
        assert_eq!(frame_scroll_y(&mut state, 7.0, 0.0), 7.0, "без кривой — значение кадра");
        state.scroll_anim =
            Some(crate::scroll_anim::ScrollAnim { start_y: 0.0, target_y: 100.0, start_time_ms: 0.0 });
        let mid = frame_scroll_y(&mut state, 0.0, 50.0);
        assert!(mid > 0.0 && mid < 100.0, "mid={mid}");
        assert!(state.scroll_anim.is_some());
        let end = frame_scroll_y(&mut state, 0.0, crate::scroll_anim::DURATION_MS + 1.0);
        assert_eq!(end, 100.0);
        assert!(state.scroll_anim.is_none());
    }

    #[test]
    fn stop_message_cancels_scroll_anim() {
        let (mut backend, mut state) = tick_state();
        let batch = vec![
            RenderMsg::StartRenderScrollAnim { start_y: 0.0, target_y: 100.0 },
            RenderMsg::StopRenderMomentum,
        ];
        process_batch(&mut backend, batch, &mut state, 0.0);
        assert!(state.scroll_anim.is_none());
    }

    // --- ADR-032, срез 3: колесо ведёт рендер-поток ---

    fn wheel_state() -> RenderState {
        let mut st = RenderState::new();
        st.max_y = 10_000.0;
        st
    }

    fn browser_frame(y: f32, ack_gen: u64) -> FrameCommit {
        FrameCommit {
            content: Arc::new(Vec::new()),
            overlay: Vec::new(),
            scroll_y: y,
            scroll_x: 0.0,
            commit_id: 1,
            ack_gen,
            adopted_gen: ack_gen,
        }
    }

    #[test]
    fn notch_starts_curve_and_advances_without_browser() {
        let mut st = wheel_state();
        assert!(!st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 120.0 }, 10_000.0, 0.0, 0.0));
        assert!(st.owned && st.driving());
        assert!(st.advance(100.0));
        assert!(st.cur_y > 0.0 && st.cur_y < 120.0, "cur_y={}", st.cur_y);
        st.advance(crate::scroll_anim::DURATION_MS + 1.0);
        assert_eq!(st.cur_y, 120.0);
        assert!(!st.driving());
    }

    #[test]
    fn repeated_notch_extends_target_instead_of_rolling_back() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 120.0 }, 10_000.0, 0.0, 0.0);
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 120.0 }, 10_000.0, 0.0, 50.0);
        st.advance(1000.0);
        assert_eq!(st.cur_y, 240.0);
    }

    #[test]
    fn notch_clamps_to_page_end() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 500.0 }, 200.0, 0.0, 0.0);
        st.advance(1000.0);
        assert_eq!(st.cur_y, 200.0);
        // На пределе колесо ничего не запускает.
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 40.0 }, 200.0, 0.0, 1000.0);
        assert!(!st.driving());
    }

    #[test]
    fn horizontal_notch_moves_instantly() {
        let mut st = wheel_state();
        assert!(st.apply_wheel(WheelInput::Notch { dx: 40.0, dy: 0.0 }, 10_000.0, 500.0, 0.0));
        assert_eq!(st.cur_x, 40.0);
    }

    #[test]
    fn touch_end_with_velocity_starts_momentum() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::TouchStart { dx: 0.0, dy: 5.0 }, 10_000.0, 0.0, 0.0);
        st.apply_wheel(WheelInput::TouchMove { dx: 0.0, dy: 40.0 }, 10_000.0, 0.0, 10.0);
        st.apply_wheel(WheelInput::TouchEnd, 10_000.0, 0.0, 20.0);
        assert!(st.momentum.is_some(), "быстрый жест запускает инерцию");
        let before = st.cur_y;
        st.advance(60.0);
        assert!(st.cur_y > before);
    }

    #[test]
    fn slow_touch_end_has_no_momentum() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::TouchStart { dx: 0.0, dy: 0.0 }, 10_000.0, 0.0, 0.0);
        st.apply_wheel(WheelInput::TouchEnd, 10_000.0, 0.0, 500.0);
        assert!(st.momentum.is_none());
    }

    #[test]
    fn stale_browser_frame_is_drawn_at_render_offset() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 120.0 }, 10_000.0, 0.0, 0.0);
        st.advance(1000.0);
        st.publish(); // поколение 1, поток браузера ещё не усыновил
        let mut f = browser_frame(0.0, 0);
        st.resolve_frame_scroll(&mut f, 1000.0);
        assert_eq!(f.scroll_y, 120.0, "кадр со старым смещением не откатывает страницу");
        assert!(st.owned);
    }

    #[test]
    fn adopted_frame_hands_ownership_back() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 120.0 }, 10_000.0, 0.0, 0.0);
        st.advance(1000.0);
        st.publish();
        let mut f = browser_frame(120.0, st.gen_id);
        st.resolve_frame_scroll(&mut f, 1000.0);
        assert!(!st.owned, "поток браузера догнал — владеет им");
        assert_eq!(st.cur_y, 120.0);
    }

    #[test]
    fn browser_set_offset_overrides_render_ownership() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 120.0 }, 10_000.0, 0.0, 0.0);
        let mut f = browser_frame(0.0, ACK_BROWSER_SET);
        st.resolve_frame_scroll(&mut f, 10.0);
        assert!(!st.owned && !st.driving(), "навигация/клавиатура отменяют кривую колеса");
        assert_eq!(f.scroll_y, 0.0);
    }

    #[test]
    fn running_curve_beats_adopted_frame() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 400.0 }, 10_000.0, 0.0, 0.0);
        st.advance(50.0);
        st.publish();
        let mut f = browser_frame(st.cur_y, st.gen_id);
        st.resolve_frame_scroll(&mut f, 100.0);
        assert!(st.owned, "кривая ещё идёт");
        assert!(f.scroll_y > 0.0 && f.scroll_y < 400.0);
    }

    #[test]
    fn ack_for_frame_tells_redraw_from_browser_scroll() {
        let mut b = ThreadedRenderBackend::new(|| {
            Ok(Box::new(crate::no_paint_backend::NoPaintBackend::new(100, 100, 1.0))
                as Box<dyn RenderBackend>)
        })
        .expect("spawn");
        let link = take_last_link().expect("ручка");
        assert_eq!(b.ack_for_frame(0.0, 0.0).0, 0);
        assert_eq!(b.ack_for_frame(0.0, 0.0).0, 0, "перерисовка без сдвига");
        assert_eq!(b.ack_for_frame(300.0, 0.0).0, ACK_BROWSER_SET, "браузер сдвинул сам");
        link.adopt(7, 500.0, 0.0);
        assert_eq!(b.ack_for_frame(500.0, 0.0).0, 7, "усыновление не считается сдвигом браузера");
        assert_eq!(b.ack_for_frame(500.0, 0.0).0, 7);
    }

    #[test]
    fn last_curve_step_taken_inside_a_frame_is_not_lost() {
        let mut st = wheel_state();
        st.apply_wheel(WheelInput::Notch { dx: 0.0, dy: 120.0 }, 10_000.0, 0.0, 0.0);
        st.advance(100.0);
        st.publish();
        let adopted = st.gen_id;
        // Кривая закончилась, пока кадр потока браузера ехал к рендер-потоку.
        let mut f = browser_frame(st.cur_y, adopted);
        st.resolve_frame_scroll(&mut f, crate::scroll_anim::DURATION_MS + 50.0);
        assert_eq!(f.scroll_y, 120.0, "финальная точка, а не усыновлённая раньше");
        assert!(st.gen_id > adopted, "и она ушла обратной связью");
    }

    // --- ADR-032, срез 4: overflow-контейнеры ---

    fn container(id: usize, rect: [f32; 4], content_h: f32) -> lumen_layout::ScrollContainer {
        lumen_layout::ScrollContainer {
            node: lumen_dom::NodeId::from_index(id),
            clip_rect: lumen_core::geom::Rect::new(rect[0], rect[1], rect[2], rect[3]),
            scroll_width: rect[2],
            scroll_height: content_h,
            scroll_x: 0.0,
            scroll_y: 0.0,
            overscroll_behavior_x: lumen_layout::style::OverscrollBehavior::Auto,
            overscroll_behavior_y: lumen_layout::style::OverscrollBehavior::Auto,
        }
    }

    fn container_state(containers: Vec<lumen_layout::ScrollContainer>) -> RenderState {
        let mut st = wheel_state();
        st.snap = Arc::new(ScrollSnapshot { enabled: true, containers, ..ScrollSnapshot::default() });
        st
    }

    fn scroll_layer(id: u32, y: f32) -> DisplayCommand {
        DisplayCommand::PushScrollLayer {
            id,
            clip_rect: lumen_core::geom::Rect::new(0.0, 0.0, 100.0, 100.0),
            scroll_x: 0.0,
            scroll_y: y,
        }
    }

    #[test]
    fn notch_over_container_moves_it_and_not_the_page() {
        let mut st = container_state(vec![container(5, [0.0, 0.0, 100.0, 100.0], 400.0)]);
        let moved = st.apply_wheel_at(
            WheelInput::Notch { dx: 0.0, dy: 40.0 },
            10_000.0,
            0.0,
            Some((50.0, 50.0)),
            0.0,
        );
        assert!(moved, "контейнер сдвигается сразу");
        assert_eq!(st.cur_y, 0.0, "страница на месте");
        assert!(!st.driving(), "кривой страницы нет");
        assert_eq!(st.owned_containers.len(), 1);
        assert_eq!((st.owned_containers[0].id, st.owned_containers[0].y), (5, 40.0));
    }

    #[test]
    fn notch_outside_container_scrolls_the_page() {
        let mut st = container_state(vec![container(5, [0.0, 0.0, 100.0, 100.0], 400.0)]);
        st.apply_wheel_at(WheelInput::Notch { dx: 0.0, dy: 40.0 }, 10_000.0, 0.0, Some((500.0, 500.0)), 0.0);
        assert!(st.owned_containers.is_empty());
        assert!(st.driving(), "кривая страницы пошла");
    }

    #[test]
    fn container_at_boundary_hands_the_wheel_to_the_page_and_contain_swallows_it() {
        let mut c = container(5, [0.0, 0.0, 100.0, 100.0], 400.0);
        c.scroll_y = 300.0;
        let mut st = container_state(vec![c.clone()]);
        // Предел достигнут, `auto` — дельта уходит странице.
        st.apply_wheel_at(WheelInput::Notch { dx: 0.0, dy: 40.0 }, 10_000.0, 0.0, Some((50.0, 50.0)), 0.0);
        assert!(st.owned_containers.is_empty());
        assert!(st.driving());
        // `contain` — жест гасится на месте.
        c.overscroll_behavior_y = lumen_layout::style::OverscrollBehavior::Contain;
        let mut st = container_state(vec![c]);
        let moved = st.apply_wheel_at(
            WheelInput::Notch { dx: 0.0, dy: 40.0 },
            10_000.0,
            0.0,
            Some((50.0, 50.0)),
            0.0,
        );
        assert!(!moved);
        assert!(!st.driving());
    }

    #[test]
    fn owned_offset_beats_snapshot_offset_for_the_next_notch() {
        let mut st = container_state(vec![container(5, [0.0, 0.0, 100.0, 100.0], 400.0)]);
        for _ in 0..2 {
            st.apply_wheel_at(WheelInput::Notch { dx: 0.0, dy: 40.0 }, 10_000.0, 0.0, Some((50.0, 50.0)), 0.0);
            st.publish();
        }
        assert_eq!(st.owned_containers[0].y, 80.0, "второй щелчок идёт от первого, а не от снимка");
    }

    #[test]
    fn drawable_patches_scroll_layers_until_adopted() {
        let mut st = container_state(vec![container(5, [0.0, 0.0, 100.0, 100.0], 400.0)]);
        let raw = Arc::new(vec![scroll_layer(5, 0.0), DisplayCommand::PopScrollLayer]);
        assert!(Arc::ptr_eq(&st.drawable(&raw), &raw), "без смещений список не копируется");
        st.apply_wheel_at(WheelInput::Notch { dx: 0.0, dy: 40.0 }, 10_000.0, 0.0, Some((50.0, 50.0)), 0.0);
        st.publish();
        let drawn = st.drawable(&raw);
        assert!(matches!(drawn[0], DisplayCommand::PushScrollLayer { scroll_y, .. } if scroll_y == 40.0));
        assert!(Arc::ptr_eq(&st.drawable(&raw), &drawn), "кэш пока смещения не менялись");
        // Кадр потока браузера, снятый до усыновления, не откатывает контейнер.
        st.frame_adopted = 0;
        st.prune_containers();
        assert_eq!(st.owned_containers.len(), 1);
        // Усыновил и кадр, и снимок — смещение отдано списку.
        st.frame_adopted = st.gen_id;
        st.snap = Arc::new(ScrollSnapshot { adopted_gen: st.gen_id, ..(*st.snap).clone() });
        st.prune_containers();
        assert!(st.owned_containers.is_empty());
        assert!(Arc::ptr_eq(&st.drawable(&raw), &raw));
    }

    #[test]
    fn snapshot_alone_does_not_release_an_offset_the_frame_lacks() {
        let mut st = container_state(vec![container(5, [0.0, 0.0, 100.0, 100.0], 400.0)]);
        st.apply_wheel_at(WheelInput::Notch { dx: 0.0, dy: 40.0 }, 10_000.0, 0.0, Some((50.0, 50.0)), 0.0);
        st.publish();
        st.snap = Arc::new(ScrollSnapshot { adopted_gen: st.gen_id, ..(*st.snap).clone() });
        st.prune_containers();
        assert_eq!(st.owned_containers.len(), 1, "кадр с усыновленным списком ещё не пришёл");
    }
}
