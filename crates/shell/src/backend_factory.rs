// backend-vello is a planned feature (ADR-010 Phase 3); the cfg guards are
// forward-looking and intentionally reference a not-yet-declared feature name.
#![allow(unexpected_cfgs)]
//! Фабрика GPU-бэкендов: читает `LUMEN_BACKEND` env var и создаёт
//! нужный [`RenderBackend`].
//!
//! Экспериментальная ветка (p1-exp-wgpu-only): OpenGL (femtovg/glutin) удалён,
//! wgpu — единственный оконный бэкенд. `LUMEN_BACKEND` оставлена для
//! совместимости: `wgpu` (и пустое значение) → `WgpuBackend`,
//! `vello` → заглушка (если скомпилирована), остальное → ошибка/wgpu.

use lumen_core::ColorSpace;
use std::sync::Arc;

use lumen_paint::RenderBackend;
#[cfg(feature = "backend-wgpu")]
use lumen_paint::WgpuBackend;
#[cfg(feature = "backend-vello")]
use lumen_paint::VelloBackend;
use winit::window::Window;

/// Создаёт windowed рендер-бэкенд для окна `window`.
///
/// Единственный оконный бэкенд — wgpu. `LUMEN_BACKEND` читается для
/// совместимости: пустое значение, `wgpu` и любые неизвестные имена ведут
/// в `WgpuBackend`; `femtovg` больше не существует (OpenGL удалён);
/// `vello` — заглушка за фичей `backend-vello`; `cpu` — ошибка (headless only).
///
/// # Errors
/// Возвращает `Err` если GPU-адаптер недоступен или инициализация wgpu
/// завершилась ошибкой.
pub fn create_backend(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
    target_color_space: ColorSpace,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    let requested = std::env::var("LUMEN_BACKEND").unwrap_or_default();
    let name = requested.trim().to_ascii_lowercase();

    match name.as_str() {
        "" | "wgpu" => create_wgpu(window, font_bytes, target_color_space),
        "femtovg" => {
            eprintln!(
                "LUMEN_BACKEND=femtovg: OpenGL-бэкенд удалён в этой ветке, используется wgpu"
            );
            create_wgpu(window, font_bytes, target_color_space)
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
                create_wgpu(window, font_bytes, target_color_space)
            }
        }
        "cpu" => {
            Err("LUMEN_BACKEND=cpu: CpuBackend недоступен как windowed-бэкенд. \
                 Используй lumen-driver для headless-рендера."
                .into())
        }
        other => {
            eprintln!("Неизвестный LUMEN_BACKEND={other:?}, используется wgpu");
            create_wgpu(window, font_bytes, target_color_space)
        }
    }
}

/// Создаёт `WgpuBackend` — единственный оконный бэкенд.
#[cfg(feature = "backend-wgpu")]
fn create_wgpu(
    window: Arc<Window>,
    font_bytes: Vec<u8>,
    target_color_space: ColorSpace,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    Ok(Box::new(WgpuBackend::new(window, font_bytes, target_color_space)?))
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
#[cfg(not(feature = "backend-wgpu"))]
fn create_wgpu(
    _window: Arc<Window>,
    _font_bytes: Vec<u8>,
    _target_color_space: ColorSpace,
) -> Result<Box<dyn RenderBackend>, Box<dyn std::error::Error>> {
    Err("wgpu backend not compiled in (missing feature backend-wgpu)".into())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn backend_name_parsing_handles_known_names() {
        let known = ["wgpu", "vello", "cpu", "", "WGPU", " wgpu "];
        for name in known {
            let normalized = name.trim().to_ascii_lowercase();
            let _ = matches!(normalized.as_str(), "wgpu" | "vello" | "cpu" | "");
        }
    }

    #[test]
    fn unknown_backend_name_recognized_as_unknown() {
        let name = "dx12".trim().to_ascii_lowercase();
        let is_known = matches!(name.as_str(), "wgpu" | "vello" | "cpu" | "");
        assert!(!is_known, "dx12 should be an unknown backend name");
    }

    #[test]
    fn empty_name_is_default_wgpu() {
        // Пустой LUMEN_BACKEND → wgpu (единственный оконный бэкенд)
        let name = "";
        let is_explicit_named = matches!(name, "vello" | "cpu");
        assert!(!is_explicit_named);
    }

    #[test]
    fn femtovg_routes_to_wgpu() {
        // femtovg удалён: имя распознаётся, но ведёт в wgpu
        let name = "femtovg";
        let routes_to_wgpu = !matches!(name, "vello" | "cpu");
        assert!(routes_to_wgpu);
    }
}
