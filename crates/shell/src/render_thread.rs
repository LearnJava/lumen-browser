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
//! - Momentum/anim по-прежнему тикают на main (кадры производит main-поток);
//!   полная независимость ввода — M2. Present уже вне main.
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

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};

use lumen_core::ext::{FontProvider, MemoryPressureLevel};
use lumen_core::geom::Size;
use lumen_image::Image;
use lumen_layout::Color;
use lumen_paint::{DisplayCommand, RenderBackend, RenderError};

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
    /// Регистрация изображения под ключом.
    RegisterImage { src: String, image: Image },
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

    fn register_image(&mut self, src: String, image: &Image) -> Result<(), String> {
        // Fire-and-forget: результат загрузки в GPU не возвращается синхронно.
        self.send(RenderMsg::RegisterImage { src, image: image.clone() });
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

/// Цикл рендер-потока: блокирующий `recv()` (idle-park, инвариант 6), затем
/// дренаж всей пачки с коалесцингом кадров (latest-wins) и строгим порядком
/// управляющих сообщений.
fn run_render_loop(backend: &mut Box<dyn RenderBackend>, rx: &Receiver<RenderMsg>) {
    loop {
        // Паркуемся до первого сообщения (без polling).
        let Ok(first) = rx.recv() else {
            return; // канал закрыт — прокси дропнут
        };
        let mut batch = vec![first];
        // Дренируем всё, что уже в очереди, одним махом.
        loop {
            match rx.try_recv() {
                Ok(m) => batch.push(m),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if process_batch(backend, batch) {
            return; // получен Shutdown
        }
    }
}

/// Обрабатывает одну пачку сообщений: применяет управляющие в порядке, рисует
/// только последний кадр пачки (устаревшие кадры отброшены). Возвращает `true`,
/// если в пачке был `Shutdown` (поток должен выйти).
///
/// Порядок строго сохраняется: кадр рисуется на своей позиции в пачке, поэтому
/// управляющие сообщения до кадра (canvas_bg / page_offset / scale) применяются
/// раньше него, а пришедшие после — уже к следующему кадру.
fn process_batch(backend: &mut Box<dyn RenderBackend>, batch: Vec<RenderMsg>) -> bool {
    let last_frame_idx = last_frame_index(&batch);
    for (i, msg) in batch.into_iter().enumerate() {
        match msg {
            RenderMsg::Frame(frame) => {
                // Рисуем только последний кадр пачки; ранние отброшены (latest-wins).
                if Some(i) == last_frame_idx
                    && let Err(err) = backend.render(
                        &frame.content,
                        &frame.overlay,
                        frame.scroll_y,
                        frame.scroll_x,
                    )
                {
                    eprintln!(
                        "[render-thread] ошибка рендера (commit {}): {err:?}",
                        frame.commit_id
                    );
                }
            }
            RenderMsg::Resize { width, height } => backend.resize(width, height),
            RenderMsg::SetScaleFactor(s) => backend.set_scale_factor(s),
            RenderMsg::SetCanvasBackground(c) => backend.set_canvas_background(c),
            RenderMsg::SetPreviewScale(s) => backend.set_preview_scale(s),
            RenderMsg::SetPageOffset { x, y } => backend.set_page_offset(x, y),
            RenderMsg::RegisterImage { src, image } => {
                if let Err(e) = backend.register_image(src, &image) {
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
}
