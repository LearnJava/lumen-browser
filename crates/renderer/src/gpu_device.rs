//! Headless `wgpu::Device` creation for `lumen-renderer` (PH3-GPUSANDBOX, срез A5).
//!
//! Same backend fallback chain as `lumen_paint::webgpu_compute::init_context`
//! (BUG-057/274/275: DX12 → Vulkan → GL on Windows, PRIMARY → GL elsewhere) and
//! the same reason for going surface-less here: `GpuInit` carries a
//! `GpuSurfaceHandle` (raw integers, see `lumen_ipc::GpuSurfaceHandle`), but
//! reconstructing a `raw-window-handle` from those integers and calling
//! `Instance::create_surface_unsafe` is deliberately deferred to a later
//! срез — it is unsafe, platform-specific, and untestable without a real
//! window on the other end of the IPC channel. This срез only proves the
//! renderer process can stand up a working GPU device on its own; `GpuRender`
//! still acknowledges without submitting (no surface to present to yet).

use std::future::Future;

/// Live GPU handles the renderer process holds after a successful `GpuInit`.
///
/// `device`/`queue` are not read anywhere yet — same
/// `#[allow(dead_code)]` rationale as `renderer_process.rs`'s
/// `RendererProcessHandle`: nothing submits a frame with them until the
/// surface/present срез lands, but that срез needs a real `Device` already
/// held here, not created from scratch again.
#[allow(dead_code)]
pub struct GpuDevice {
    /// The adapter the device was created from — used for `get_info()`
    /// (backend/name) today, will back a future adapter-loss recovery path.
    pub adapter: wgpu::Adapter,
    /// Logical GPU device. Unused until the surface/present срез submits
    /// real command buffers.
    pub device: wgpu::Device,
    /// Command queue paired with `device`. Unused until the surface/present
    /// срез lands.
    pub queue: wgpu::Queue,
}

/// Tries each backend in `lumen-paint`'s fallback order and returns the first
/// one that yields a working adapter + device. `None` means no GPU/driver is
/// reachable from this process — the caller replies `GpuError`, it does not
/// panic (the renderer process must survive a machine with no GPU, same as
/// the in-process `backend_probe`/`webgpu_compute` paths do today).
pub fn init() -> Option<GpuDevice> {
    let backend_prefs: &[wgpu::Backends] = if cfg!(target_os = "windows") {
        &[wgpu::Backends::DX12, wgpu::Backends::VULKAN, wgpu::Backends::GL]
    } else {
        &[wgpu::Backends::PRIMARY, wgpu::Backends::GL]
    };

    backend_prefs.iter().find_map(|&backends| {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends, ..Default::default() });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            // No surface yet (see module docs) — compatibility with a future
            // surface is re-checked once GpuRender/resize wiring lands.
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok()?;

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("lumen-renderer-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .ok()?;

        // Uncaptured device errors default to a wgpu panic; the renderer
        // process must instead report `GpuError` to the shell and keep
        // running (crash isolation is the entire point of this process
        // boundary — see docs/tasks/ph3-gpu-process-sandbox.md Goal).
        device.on_uncaptured_error(Box::new(|e| {
            eprintln!("lumen-renderer: uncaptured wgpu device error: {e}");
        }));

        Some(GpuDevice { adapter, device, queue })
    })
}

/// Local `block_on` without tokio/pollster — same trick as
/// `lumen_paint::webgpu_compute::block_on`: two or three async calls during
/// device init, normally `Ready` immediately.
fn block_on<F: Future>(future: F) -> F::Output {
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

    // GPU-зависимый тест: на headless CI без адаптера просто не паникует и
    // возвращает `None` — тот же допуск, что у `webgpu_compute`'s
    // `adapter_info_present_when_gpu_available`.
    #[test]
    fn init_does_not_panic_and_reports_backend_when_available() {
        match init() {
            Some(gpu) => {
                let info = gpu.adapter.get_info();
                assert!(!info.name.is_empty(), "adapter name must be non-empty when a device is created");
            }
            None => eprintln!("skip: no GPU adapter available"),
        }
    }
}
