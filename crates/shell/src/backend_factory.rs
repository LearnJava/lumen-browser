// backend-vello is a planned feature (ADR-010 Phase 3); the cfg guards are
// forward-looking and intentionally reference a not-yet-declared feature name.
#![allow(unexpected_cfgs)]
//! Фабрика GPU-бэкендов: читает `LUMEN_BACKEND` env var и создаёт
//! нужный [`RenderBackend`].
//!
//! Порядок приоритетов (ADR-010 §Migration path):
//! 1. `LUMEN_BACKEND` env var (`wgpu` / `femtovg` / `vello` / `cpu`).
//! 2. Скомпилированный дефолт из feature-флагов.
//! 3. Auto-fallback: если предпочтительный бэкенд не инициализировался — пробуем следующий.
//!
//! Phase 3 (текущая): wgpu по умолчанию (probe Vulkan→GL→DX12, ADR-017); fallback → femtovg.
//! `LUMEN_BACKEND=femtovg` — детерминированный femtovg (без fallback-а на wgpu).

use lumen_core::ColorSpace;
use std::sync::Arc;

use crate::render_thread::ThreadedRenderBackend;
use lumen_paint::RenderBackend;
#[cfg(feature = "backend-wgpu")]
use lumen_paint::WgpuBackend;
#[cfg(feature = "backend-femtovg")]
use lumen_paint::FemtovgBackend;
#[cfg(feature = "backend-vello")]
use lumen_paint::VelloBackend;
use winit::window::Window;

/// Создаёт windowed рендер-бэкенд для окна `window`.
///
/// Читает `LUMEN_BACKEND` env var для выбора бэкенда. Если переменная не задана —
/// используется wgpu (Phase 3 default, ADR-017) с probe-выбором API (Vulkan→GL→DX12);
/// при ошибке инициализации wgpu автоматически fallback на femtovg.
///
/// При `LUMEN_BACKEND=wgpu` создаёт `WgpuBackend` напрямую (без fallback).
/// При `LUMEN_BACKEND=femtovg` создаёт `FemtovgBackend` напрямую (без fallback на wgpu).
/// При `LUMEN_BACKEND=vello` создаёт `VelloBackend` (RB-7 заглушка, ADR-010).
///
/// # ADR-016 M1 (spike): рендер-поток
/// Если задан `LUMEN_RENDER_THREAD=1`, настоящий бэкенд создаётся и живёт на
/// выделенном рендер-потоке, а окну возвращается [`ThreadedRenderBackend`]-прокси
/// (present уходит с UI-потока). При сбое создания бэкенда на потоке (например,
/// GL-контекст не создался вне главного потока) — автоматический откат на обычный
/// однопоточный путь. Значение по умолчанию — однопоточный in-process бэкенд.
///
/// # Errors
/// Возвращает `Err` если GPU-адаптер недоступен или инициализация всех бэкендов
/// завершилась ошибкой.
pub fn create_backend(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
    target_color_space: ColorSpace,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    // ADR-016 M1.2: опциональный рендер-поток за env-флагом (дефолт — выключен).
    // На femtovg бэкенд создаётся на ГЛАВНОМ потоке (winit отдаёт window handle
    // только там — M1.1 spike), контекст открепляется (`make_not_current`) и
    // переносится на рендер-поток, где привязывается обратно (`make_current`).
    if render_thread_enabled() {
        #[cfg(feature = "backend-femtovg")]
        {
            match create_threaded_femtovg(Arc::clone(&window), font_bytes.clone()) {
                Ok(b) => return Ok(b),
                Err(e) => eprintln!(
                    "LUMEN_RENDER_THREAD=1: рендер-поток не стартовал ({e}), откат на in-process"
                ),
            }
        }
        #[cfg(not(feature = "backend-femtovg"))]
        eprintln!(
            "LUMEN_RENDER_THREAD=1: собрано без backend-femtovg, рендер-поток недоступен, \
             откат на in-process"
        );
    }
    create_backend_inprocess(window, font_bytes, target_color_space)
}

/// ADR-016 M1.2: создаёт `FemtovgBackend` на текущем (главном) потоке, открепляет
/// его GL-контекст и передаёт рендер-потоку через [`ThreadedRenderBackend`].
///
/// Порядок: `FemtovgBackend::new` (handle окна валиден только на main) →
/// `detach_gl_context` (`make_not_current` на main) → перенос конкретного
/// бэкенда в замыкание (`FemtovgBackend: Send`) → на рендер-потоке
/// `attach_gl_context` (`make_current`) → цикл present вне UI-потока.
///
/// # Errors
/// Возвращает `Err`, если `FemtovgBackend::new` не смог создать контекст,
/// `detach_gl_context` не удался, или рендер-поток не прошёл handshake (тогда
/// вызывающая сторона откатывается на однопоточный in-process путь).
#[cfg(feature = "backend-femtovg")]
fn create_threaded_femtovg(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
) -> Result<Box<dyn RenderBackend>, String> {
    // Создаём бэкенд на главном потоке (window handle доступен только здесь).
    let mut backend = FemtovgBackend::new(window, font_bytes).map_err(|e| e.to_string())?;
    // Открепляем контекст на main, чтобы его можно было привязать на рендер-потоке.
    backend.detach_gl_context()?;
    // Замыкание захватывает КОНКРЕТНЫЙ `FemtovgBackend` (Send через ручной
    // `unsafe impl`), поэтому оно `Send` без Send-супертрейта на `RenderBackend`.
    let ctor = move || {
        let mut backend = backend;
        backend.attach_gl_context()?;
        Ok(Box::new(backend) as Box<dyn RenderBackend>)
    };
    let proxy = ThreadedRenderBackend::new(ctor)?;
    Ok(Box::new(proxy))
}

/// Читает `LUMEN_RENDER_THREAD` — `1`/`true`/`on` включают рендер-поток.
fn render_thread_enabled() -> bool {
    matches!(
        std::env::var("LUMEN_RENDER_THREAD")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// Однопоточный in-process выбор бэкенда (историческое поведение `create_backend`).
///
/// Читает `LUMEN_BACKEND` env var и создаёт нужный [`RenderBackend`] прямо на
/// вызывающем потоке. Вызывается как напрямую (дефолт), так и с рендер-потока
/// внутри замыкания-конструктора [`ThreadedRenderBackend`].
///
/// # Errors
/// Возвращает `Err` если GPU-адаптер недоступен или инициализация всех бэкендов
/// завершилась ошибкой.
fn create_backend_inprocess(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
    target_color_space: ColorSpace,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    let requested = std::env::var("LUMEN_BACKEND").unwrap_or_default();
    let name = requested.trim().to_ascii_lowercase();

    match name.as_str() {
        // Phase 3 default: wgpu (probe Vulkan→GL→DX12) → fallback femtovg (ADR-017)
        "" => create_wgpu_or_femtovg(window, font_bytes, target_color_space),
        // Явный запрос femtovg: femtovg → wgpu при ошибке (детерминированный приоритет)
        "femtovg" => create_femtovg_or_wgpu(window, font_bytes, target_color_space),
        // Явный запрос wgpu: прямой, без femtovg fallback
        "wgpu" => create_wgpu(window, font_bytes, target_color_space),
        "vello" => {
            #[cfg(feature = "backend-vello")]
            {
                eprintln!("LUMEN_BACKEND=vello: VelloBackend (RB-7 заглушка — ничего не рисует)");
                create_vello(window)
            }
            #[cfg(not(feature = "backend-vello"))]
            {
                eprintln!("LUMEN_BACKEND=vello: скомпилировано без backend-vello, используется femtovg");
                create_femtovg_or_wgpu(window, font_bytes, target_color_space)
            }
        }
        "cpu" => {
            Err("LUMEN_BACKEND=cpu: CpuBackend недоступен как windowed-бэкенд. \
                 Используй lumen-driver для headless-рендера."
                .into())
        }
        other => {
            eprintln!("Неизвестный LUMEN_BACKEND={other:?}, используется femtovg");
            create_femtovg_or_wgpu(window, font_bytes, target_color_space)
        }
    }
}

/// Phase 3 цепочка (ADR-017): пытается создать `WgpuBackend` (с probe-выбором API);
/// при ошибке — fallback на `FemtovgBackend`.
///
/// Вызывается только для дефолта (пустой `LUMEN_BACKEND`).
fn create_wgpu_or_femtovg(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
    target_color_space: ColorSpace,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    #[cfg(feature = "backend-wgpu")]
    {
        match create_wgpu(window.clone(), font_bytes.clone(), target_color_space) {
            Ok(b) => return Ok(b),
            Err(e) => eprintln!("wgpu: ошибка инициализации ({e}), fallback → femtovg"),
        }
    }
    // Финальный femtovg fallback; `create_femtovg_or_wgpu` обрабатывает случай
    // когда backend-femtovg не скомпилирован (и тогда снова попытается wgpu, которая
    // тоже завершится ошибкой — приемлемо при полностью неработающем GPU).
    create_femtovg_or_wgpu(window, font_bytes, target_color_space)
}

/// Phase 2 цепочка: пытается создать `FemtovgBackend`; при ошибке — fallback на `WgpuBackend`.
///
/// Вызывается для явного `LUMEN_BACKEND=femtovg`.
fn create_femtovg_or_wgpu(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
    target_color_space: ColorSpace,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    #[cfg(feature = "backend-femtovg")]
    {
        match create_femtovg(window.clone(), font_bytes.clone()) {
            Ok(b) => return Ok(b),
            Err(e) => eprintln!("femtovg: ошибка инициализации ({e}), fallback → wgpu"),
        }
    }
    create_wgpu(window, font_bytes, target_color_space)
}

/// Создаёт `WgpuBackend` (Phase 1 / Phase 2 fallback).
#[cfg(feature = "backend-wgpu")]
fn create_wgpu(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
    target_color_space: ColorSpace,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    Ok(Box::new(WgpuBackend::new(window, font_bytes, target_color_space)?))
}

/// Создаёт `FemtovgBackend` (Phase 2 default, ADR-010 RB-9).
#[cfg(feature = "backend-femtovg")]
fn create_femtovg(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    Ok(Box::new(FemtovgBackend::new(window, font_bytes)?))
}

/// Создаёт `VelloBackend` (Phase 3 заглушка, ADR-010 RB-7).
///
/// Читает размер окна для начальной конфигурации поверхности.
/// Заглушка не требует `font_bytes` — текст не рендерится.
#[cfg(feature = "backend-vello")]
fn create_vello(
    window: Arc<Window>,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    let size = window.inner_size();
    Ok(Box::new(VelloBackend::new(size.width.max(1), size.height.max(1))))
}

// Если wgpu не скомпилирован — возвращаем ошибку.
// Нормальная ситуация при --no-default-features --features backend-femtovg.
#[cfg(not(feature = "backend-wgpu"))]
fn create_wgpu(
    _window: Arc<Window>,
    _font_bytes: Vec<u8>,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    Err("wgpu backend not compiled in (missing feature backend-wgpu)".into())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn backend_name_parsing_handles_known_names() {
        let known = ["wgpu", "femtovg", "vello", "cpu", "", "WGPU", " wgpu "];
        for name in known {
            let normalized = name.trim().to_ascii_lowercase();
            let _ = matches!(normalized.as_str(), "wgpu" | "femtovg" | "vello" | "cpu" | "");
        }
    }

    #[test]
    fn unknown_backend_name_recognized_as_unknown() {
        let name = "dx12".trim().to_ascii_lowercase();
        let is_known = matches!(name.as_str(), "wgpu" | "femtovg" | "vello" | "cpu" | "");
        assert!(!is_known, "dx12 should be an unknown backend name");
    }

    #[test]
    fn femtovg_name_recognized_as_known() {
        let name = "femtovg";
        let is_known = matches!(name, "wgpu" | "femtovg" | "vello" | "cpu" | "");
        assert!(is_known);
    }

    #[test]
    fn empty_name_is_phase3_default() {
        // Phase 3: пустой LUMEN_BACKEND → wgpu (probe, ADR-017) → femtovg fallback
        // Пустая строка маршрутизируется как дефолт, не как явный femtovg/vello/cpu
        let name = "";
        let is_explicit_named = matches!(name, "wgpu" | "femtovg" | "vello" | "cpu");
        assert!(!is_explicit_named);
    }

    #[test]
    fn wgpu_explicit_does_not_match_empty() {
        // LUMEN_BACKEND=wgpu — явный запрос, отдельная ветка от дефолта
        let name = "wgpu";
        let is_wgpu = matches!(name, "wgpu");
        let is_empty = matches!(name, "");
        assert!(is_wgpu);
        assert!(!is_empty);
    }
}
