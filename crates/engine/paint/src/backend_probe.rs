//! Ярус 0 экспериментальной ветки: авто-проба wgpu-бэкенда при старте.
//!
//! Порядок кандидатов: **Vulkan** (быстрейший API на исправном драйвере:
//! прогретый кадр 7 мс против 450–950 мс на DX12, bugs/BUG-274-OPEN.md) →
//! **GL** (лучший idle-CPU на Intel Iris Plus) → **DX12** (резерв —
//! корректная картинка везде, но патология ~2.3 мс CPU на закрытие каждого
//! render pass). Для каждого кандидата рисуется пробный кадр (clear в
//! характерный цвет) прямо в поверхность окна, затем снимаются два сигнала:
//!
//! 1. **texture readback** — копия центра surface-текстуры в staging-буфер:
//!    проверяет, что рендер вообще пишет пиксели (ловит сломанный
//!    device / render pass);
//! 2. **захват презентации** — `PrintWindow(PW_CLIENTONLY |
//!    PW_RENDERFULLCONTENT)` клиентской области (Windows): проверяет, что
//!    presented-кадр дошёл до DWM. Именно этот сигнал ловит BUG-275
//!    (Vulkan-окно презентует белым при «исправном» по всем логам рендере —
//!    WSI-глюк драйвера Intel), который texture-readback может пропустить.
//!
//! Кандидат принимается, если презентация совпала с пробным цветом; при
//! недоступном захвате (не Windows / GDI-сбой) — если совпал readback.
//!
//! Управление:
//! - `WGPU_BACKEND=...` — проба пропускается, env-выбор главнее;
//! - `LUMEN_NO_BACKEND_PROBE=1` — проба выключена, работает статическая
//!   цепочка DX12 → Vulkan → GL (поведение до яруса 0);
//! - `LUMEN_FRAME_LOG=1` — подробный лог сигналов по каждому кандидату.
//!
//! Побочный эффект: на время пробы (~0.2–1 с) окно показывает кадр(ы)
//! пробного цвета — осознанная плата за автоматический выбор API.

use std::sync::Arc;
use std::time::Instant;

use winit::window::Window;

/// Пробный цвет кадра, линейные компоненты 0..1. Выбран далёким и от белого
/// (симптом BUG-275), и от чёрного (пустой захват), с попарно различными
/// каналами — перепутанный порядок каналов не даст ложного совпадения.
const PROBE_COLOR: wgpu::Color = wgpu::Color { r: 0.25, g: 0.55, b: 0.85, a: 1.0 };

/// Допуск сравнения каналов (байты). Покрывает округление формата,
/// dithering DWM и лёгкие цветовые преобразования драйвера.
const TOLERANCE: i32 = 45;

/// Ширина региона readback (64 px × 4 байта = 256 = COPY_BYTES_PER_ROW_ALIGNMENT,
/// строка не требует паддинга).
const READBACK_W: u32 = 64;
/// Высота региона readback.
const READBACK_H: u32 = 16;

/// Результат одного сигнала пробы.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Signal {
    /// Пиксели совпали с пробным цветом.
    Match,
    /// Пиксели равномерно белые — симптом BUG-275.
    White,
    /// Пиксели есть, но не пробный цвет (средний RGB в байтах).
    Other([u8; 3]),
    /// Сигнал снять не удалось (нет COPY_SRC / GDI-сбой / не Windows).
    Unavailable,
}

impl Signal {
    /// Короткая метка для лога.
    fn label(self) -> String {
        match self {
            Signal::Match => "ok".into(),
            Signal::White => "WHITE".into(),
            Signal::Other([r, g, b]) => format!("other({r},{g},{b})"),
            Signal::Unavailable => "n/a".into(),
        }
    }
}

/// Ожидаемые байты канала для линейного значения `v` с учётом sRGB-кодирования
/// формата поверхности: non-sRGB формат хранит значение как есть (`v*255`),
/// sRGB-формат кодирует линейное значение в sRGB-байт.
fn expected_byte(v: f64, srgb: bool) -> u8 {
    let encoded = if srgb {
        if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
    } else {
        v
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Классифицирует средний цвет `avg` (порядок RGB) против пробного цвета.
fn classify(avg: [u8; 3], srgb: bool) -> Signal {
    let expected = [
        expected_byte(PROBE_COLOR.r, srgb),
        expected_byte(PROBE_COLOR.g, srgb),
        expected_byte(PROBE_COLOR.b, srgb),
    ];
    let matches = avg
        .iter()
        .zip(expected.iter())
        .all(|(&a, &e)| (i32::from(a) - i32::from(e)).abs() <= TOLERANCE);
    if matches {
        Signal::Match
    } else if avg.iter().all(|&c| c >= 240) {
        Signal::White
    } else {
        Signal::Other(avg)
    }
}

/// Отчёт пробы одного кандидата.
struct CandidateReport {
    /// Имя адаптера, каким его сообщил wgpu.
    adapter: String,
    /// Сигнал texture readback.
    texture: Signal,
    /// Сигнал захвата презентации.
    present: Signal,
}

/// `true`, если проба выключена (`LUMEN_NO_BACKEND_PROBE=1`) или бэкенд
/// уже выбран явно (`WGPU_BACKEND`).
fn probe_disabled() -> bool {
    std::env::var("LUMEN_NO_BACKEND_PROBE").is_ok_and(|v| v == "1")
        || std::env::var("WGPU_BACKEND").is_ok_and(|v| !v.trim().is_empty())
}

/// Авто-проба бэкендов: возвращает первый кандидат из цепочки
/// Vulkan → GL → DX12, чей пробный кадр реально виден на экране.
///
/// `None` — проба выключена/неприменима (env-override, не Windows) или все
/// кандидаты провалились; вызывающий код использует статическую цепочку.
pub async fn pick_backend(window: &Arc<Window>) -> Option<wgpu::Backends> {
    // BUG-275 — специфика Windows (DWM/WSI); на других ОС проба не нужна,
    // а пробный цветной кадр в окне — неоправданный побочный эффект.
    if !cfg!(target_os = "windows") || probe_disabled() {
        return None;
    }
    let started = Instant::now();
    let candidates: [(wgpu::Backends, &str); 3] = [
        (wgpu::Backends::VULKAN, "Vulkan"),
        (wgpu::Backends::GL, "GL"),
        (wgpu::Backends::DX12, "DX12"),
    ];
    for (backends, name) in candidates {
        let t0 = Instant::now();
        match probe_candidate(window, backends).await {
            Ok(rep) => {
                let accepted = matches!(
                    (rep.present, rep.texture),
                    (Signal::Match, _) | (Signal::Unavailable, Signal::Match)
                );
                eprintln!(
                    "[probe] {name}: present={} texture={} adapter=\"{}\" ({} мс) — {}",
                    rep.present.label(),
                    rep.texture.label(),
                    rep.adapter,
                    t0.elapsed().as_millis(),
                    if accepted { "ПРИНЯТ" } else { "отклонён" },
                );
                if accepted {
                    eprintln!(
                        "[probe] бэкенд выбран за {} мс: {name}",
                        started.elapsed().as_millis()
                    );
                    return Some(backends);
                }
            }
            Err(e) => {
                eprintln!("[probe] {name}: недоступен ({e})");
            }
        }
    }
    eprintln!(
        "[probe] все кандидаты отклонены за {} мс — статическая цепочка",
        started.elapsed().as_millis()
    );
    None
}

/// Пробует один бэкенд: instance → surface → adapter → device → 2 кадра
/// clear-ом пробного цвета → readback + захват презентации.
async fn probe_candidate(
    window: &Arc<Window>,
    backends: wgpu::Backends,
) -> Result<CandidateReport, String> {
    // Явный выбор бэкенда — без `.with_env()`: probe_disabled() уже
    // гарантировал, что WGPU_BACKEND не задан.
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends,
        ..Default::default()
    });
    let surface = instance
        .create_surface(window.clone())
        .map_err(|e| format!("create_surface: {e}"))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| format!("request_adapter: {e}"))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("lumen-probe-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| format!("request_device: {e}"))?;

    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .iter()
        .find(|f| !f.is_srgb())
        .copied()
        .unwrap_or(caps.formats[0]);
    let can_copy = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
    let size = window.inner_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let config = wgpu::SurfaceConfiguration {
        usage: if can_copy {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        },
        format,
        width,
        height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    // Порядок байтов текселя для readback-классификации.
    let byte_order: Option<[usize; 3]> = match format {
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => Some([0, 1, 2]),
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => Some([2, 1, 0]),
        _ => None,
    };
    let do_readback =
        can_copy && byte_order.is_some() && width >= READBACK_W && height >= READBACK_H;

    // Два кадра: первый прогревает swapchain, у второго снимаем readback.
    let mut texture = Signal::Unavailable;
    for frame_idx in 0..2 {
        let frame = surface
            .get_current_texture()
            .map_err(|e| format!("get_current_texture: {e}"))?;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("probe-encoder"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("probe-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(PROBE_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        let staging = if frame_idx == 1 && do_readback {
            let staging = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("probe-readback"),
                size: u64::from(READBACK_W * 4 * READBACK_H),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &frame.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: (width - READBACK_W) / 2,
                        y: (height - READBACK_H) / 2,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(READBACK_W * 4),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: READBACK_W,
                    height: READBACK_H,
                    depth_or_array_layers: 1,
                },
            );
            Some(staging)
        } else {
            None
        };
        queue.submit([encoder.finish()]);
        frame.present();

        if let Some(staging) = staging {
            texture = read_staging(&device, &staging, byte_order, format.is_srgb())?;
        }
    }

    // DWM компонует презентованный кадр асинхронно — даём ему время и
    // перепроверяем захват до 3 раз, принимая первый Match.
    let mut present = Signal::Unavailable;
    for _ in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(120));
        match capture_present(window, format.is_srgb()) {
            Some(sig) => {
                present = sig;
                if sig == Signal::Match {
                    break;
                }
            }
            None => {
                present = Signal::Unavailable;
                break;
            }
        }
    }

    Ok(CandidateReport { adapter: adapter.get_info().name, texture, present })
}

/// Читает staging-буфер readback-а и классифицирует средний цвет региона.
fn read_staging(
    device: &wgpu::Device,
    staging: &wgpu::Buffer,
    byte_order: Option<[usize; 3]>,
    srgb: bool,
) -> Result<Signal, String> {
    let Some(order) = byte_order else {
        return Ok(Signal::Unavailable);
    };
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::Wait)
        .map_err(|e| format!("device.poll: {e}"))?;
    rx.recv()
        .map_err(|_| "readback: канал map_async оборвался".to_string())?
        .map_err(|e| format!("map_async: {e}"))?;
    let mapped = slice.get_mapped_range();
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for texel in mapped.chunks_exact(4) {
        for (i, &o) in order.iter().enumerate() {
            sum[i] += u64::from(texel[o]);
        }
        count += 1;
    }
    drop(mapped);
    staging.unmap();
    if count == 0 {
        return Ok(Signal::Unavailable);
    }
    let avg = [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
    ];
    Ok(classify(avg, srgb))
}

/// Захватывает презентованное содержимое клиентской области окна и
/// классифицирует средний цвет центрального блока 32×32.
///
/// `None` — захват недоступен (не Windows / GDI-сбой) — сигнал
/// [`Signal::Unavailable`] у вызывающего.
#[cfg(target_os = "windows")]
fn capture_present(window: &Window, srgb: bool) -> Option<Signal> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let hwnd = match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => h.hwnd.get() as *mut std::ffi::c_void,
        _ => return None,
    };
    let avg = win_capture::client_center_avg(hwnd)?;
    Some(classify(avg, srgb))
}

/// Заглушка захвата для не-Windows: сигнал недоступен.
#[cfg(not(target_os = "windows"))]
fn capture_present(_window: &Window, _srgb: bool) -> Option<Signal> {
    None
}

// ── Windows: PrintWindow-захват клиентской области ──────────────────────────

#[cfg(target_os = "windows")]
mod win_capture {
    use std::ffi::c_void;

    /// PW_CLIENTONLY | PW_RENDERFULLCONTENT — клиентская область с
    /// DWM-содержимым (GPU-swapchain), а не только GDI-поверхность.
    const PW_FLAGS: u32 = 0x1 | 0x2;
    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: u32 = 0;

    /// RECT (windef.h).
    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    /// BITMAPINFOHEADER (wingdi.h).
    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    /// BITMAPINFO (wingdi.h) — минимальная таблица цветов.
    #[repr(C)]
    struct BitmapInfo {
        bmi_header: BitmapInfoHeader,
        bmi_colors: [u32; 1],
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClientRect(h_wnd: *mut c_void, rect: *mut Rect) -> i32;
        fn GetDC(h_wnd: *mut c_void) -> *mut c_void;
        fn ReleaseDC(h_wnd: *mut c_void, h_dc: *mut c_void) -> i32;
        fn PrintWindow(h_wnd: *mut c_void, h_dc: *mut c_void, flags: u32) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(h_dc: *mut c_void) -> *mut c_void;
        fn CreateCompatibleBitmap(h_dc: *mut c_void, cx: i32, cy: i32) -> *mut c_void;
        fn SelectObject(h_dc: *mut c_void, h: *mut c_void) -> *mut c_void;
        fn GetDIBits(
            h_dc: *mut c_void,
            h_bm: *mut c_void,
            start: u32,
            c_lines: u32,
            lp_vbits: *mut c_void,
            lp_bmi: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
        fn DeleteObject(ho: *mut c_void) -> i32;
        fn DeleteDC(h_dc: *mut c_void) -> i32;
    }

    /// Средний цвет (RGB) центрального блока 32×32 клиентской области окна,
    /// снятого через `PrintWindow`. `None` при любом GDI-сбое.
    pub fn client_center_avg(hwnd: *mut c_void) -> Option<[u8; 3]> {
        // SAFETY: все вызовы Win32 GDI следуют документированным контрактам
        // (паттерн crates/shell/src/platform/screen_capture.rs): каждый handle
        // проверяется, все ресурсы освобождаются до выхода.
        unsafe {
            let mut rect = Rect { left: 0, top: 0, right: 0, bottom: 0 };
            if GetClientRect(hwnd, &raw mut rect) == 0 {
                return None;
            }
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            if w < 32 || h < 32 {
                return None;
            }

            let win_dc = GetDC(hwnd);
            if win_dc.is_null() {
                return None;
            }
            let mem_dc = CreateCompatibleDC(win_dc);
            if mem_dc.is_null() {
                ReleaseDC(hwnd, win_dc);
                return None;
            }
            let bitmap = CreateCompatibleBitmap(win_dc, w, h);
            if bitmap.is_null() {
                DeleteDC(mem_dc);
                ReleaseDC(hwnd, win_dc);
                return None;
            }
            let old_obj = SelectObject(mem_dc, bitmap);
            let printed = PrintWindow(hwnd, mem_dc, PW_FLAGS);

            let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
            let got_bits = if printed != 0 {
                let mut bmi = BitmapInfo {
                    bmi_header: BitmapInfoHeader {
                        bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                        bi_width: w,
                        // Отрицательная высота → top-down DIB (строка 0 сверху).
                        bi_height: -h,
                        bi_planes: 1,
                        bi_bit_count: 32,
                        bi_compression: BI_RGB,
                        bi_size_image: 0,
                        bi_x_pels_per_meter: 0,
                        bi_y_pels_per_meter: 0,
                        bi_clr_used: 0,
                        bi_clr_important: 0,
                    },
                    bmi_colors: [0],
                };
                GetDIBits(
                    mem_dc,
                    bitmap,
                    0,
                    h as u32,
                    pixels.as_mut_ptr().cast::<c_void>(),
                    &raw mut bmi,
                    DIB_RGB_COLORS,
                ) > 0
            } else {
                false
            };

            SelectObject(mem_dc, old_obj);
            DeleteObject(bitmap);
            DeleteDC(mem_dc);
            ReleaseDC(hwnd, win_dc);

            if !got_bits {
                return None;
            }

            // Средний цвет центрального блока 32×32; GDI отдаёт BGRA.
            let (cx, cy) = (w / 2, h / 2);
            let mut sum = [0u64; 3];
            let mut count = 0u64;
            for y in (cy - 16)..(cy + 16) {
                for x in (cx - 16)..(cx + 16) {
                    let off = ((y as usize) * (w as usize) + (x as usize)) * 4;
                    sum[0] += u64::from(pixels[off + 2]); // R
                    sum[1] += u64::from(pixels[off + 1]); // G
                    sum[2] += u64::from(pixels[off]); // B
                    count += 1;
                }
            }
            if count == 0 {
                return None;
            }
            Some([
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
            ])
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_color_matches_itself_linear() {
        let avg = [
            expected_byte(PROBE_COLOR.r, false),
            expected_byte(PROBE_COLOR.g, false),
            expected_byte(PROBE_COLOR.b, false),
        ];
        assert_eq!(classify(avg, false), Signal::Match);
    }

    #[test]
    fn probe_color_matches_itself_srgb() {
        let avg = [
            expected_byte(PROBE_COLOR.r, true),
            expected_byte(PROBE_COLOR.g, true),
            expected_byte(PROBE_COLOR.b, true),
        ];
        assert_eq!(classify(avg, true), Signal::Match);
    }

    #[test]
    fn white_is_detected_as_white() {
        assert_eq!(classify([255, 255, 255], false), Signal::White);
        assert_eq!(classify([243, 246, 250], false), Signal::White);
    }

    #[test]
    fn black_is_other_not_white() {
        assert_eq!(classify([0, 0, 0], false), Signal::Other([0, 0, 0]));
    }

    #[test]
    fn swapped_channels_do_not_match() {
        // Пробный цвет с перепутанными R и B не должен давать Match:
        // каналы выбраны попарно различными (64/140/217, дельта > TOLERANCE).
        let swapped = [
            expected_byte(PROBE_COLOR.b, false),
            expected_byte(PROBE_COLOR.g, false),
            expected_byte(PROBE_COLOR.r, false),
        ];
        assert_eq!(classify(swapped, false), Signal::Other(swapped));
    }

    #[test]
    fn expected_byte_srgb_encodes() {
        // 0.25 линейно ≈ 137 в sRGB-байтах, 64 без кодирования.
        assert_eq!(expected_byte(0.25, false), 64);
        let srgb = expected_byte(0.25, true);
        assert!((130..=143).contains(&srgb), "sRGB(0.25) ≈ 137, получили {srgb}");
    }
}
