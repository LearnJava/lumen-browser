//! Окно winit, чьи сырые дескрипторы сняты один раз на главном потоке.
//!
//! winit 0.30 на Windows отдаёт `window_handle()` только потоку, создавшему
//! окно, — с любого другого приходит `HandleError::Unavailable`. С ADR-032
//! (поток браузера) окно создаётся на главном потоке, а рендерер строится на
//! потоке браузера, поэтому дескрипторы снимает главный поток при создании
//! окна, а остальные потоки читают сохранённые.

use winit::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use winit::window::Window;

/// `Window` + сохранённые сырые дескрипторы; через `Deref` — обычное окно.
pub struct SurfaceWindow {
    window: Window,
    window_handle: RawWindowHandle,
    display_handle: RawDisplayHandle,
}

// SAFETY: сырые дескрипторы — это идентификаторы окна/дисплея ОС (HWND,
// HINSTANCE-эквиваленты), а не доступ к данным. Их создание на других потоках
// допускают wgpu/glutin для Win32/X11/Wayland; для macOS поведение вне
// главного потока не проверено (ADR-032, срез 2).
unsafe impl Send for SurfaceWindow {}
// SAFETY: см. выше; `Window` сам `Send + Sync`.
unsafe impl Sync for SurfaceWindow {}

impl SurfaceWindow {
    /// Вызывать на потоке, создавшем окно (на главном потоке).
    ///
    /// # Errors
    /// Платформа не отдаёт дескрипторы окна.
    pub fn new(window: Window) -> Result<Self, HandleError> {
        let window_handle = window.window_handle()?.as_raw();
        let display_handle = window.display_handle()?.as_raw();
        Ok(Self { window, window_handle, display_handle })
    }
}

impl std::ops::Deref for SurfaceWindow {
    type Target = Window;
    fn deref(&self) -> &Window {
        &self.window
    }
}

impl HasWindowHandle for SurfaceWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: дескриптор снят с живого окна, которым владеет `self`.
        Ok(unsafe { WindowHandle::borrow_raw(self.window_handle) })
    }
}

impl HasDisplayHandle for SurfaceWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: см. `window_handle`.
        Ok(unsafe { DisplayHandle::borrow_raw(self.display_handle) })
    }
}
