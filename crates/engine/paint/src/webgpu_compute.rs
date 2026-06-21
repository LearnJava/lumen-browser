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
//! **Что ещё stub (Stage 2+):** буферы (`createBuffer`/`writeBuffer`/`mapAsync`),
//! compute-пайплайны и `dispatchWorkgroups`, render-пайплайны и present в canvas.
//! Эти операции по-прежнему обслуживает in-memory JS-шим; этот модуль их не трогает.
//!
//! **Доступность.** GPU-устройство создаётся лениво один раз (`OnceLock`). Если адаптер
//! недоступен (headless CI без GPU, нет драйвера), [`adapter_info`] и [`validate_wgsl`]
//! отдают `None`/`Err`, и JS-шим продолжает работать в режиме stub — никакой регрессии.

use std::sync::OnceLock;

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
    /// Очередь команд устройства (понадобится для submit в Stage 2).
    #[allow(dead_code)]
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
}
