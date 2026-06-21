//! Реальный WebGPU-бэкенд, Stage 1: получение GPU-устройства и валидация WGSL.
//!
//! До этого `navigator.gpu` в `lumen-js` был чистым JS-шимом (Phase 0): `requestAdapter`
//! всегда возвращал фейковый stub-адаптер, `createShaderModule` ничего не проверял,
//! `getCompilationInfo()` всегда отдавал пустой список диагностик. Этот модуль даёт
//! `lumen-js` доступ к **настоящему** wgpu-устройству (тот же бэкенд, что и у рендерера —
//! DX12 на Windows, PRIMARY иначе), но без surface (headless): WebGPU compute/validation
//! не требует окна.
//!
//! **Что реально в Stage 1:**
//! - `adapter_info()` — настоящие vendor/architecture/device/description адаптера GPU
//!   (через `wgpu::Adapter::get_info`), а не строка `"Lumen WebGPU Phase 0 stub"`.
//! - `validate_wgsl()` — настоящая трансляция и валидация WGSL через `naga`/wgpu:
//!   синтаксические и типовые ошибки шейдера возвращаются как диагностика с текстом,
//!   как в реальном браузере, вместо всегда-пустого `getCompilationInfo()`.
//!
//! **Что реально в Stage 2 (под-этап 1, буферы):**
//! - [`buffer_create`]/[`buffer_write`]/[`buffer_read`]/[`buffer_destroy`] — настоящие
//!   `wgpu::Buffer` в GPU-памяти, адресуемые по числовому хэндлу из JS-шима.
//! - [`submit`] — исполняет записанные command-encoder операции (пока только
//!   `copyBufferToBuffer`) в одном `wgpu::CommandEncoder` + `queue.submit`, как реальный
//!   браузер батчит работу на `GPUQueue.submit`.
//! - Полный round-trip: write → copy(STORAGE/COPY_SRC → MAP_READ) → map → read возвращает
//!   данные, реально прошедшие через GPU-память, а не JS-`ArrayBuffer`.
//!
//! **Что ещё stub (Stage 2+):** compute-пайплайны и `dispatchWorkgroups`,
//! render-пайплайны и present в canvas. Эти операции по-прежнему обслуживает in-memory
//! JS-шим; буферный реестр заложен как фундамент для следующего под-этапа (compute).
//!
//! **Доступность.** GPU-устройство создаётся лениво один раз (`OnceLock`). Если адаптер
//! недоступен (headless CI без GPU, нет драйвера), [`adapter_info`] и [`validate_wgsl`]
//! отдают `None`/`Err`, и JS-шим продолжает работать в режиме stub — никакой регрессии.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Информация о GPU-адаптере для отдачи в JS (`GPUAdapter.info`).
///
/// Поля соответствуют W3C `GPUAdapterInfo`: значения берутся из настоящего
/// [`wgpu::AdapterInfo`], а не из захардкоженного stub.
#[derive(Debug, Clone)]
pub struct AdapterInfo {
    /// Производитель адаптера (например `"nvidia"`, `"amd"`, `"intel"`, `"microsoft"`).
    pub vendor: String,
    /// Архитектура GPU; wgpu не всегда её знает — тогда пустая строка (как в браузерах).
    pub architecture: String,
    /// Название устройства/модель GPU из драйвера.
    pub device: String,
    /// Человекочитаемое описание: имя адаптера + backend + driver.
    pub description: String,
}

/// Лениво инициализируемый реальный GPU-контекст для WebGPU.
///
/// Держит живые `Device`/`Queue` (понадобятся буферам и пайплайнам в Stage 2) и
/// снимок информации об адаптере. Создаётся ровно один раз на процесс.
struct ComputeContext {
    /// Логическое GPU-устройство (источник шейдер-модулей, буферов, пайплайнов).
    device: wgpu::Device,
    /// Очередь команд устройства (submit копий, запись буферов).
    queue: wgpu::Queue,
    /// Снимок информации об адаптере для `GPUAdapter.info`.
    info: AdapterInfo,
}

/// Глобальный кэш GPU-контекста: `None` — адаптер недоступен (нет GPU/драйвера).
static CONTEXT: OnceLock<Option<ComputeContext>> = OnceLock::new();

/// Создаёт headless wgpu-устройство тем же выбором бэкенда, что и рендерер (BUG-057):
/// DX12 на Windows, PRIMARY иначе. Возвращает `None`, если адаптер недоступен.
fn init_context() -> Option<ComputeContext> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: if cfg!(target_os = "windows") {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::PRIMARY
        },
        ..Default::default()
    });

    // Surface не нужен — compute/validation работают без окна.
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;

    let raw = adapter.get_info();
    let info = AdapterInfo {
        vendor: vendor_string(raw.vendor),
        // wgpu не выдаёт архитектуру GPU отдельным полем; браузеры тоже часто отдают "".
        architecture: String::new(),
        device: raw.name.clone(),
        description: format!("{} ({:?}, {})", raw.name, raw.backend, raw.driver),
    };

    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("lumen-webgpu-compute-device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;

    // Заглушаем wgpu-логгер ошибок устройства: невалидный WGSL мы ловим через error scope,
    // и без своего обработчика wgpu по умолчанию паникует на uncaptured-ошибке.
    device.on_uncaptured_error(Box::new(|e| {
        // Тихо проглатываем — валидацию делаем явным error scope в validate_wgsl.
        let _ = e;
    }));

    Some(ComputeContext {
        device,
        queue,
        info,
    })
}

/// Возвращает ссылку на ленивый GPU-контекст (инициализирует при первом обращении).
fn context() -> Option<&'static ComputeContext> {
    CONTEXT.get_or_init(init_context).as_ref()
}

/// Доступен ли реальный GPU-бэкенд (есть адаптер и устройство).
///
/// `false` — `lumen-js` остаётся на JS-шиме (in-memory stub).
pub fn is_available() -> bool {
    context().is_some()
}

/// Информация о реальном GPU-адаптере или `None`, если GPU недоступен.
pub fn adapter_info() -> Option<AdapterInfo> {
    context().map(|c| c.info.clone())
}

/// Валидирует исходник WGSL на настоящем GPU-устройстве (трансляция + типовая проверка).
///
/// Возвращает:
/// - `None` — шейдер валиден (диагностики нет), либо GPU недоступен (fallback: не мешаем);
/// - `Some(сообщение)` — текст ошибки компиляции WGSL для `getCompilationInfo()`.
///
/// Использует wgpu error scope: модуль создаётся всегда, а ошибка валидации
/// поднимается асинхронно и перехватывается здесь, не доходя до uncaptured-обработчика.
pub fn validate_wgsl(source: &str) -> Option<String> {
    let ctx = context()?;
    // wgpu error scopes form a single per-device stack: concurrent validations would
    // interleave push/create/pop and catch each other's errors. Serialize so each
    // push → create_shader_module → pop is atomic relative to other validations.
    static VALIDATE_LOCK: Mutex<()> = Mutex::new(());
    let _guard = VALIDATE_LOCK.lock().ok()?;
    ctx.device
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let _module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lumen-webgpu-validate"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let err = block_on(ctx.device.pop_error_scope());
    err.map(|e| e.to_string())
}

// ── Реестр GPU-буферов (Stage 2, под-этап 1) ────────────────────────────────
//
// JS-шим `navigator.gpu` держит непрозрачные хэндлы (`u64`), а сами `wgpu::Buffer`
// живут здесь. Это позволяет `writeBuffer`/`copyBufferToBuffer`/`mapAsync` работать с
// настоящей GPU-памятью, оставаясь за границей `lumen-js` (тот не зависит от wgpu-типов).

/// Запись реестра: живой GPU-буфер. Размер берётся из `wgpu::Buffer::size()`
/// (выровненный вверх), спец-значение `GPUBuffer.size` хранит JS-шим.
struct BufferEntry {
    /// Настоящий GPU-буфер (источник/приёмник копий, цель `writeBuffer`).
    buffer: wgpu::Buffer,
}

/// Глобальный реестр буферов, ключ — хэндл, выданный [`buffer_create`].
static BUFFERS: OnceLock<Mutex<HashMap<u64, BufferEntry>>> = OnceLock::new();

/// Монотонный счётчик хэндлов буферов (0 зарезервирован под «невалидный»).
static NEXT_BUFFER_ID: AtomicU64 = AtomicU64::new(1);

/// Доступ к реестру буферов (создаётся при первом обращении).
fn buffers() -> &'static Mutex<HashMap<u64, BufferEntry>> {
    BUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Переводит биты `GPUBufferUsage` (W3C) в [`wgpu::BufferUsages`].
///
/// Значения битов совпадают с константами `GPUBufferUsage` в JS-шиме.
fn buffer_usages(bits: u32) -> wgpu::BufferUsages {
    let mut u = wgpu::BufferUsages::empty();
    if bits & 0x0001 != 0 {
        u |= wgpu::BufferUsages::MAP_READ;
    }
    if bits & 0x0002 != 0 {
        u |= wgpu::BufferUsages::MAP_WRITE;
    }
    if bits & 0x0004 != 0 {
        u |= wgpu::BufferUsages::COPY_SRC;
    }
    if bits & 0x0008 != 0 {
        u |= wgpu::BufferUsages::COPY_DST;
    }
    if bits & 0x0010 != 0 {
        u |= wgpu::BufferUsages::INDEX;
    }
    if bits & 0x0020 != 0 {
        u |= wgpu::BufferUsages::VERTEX;
    }
    if bits & 0x0040 != 0 {
        u |= wgpu::BufferUsages::UNIFORM;
    }
    if bits & 0x0080 != 0 {
        u |= wgpu::BufferUsages::STORAGE;
    }
    if bits & 0x0100 != 0 {
        u |= wgpu::BufferUsages::INDIRECT;
    }
    if bits & 0x0200 != 0 {
        u |= wgpu::BufferUsages::QUERY_RESOLVE;
    }
    u
}

/// Создаёт настоящий `wgpu::Buffer` и регистрирует его.
///
/// `usage_bits` — биты `GPUBufferUsage` из JS. Возвращает непрозрачный хэндл буфера,
/// либо `None`, если GPU недоступен. Размер выравнивается вверх до кратного 4 (требование
/// wgpu к `COPY`/`map`-операциям) — спец `GPUBuffer.size` в JS-шиме остаётся как запрошено.
pub fn buffer_create(size: u64, usage_bits: u32, mapped_at_creation: bool) -> Option<u64> {
    let ctx = context()?;
    let aligned = size.max(4).div_ceil(4) * 4;
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumen-webgpu-buffer"),
        size: aligned,
        usage: buffer_usages(usage_bits),
        mapped_at_creation,
    });
    let id = NEXT_BUFFER_ID.fetch_add(1, Ordering::Relaxed);
    buffers().lock().ok()?.insert(id, BufferEntry { buffer });
    Some(id)
}

/// Записывает байты в буфер по смещению через `queue.write_buffer`.
///
/// Возвращает `false`, если хэндл неизвестен, запись выходит за пределы буфера или GPU
/// недоступен. После записи делает `queue.submit([])`, чтобы стейджинг гарантированно
/// долетел до GPU-памяти до последующего чтения.
pub fn buffer_write(id: u64, offset: u64, data: &[u8]) -> bool {
    let Some(ctx) = context() else { return false };
    let map = buffers();
    let guard = match map.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let Some(entry) = guard.get(&id) else {
        return false;
    };
    if offset + data.len() as u64 > entry.buffer.size() {
        return false;
    }
    ctx.queue.write_buffer(&entry.buffer, offset, data);
    ctx.queue.submit(std::iter::empty());
    true
}

/// Читает байты из буфера (буфер должен иметь usage `MAP_READ`).
///
/// Синхронно мапит диапазон, копирует данные, размапливает. Возвращает `None`, если
/// хэндл неизвестен, диапазон вне буфера, буфер не `MAP_READ`, или GPU недоступен.
pub fn buffer_read(id: u64, offset: u64, size: u64) -> Option<Vec<u8>> {
    let ctx = context()?;
    let map = buffers();
    let guard = map.lock().ok()?;
    let entry = guard.get(&id)?;
    if offset + size > entry.buffer.size() {
        return None;
    }
    let slice = entry.buffer.slice(offset..offset + size);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    ctx.device.poll(wgpu::PollType::Wait).ok()?;
    rx.recv().ok()?.ok()?;
    let bytes = slice.get_mapped_range().to_vec();
    entry.buffer.unmap();
    Some(bytes)
}

/// Удаляет буфер из реестра (освобождает GPU-память при дропе).
pub fn buffer_destroy(id: u64) {
    if let Ok(mut guard) = buffers().lock() {
        guard.remove(&id);
    }
}

/// Одна записанная операция command-encoder для исполнения на `queue.submit`.
///
/// Пока поддерживается только копирование буфер→буфер; следующий под-этап добавит
/// compute-pass (`dispatchWorkgroups`).
#[derive(Debug, Clone, Copy)]
pub enum GpuOp {
    /// `copyBufferToBuffer(src, src_offset, dst, dst_offset, size)`.
    CopyBufferToBuffer {
        /// Хэндл буфера-источника.
        src: u64,
        /// Смещение в источнике (байты).
        src_offset: u64,
        /// Хэндл буфера-приёмника.
        dst: u64,
        /// Смещение в приёмнике (байты).
        dst_offset: u64,
        /// Сколько байт копировать.
        size: u64,
    },
}

/// Исполняет набор операций в одном `CommandEncoder` и сабмитит на очередь.
///
/// Соответствует `GPUQueue.submit([commandBuffer])`: операции, записанные в
/// command-encoder в JS, прилетают сюда и исполняются батчем. Возвращает `false`, если
/// GPU недоступен или какой-то хэндл/диапазон невалиден (тогда ничего не сабмитится).
pub fn submit(ops: &[GpuOp]) -> bool {
    let Some(ctx) = context() else { return false };
    let map = buffers();
    let guard = match map.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lumen-webgpu-submit"),
        });
    for op in ops {
        match *op {
            GpuOp::CopyBufferToBuffer {
                src,
                src_offset,
                dst,
                dst_offset,
                size,
            } => {
                let (Some(s), Some(d)) = (guard.get(&src), guard.get(&dst)) else {
                    return false;
                };
                if src_offset + size > s.buffer.size() || dst_offset + size > d.buffer.size() {
                    return false;
                }
                encoder.copy_buffer_to_buffer(&s.buffer, src_offset, &d.buffer, dst_offset, size);
            }
        }
    }
    ctx.queue.submit(std::iter::once(encoder.finish()));
    true
}

/// Переводит числовой PCI vendor id в строку, как это делают браузеры в `GPUAdapterInfo.vendor`.
fn vendor_string(vendor: u32) -> String {
    match vendor {
        0x1002 => "amd".to_string(),
        0x10de => "nvidia".to_string(),
        0x8086 => "intel".to_string(),
        0x13b5 => "arm".to_string(),
        0x5143 => "qualcomm".to_string(),
        0x1414 => "microsoft".to_string(),
        0x106b => "apple".to_string(),
        0 => String::new(),
        other => format!("0x{other:04x}"),
    }
}

/// Локальный `block_on` без tokio/pollster: WebGPU compute-инициализация — два-три
/// async-вызова, обычно сразу `Ready`. Тот же приём, что в `renderer::block_on`.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::pin::pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;

    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_string_known_ids() {
        assert_eq!(vendor_string(0x10de), "nvidia");
        assert_eq!(vendor_string(0x1002), "amd");
        assert_eq!(vendor_string(0x8086), "intel");
        assert_eq!(vendor_string(0), "");
        assert_eq!(vendor_string(0x1234), "0x1234");
    }

    // GPU-зависимые тесты: пропускаются, если адаптер недоступен (headless CI без GPU).
    // На машинах с GPU проверяют, что путь реальный, а не stub.

    #[test]
    fn adapter_info_present_when_gpu_available() {
        if !is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        let info = adapter_info().expect("adapter info when available");
        // Реальный адаптер всегда сообщает имя устройства.
        assert!(!info.device.is_empty(), "device name must be non-empty");
        assert!(
            !info.description.contains("Phase 0 stub"),
            "must not be the Phase 0 stub string"
        );
    }

    #[test]
    fn valid_wgsl_passes() {
        if !is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        let src = r#"
            @group(0) @binding(0) var<storage, read_write> data: array<u32>;
            @compute @workgroup_size(1)
            fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                data[id.x] = data[id.x] * 2u;
            }
        "#;
        assert_eq!(validate_wgsl(src), None, "valid WGSL must produce no error");
    }

    #[test]
    fn invalid_wgsl_reports_error() {
        if !is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        // Синтаксический мусор — должен дать непустую диагностику.
        let err = validate_wgsl("this is not valid wgsl @@@");
        assert!(err.is_some(), "invalid WGSL must report a compilation error");
    }

    #[test]
    fn buffer_usages_maps_bits() {
        // MAP_READ | COPY_DST
        let u = buffer_usages(0x0001 | 0x0008);
        assert!(u.contains(wgpu::BufferUsages::MAP_READ));
        assert!(u.contains(wgpu::BufferUsages::COPY_DST));
        assert!(!u.contains(wgpu::BufferUsages::STORAGE));
        // STORAGE | COPY_SRC | VERTEX
        let u = buffer_usages(0x0080 | 0x0004 | 0x0020);
        assert!(u.contains(wgpu::BufferUsages::STORAGE));
        assert!(u.contains(wgpu::BufferUsages::COPY_SRC));
        assert!(u.contains(wgpu::BufferUsages::VERTEX));
    }

    #[test]
    fn buffer_write_read_round_trip() {
        if !is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        // MAP_READ | COPY_DST позволяет и write_buffer, и последующий map-read.
        let id = buffer_create(16, 0x0001 | 0x0008, false).expect("create buffer");
        let payload: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        assert!(buffer_write(id, 0, &payload), "write must succeed");
        let got = buffer_read(id, 0, 16).expect("read must succeed");
        assert_eq!(got, payload, "data must round-trip through GPU memory");
        buffer_destroy(id);
        // После destroy чтение по тому же хэндлу невозможно.
        assert!(buffer_read(id, 0, 16).is_none(), "read after destroy must fail");
    }

    #[test]
    fn copy_buffer_to_buffer_round_trip() {
        if !is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        // src: данные пишутся, копируются на GPU в dst, dst читается.
        let src = buffer_create(8, 0x0004 | 0x0008, false).expect("create src"); // COPY_SRC|COPY_DST
        let dst = buffer_create(8, 0x0001 | 0x0008, false).expect("create dst"); // MAP_READ|COPY_DST
        let payload: [u8; 8] = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x11, 0x22, 0x33];
        assert!(buffer_write(src, 0, &payload));
        assert!(
            submit(&[GpuOp::CopyBufferToBuffer {
                src,
                src_offset: 0,
                dst,
                dst_offset: 0,
                size: 8,
            }]),
            "submit copy must succeed"
        );
        let got = buffer_read(dst, 0, 8).expect("read dst");
        assert_eq!(got, payload, "copy must move bytes through GPU");
        buffer_destroy(src);
        buffer_destroy(dst);
    }

    #[test]
    fn write_out_of_bounds_rejected() {
        if !is_available() {
            eprintln!("skip: no GPU adapter available");
            return;
        }
        let id = buffer_create(4, 0x0008, false).expect("create");
        // 4 байта по смещению 4 — выходит за пределы 4-байтного буфера.
        assert!(!buffer_write(id, 4, &[1, 2, 3, 4]), "out-of-bounds write must be rejected");
        buffer_destroy(id);
    }

    #[test]
    fn unknown_handle_is_safe() {
        // Операции по несуществующему хэндлу не паникуют и сообщают об ошибке.
        assert!(!buffer_write(999_999, 0, &[0]));
        assert!(buffer_read(999_999, 0, 4).is_none());
        buffer_destroy(999_999);
    }
}
