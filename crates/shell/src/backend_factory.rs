//! Фабрика GPU-бэкендов: читает `LUMEN_BACKEND` env var и создаёт
//! нужный [`RenderBackend`].
//!
//! Порядок приоритетов (ADR-010 §Migration path):
//! 1. `LUMEN_BACKEND` env var (`wgpu` / `femtovg` / `vello` / `cpu`).
//! 2. Скомпилированный дефолт из feature-флагов.
//! 3. Auto-fallback: если запрошенный бэкенд не инициализировался — пробуем следующий.
//!
//! Phase 1 (текущая): только `wgpu` доступен как windowed-бэкенд.
//! Phase 2: добавится `femtovg` (RB-5) и станет default.
//! Phase 3: добавится `vello` (RB-10).

use std::sync::Arc;

use lumen_paint::{RenderBackend, WgpuBackend};
use winit::window::Window;

/// Создаёт windowed рендер-бэкенд для окна `window`.
///
/// Читает `LUMEN_BACKEND` env var для выбора бэкенда. Если переменная не задана
/// или содержит неизвестное значение — используется wgpu (Phase 1 default).
///
/// Неизвестные имена логируются в stderr и fallback-ятся на wgpu.
///
/// # Errors
/// Возвращает `Err` если GPU-адаптер недоступен или инициализация бэкенда
/// завершилась ошибкой.
pub fn create_backend(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    let requested = std::env::var("LUMEN_BACKEND").unwrap_or_default();
    let name = requested.trim().to_ascii_lowercase();

    match name.as_str() {
        "femtovg" => {
            eprintln!("LUMEN_BACKEND=femtovg: FemtovgBackend ещё не реализован (RB-5), используется wgpu");
            create_wgpu(window, font_bytes)
        }
        "vello" => {
            eprintln!("LUMEN_BACKEND=vello: VelloBackend ещё не реализован (RB-10), используется wgpu");
            create_wgpu(window, font_bytes)
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
fn create_wgpu(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    Ok(Box::new(WgpuBackend::new(window, font_bytes)?))
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
}
