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

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lumen_core::ext::{FontProvider, MemoryPressureLevel};
use lumen_core::geom::Size;
use lumen_image::Image;
use lumen_layout::Color;
use lumen_paint::{DisplayCommand, RenderBackend, RenderError};

use crate::momentum_anim;

/// Бюджет кадра для self-tick momentum (~60 fps). При активном render-side
/// momentum поток ждёт сообщения не дольше этого; таймаут = UI-поток ничего не
/// прислал за интервал → он застопорился → продолжаем инерцию сами.
const MOMENTUM_TICK: Duration = Duration::from_millis(16);

/// Один кадр, переданный рендер-потоку. Владеющая копия — снапшот, который
/// поток рисует независимо от main (ADR-016 инвариант 1).
struct FrameCommit {
    /// Команды страницы (уже с применённым scroll на стороне рендера).
    content: Vec<DisplayCommand>,
    /// Команды поверх страницы (tab bar, панели, pop-up'ы).
    overlay: Vec<DisplayCommand>,
    /// Текущий вертикальный скролл в CSS px.
    scroll_y: f32,
    /// Текущий горизонтальный скролл в CSS px.
    scroll_x: f32,
    /// Монотонный идентификатор коммита (для диагностики / frame-log).
    commit_id: u64,
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
    /// Отмена render-side momentum (новый жест / навигация).
    StopRenderMomentum,
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

        Ok(Self {
            tx,
            join: Some(join),
            scale: caps.scale,
            phys_w: caps.phys_w,
            phys_h: caps.phys_h,
            supports_page_offset: caps.supports_page_offset,
            commit_counter: 0,
        })
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
        let frame = FrameCommit {
            content: content.to_vec(),
            overlay: overlay.to_vec(),
            scroll_y,
            scroll_x,
            commit_id: self.commit_counter,
        };
        self.send(RenderMsg::Frame(frame));
        // Fire-and-forget latest-wins: main не ждёт present (ADR-016 инвариант 4).
        Ok(())
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

/// Удержанное между пачками состояние рендер-потока (ADR-016 M1.3). Позволяет
/// продолжать momentum-презентацию из последнего закоммиченного кадра, когда
/// UI-поток застопорился и новых кадров нет.
struct RenderState {
    /// Последний закоммиченный контент страницы (для повторной презентации).
    last_content: Vec<DisplayCommand>,
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
}

impl RenderState {
    /// Пустое состояние: кадров ещё не было, momentum неактивен.
    fn new() -> Self {
        Self {
            last_content: Vec::new(),
            last_overlay: Vec::new(),
            anchor_scroll_y: 0.0,
            anchor_scroll_x: 0.0,
            anchor_t_ms: 0.0,
            anchor_commit_id: 0,
            momentum: None,
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
    loop {
        let first = if state.momentum.is_some() {
            match rx.recv_timeout(MOMENTUM_TICK) {
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
                if process_batch(backend, batch, &mut state, now_ms) {
                    return; // получен Shutdown
                }
            }
            None => {
                // Таймаут при активном momentum: UI-поток молчит — тикаем сами.
                let now_ms = clock.elapsed().as_secs_f64() * 1000.0;
                self_tick_momentum(backend, &mut state, now_ms);
            }
        }
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
    if let Err(err) = backend.render(&state.last_content, &state.last_overlay, scroll_y, scroll_x) {
        eprintln!("[render-thread] ошибка self-tick momentum: {err:?}");
    }
    if done {
        state.momentum = None;
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
                    // ADR-016 M1: аннотируем кадр в LUMEN_FRAME_LOG (не self-tick).
                    backend.set_frame_commit_id(frame.commit_id, false);
                    if let Err(err) = backend.render(
                        &frame.content,
                        &frame.overlay,
                        frame.scroll_y,
                        frame.scroll_x,
                    ) {
                        eprintln!(
                            "[render-thread] ошибка рендера (commit {}): {err:?}",
                            frame.commit_id
                        );
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
                state.momentum = Some(RenderMomentum {
                    v0_y: vel_y,
                    v0_x: vel_x,
                    t0_ms: now_ms,
                    max_y: max_scroll_y,
                    max_x: max_scroll_x,
                });
            }
            RenderMsg::StopRenderMomentum => state.momentum = None,
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
            content: Vec::new(),
            overlay: Vec::new(),
            scroll_y: 0.0,
            scroll_x: 0.0,
            commit_id,
        })
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
}
