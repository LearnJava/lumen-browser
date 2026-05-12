//! Lumen shell — точка входа браузера.
//!
//! Phase 0: открываем пустое окно через winit. Рендеринг (wgpu + display list
//! из lumen-paint) подключим, когда DOM/layout/paint будут готовы.

use std::error::Error;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Default)]
struct Lumen {
    window: Option<Window>,
}

impl ApplicationHandler for Lumen {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title(format!("Lumen {}", env!("CARGO_PKG_VERSION")))
            .with_inner_size(LogicalSize::new(1024.0, 720.0));

        match event_loop.create_window(attrs) {
            Ok(window) => self.window = Some(window),
            Err(err) => {
                eprintln!("Не удалось создать окно: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                // Phase 0: ещё нечего рисовать. Здесь будет вызов lumen-paint.
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("Lumen v{} — Phase 0 prototype", env!("CARGO_PKG_VERSION"));

    let event_loop = EventLoop::new()?;
    let mut app = Lumen::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}
