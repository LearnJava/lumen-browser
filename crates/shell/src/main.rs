//! Lumen shell — точка входа браузера.
//!
//! Режимы запуска:
//! - `lumen` — открыть пустое окно.
//! - `lumen <path.html>` — распарсить файл, собрать стили из `<style>`-блоков,
//!   layout, paint и нарисовать в окне через wgpu.

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_dom::{Document, NodeData, NodeId};
use lumen_paint::{DisplayList, Renderer};
use winit::application::ApplicationHandler;

/// Bundled-шрифт: статический Inter v4.1 Regular (~411 КБ),
/// SIL OFL 1.1, см. assets/fonts/OFL.txt.
const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

fn main() -> ExitCode {
    println!("Lumen v{} — Phase 0 prototype", env!("CARGO_PKG_VERSION"));

    let html_path = std::env::args().nth(1).map(PathBuf::from);
    let initial_list = match html_path {
        Some(path) => match load_page(&path) {
            Ok(list) => list,
            Err(err) => {
                eprintln!("Ошибка загрузки {}: {err}", path.display());
                return ExitCode::FAILURE;
            }
        },
        None => DisplayList::new(),
    };

    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(err) => {
            eprintln!("Не удалось создать event loop: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut app = Lumen {
        display_list: initial_list,
        window: None,
        renderer: None,
    };
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("Ошибка event loop: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn load_page(path: &PathBuf) -> Result<DisplayList, Box<dyn Error>> {
    let bytes = std::fs::read(path)?;
    // Phase 0: входной файл считаем UTF-8. Encoding detection (cp1251, KOI8-R)
    // подключим в §10.1.
    let source = std::str::from_utf8(&bytes)
        .map_err(|e| format!("не UTF-8: {e}"))?;

    let doc = lumen_html_parser::parse(source);
    let css = extract_style_blocks(&doc);
    let sheet = lumen_css_parser::parse(&css);
    let viewport = Size::new(1024.0, 720.0);
    let layout = lumen_layout::layout(&doc, &sheet, viewport);
    let list = lumen_paint::build_display_list(&layout);

    println!(
        "Распарсено: {} DOM-узлов, {} CSS-правил, {} paint-команд",
        doc.len(),
        sheet.rules.len(),
        list.len()
    );
    Ok(list)
}

fn extract_style_blocks(doc: &Document) -> String {
    let mut out = String::new();
    walk_style_blocks(doc, doc.root(), &mut out);
    out
}

fn walk_style_blocks(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Element { name, .. } = &node.data
        && name.local == "style"
    {
        for &child in &node.children {
            if let NodeData::Text(s) = &doc.get(child).data {
                out.push_str(s);
                out.push('\n');
            }
        }
        return;
    }
    for &child in &node.children {
        walk_style_blocks(doc, child, out);
    }
}

struct Lumen {
    display_list: DisplayList,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
}

impl ApplicationHandler for Lumen {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title(format!("Lumen {}", env!("CARGO_PKG_VERSION")))
            .with_inner_size(LogicalSize::new(1024.0, 720.0));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(err) => {
                eprintln!("Не удалось создать окно: {err}");
                event_loop.exit();
                return;
            }
        };

        let renderer = match Renderer::new(window.clone(), INTER_FONT.to_vec()) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Не удалось инициализировать рендер: {err}");
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(r) = self.renderer.as_mut()
                    && let Err(err) = r.render(&self.display_list)
                {
                    eprintln!("Ошибка рендера: {err:?}");
                }
            }
            _ => {}
        }
    }
}
