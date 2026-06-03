//! Фабрика GPU-бэкендов: читает `LUMEN_BACKEND` env var и создаёт
//! нужный [`RenderBackend`].
//!
//! Порядок приоритетов (ADR-010 §Migration path):
//! 1. `LUMEN_BACKEND` env var (`wgpu` / `femtovg` / `vello` / `cpu`).
//! 2. Скомпилированный дефолт из feature-флагов.
//! 3. Auto-fallback: если запрошенный бэкенд не инициализировался — пробуем следующий.
//!
//! Phase 1 (текущая): wgpu по умолчанию; femtovg доступен через `LUMEN_BACKEND=femtovg`.
//! Phase 2: femtovg станет default (RB-9).
//! Phase 3: добавится vello (RB-10).

use std::sync::Arc;

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
/// Читает `LUMEN_BACKEND` env var для выбора бэкенда. Если переменная не задана
/// или содержит неизвестное значение — используется wgpu (Phase 1 default).
///
/// При `LUMEN_BACKEND=femtovg` создаёт `FemtovgBackend` (RB-5+, ADR-010 Phase 2).
/// При `LUMEN_BACKEND=vello` создаёт `VelloBackend` (RB-7 заглушка, ADR-010 Phase 3).
/// При неудаче инициализации — fallback на wgpu.
///
/// # Errors
/// Возвращает `Err` если GPU-адаптер недоступен или инициализация всех бэкендов
/// завершилась ошибкой.
pub fn create_backend(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    let requested = std::env::var("LUMEN_BACKEND").unwrap_or_default();
    let name = requested.trim().to_ascii_lowercase();

    match name.as_str() {
        "femtovg" => {
            #[cfg(feature = "backend-femtovg")]
            {
                create_femtovg(window.clone(), font_bytes.clone())
                    .or_else(|e| {
                        eprintln!("LUMEN_BACKEND=femtovg: ошибка инициализации ({e}), fallback → wgpu");
                        create_wgpu(window, font_bytes)
                    })
            }
            #[cfg(not(feature = "backend-femtovg"))]
            {
                eprintln!("LUMEN_BACKEND=femtovg: скомпилировано без backend-femtovg, используется wgpu");
                create_wgpu(window, font_bytes)
            }
        }
        "vello" => {
            #[cfg(feature = "backend-vello")]
            {
                eprintln!("LUMEN_BACKEND=vello: VelloBackend (RB-7 заглушка — ничего не рисует)");
                create_vello(window)
            }
            #[cfg(not(feature = "backend-vello"))]
            {
                eprintln!("LUMEN_BACKEND=vello: скомпилировано без backend-vello, используется wgpu");
                create_wgpu(window, font_bytes)
            }
        }
        "cpu" => {
            Err("LUMEN_BACKEND=cpu: CpuBackend недоступен как windowed-бэкенд. \
                 Используй lumen-driver для headless-рендера."
                .into())
        }
        "wgpu" | "" => create_wgpu(window, font_bytes),
        other => {
            eprintln!("Неизвестный LUMEN_BACKEND={other:?}, используется wgpu");
            create_wgpu(window, font_bytes)
        }
    }
}

/// Создаёт `WgpuBackend` (Phase 1 default).
#[cfg(feature = "backend-wgpu")]
fn create_wgpu(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    Ok(Box::new(WgpuBackend::new(window, font_bytes)?))
}

/// Создаёт `FemtovgBackend` (Phase 2, ADR-010 RB-5).
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

// Fallback: если компилируем без backend-wgpu (не должно происходить в production,
// но нужно для компиляции с --no-default-features --features backend-femtovg).
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
        // Проверяем что разбор имён не паникует на известных значениях
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
}
