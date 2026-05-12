//! Lumen shell — точка входа браузера.
//!
//! Режимы запуска:
//! - `lumen` — открывает пустое окно через winit.
//! - `lumen <path.html>` — парсит локальный HTML-файл через
//!   `lumen-html-parser` и печатает дерево в stdout. Окно не открывается —
//!   это режим dogfooding для парсера, до того как появится paint.

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn main() -> ExitCode {
    println!("Lumen v{} — Phase 0 prototype", env!("CARGO_PKG_VERSION"));

    let mut args = std::env::args().skip(1);
    match args.next() {
        Some(arg) => match dump_html(PathBuf::from(arg)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("Ошибка: {err}");
                ExitCode::FAILURE
            }
        },
        None => match run_window() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("Ошибка: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

fn dump_html(path: PathBuf) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::read(&path)?;
    // Phase 0: считаем, что файл в UTF-8. Encoding detection — задача §10.1.
    let source = std::str::from_utf8(&bytes)
        .map_err(|e| format!("файл {} не UTF-8: {e}", path.display()))?;

    let doc = lumen_html_parser::parse(source);
    println!("Распарсено: {} узлов из {}", doc.len(), path.display());
    println!("---");
    print!("{doc}");
    Ok(())
}

fn run_window() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = Lumen::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}

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
