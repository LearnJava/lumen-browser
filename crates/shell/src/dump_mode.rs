//! Headless render entry points: `--dump-*`, `--screenshot`, `--print-to-pdf`
//! and `--trace-nav`.
//!
//! Every mode here shares one shape — load the [`PageSource`], lay it out, and
//! render on the CPU without ever creating a window, wgpu surface or winit
//! event loop — which is what makes their output reproducible in CI. Moved out
//! of `main.rs` by the SPLIT track (batch SH-3a); behaviour and signatures are
//! unchanged.

use crate::*;

pub(crate) fn run_dump_mode(
    source: &PageSource,
    kind: DumpKind,
    event_sink: Arc<dyn EventSink>,
    viewport_override: Option<(f32, f32)>,
) -> ExitCode {
    match run_dump(source, kind, event_sink, viewport_override) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("РћС€РёР±РєР° dump {}: {err}", source.describe());
            ExitCode::FAILURE
        }
    }
}

/// Р—Р°РїСѓСЃС‚РёС‚СЊ `--print-to-pdf`: layout в†’ paginate в†’ render в†’ PDF в†’ С„Р°Р№Р».
pub(crate) fn run_print_to_pdf(
    source: &PageSource,
    output: &std::path::Path,
    event_sink: Arc<dyn EventSink>,
) -> ExitCode {
    match do_print_to_pdf(source, output, event_sink) {
        Ok(page_count) => {
            eprintln!("PDF СЃРѕС…СЂР°РЅС‘РЅ: {} ({page_count} СЃС‚СЂ.)", output.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("РћС€РёР±РєР° --print-to-pdf {}: {err}", source.describe());
            ExitCode::FAILURE
        }
    }
}

/// A4 @ 96 DPI: 210 mm Г— 297 mm в†’ 794 Г— 1123 px.
pub(crate) const PDF_PAGE_W: u32 = 794;
pub(crate) const PDF_PAGE_H: u32 = 1123;

/// РЁРёСЂРёРЅР° viewport РґР»СЏ headless `--screenshot` (РєР°Рє РІ dump-СЂРµР¶РёРјР°С…).
pub(crate) const SCREENSHOT_VP_W: f32 = 1024.0;
/// РњРёРЅРёРјР°Р»СЊРЅР°СЏ РІС‹СЃРѕС‚Р° СЃРЅРёРјРєР° (РѕРґРёРЅ СЌРєСЂР°РЅ) РґР»СЏ РєРѕСЂРѕС‚РєРёС… СЃС‚СЂР°РЅРёС†.
pub(crate) const SCREENSHOT_MIN_H: f32 = 720.0;
/// Р’РµСЂС…РЅРёР№ РїСЂРµРґРµР» РІС‹СЃРѕС‚С‹ СЃРЅРёРјРєР° вЂ” Р·Р°С‰РёС‚Р° РѕС‚ РіРёРіР°РЅС‚СЃРєРёС… Р°Р»Р»РѕРєР°С†РёР№ РЅР° РѕС‡РµРЅСЊ
/// РґР»РёРЅРЅС‹С… СЃС‚СЂР°РЅРёС†Р°С… (1024 Г— 32768 Г— 4 в‰€ 134 РњР‘ вЂ” РїРѕС‚РѕР»РѕРє).
pub(crate) const SCREENSHOT_MAX_H: f32 = 32768.0;

/// Р—Р°РїСѓСЃС‚РёС‚СЊ `--screenshot`: load в†’ layout в†’ CPU-СЂР°СЃС‚РµСЂРёР·Р°С†РёСЏ в†’ PNG в†’ С„Р°Р№Р».
pub(crate) fn run_screenshot(
    source: &PageSource,
    output: &std::path::Path,
    event_sink: Arc<dyn EventSink>,
    viewport_override: Option<(f32, f32)>,
) -> ExitCode {
    match do_screenshot(source, output, event_sink, viewport_override) {
        Ok((w, h)) => {
            eprintln!("РЎРЅРёРјРѕРє СЃРѕС…СЂР°РЅС‘РЅ: {} ({w}Г—{h})", output.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("РћС€РёР±РєР° --screenshot {}: {err}", source.describe());
            ExitCode::FAILURE
        }
    }
}

/// Р—Р°РїСѓСЃС‚РёС‚СЊ `--trace-nav <out.json> <url>` (PERF-1): РїСЂРѕРіРЅР°С‚СЊ РѕРґРЅСѓ РЅР°РІРёРіР°С†РёСЋ
/// С‡РµСЂРµР· С‚РѕС‚ Р¶Рµ headless CPU-РїСѓС‚СЊ, С‡С‚Рѕ `--screenshot`, РЅРѕ СЃ РІРєР»СЋС‡С‘РЅРЅС‹Рј
/// С‚СЂРµР№СЃРµСЂРѕРј [`lumen_core::trace`], Рё СЃРѕС…СЂР°РЅРёС‚СЊ СЃРѕР±СЂР°РЅРЅС‹Р№ С‚Р°Р№РјР»Р°Р№РЅ РєР°Рє
/// Chrome-trace JSON (РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ РІ Perfetto / `chrome://tracing` /
/// `edge://tracing`). PNG СЂР°СЃС‚РµСЂРёР·СѓРµС‚СЃСЏ РєР°Рє РїРѕР±РѕС‡РЅС‹Р№ СЌС„С„РµРєС‚ РїСѓС‚Рё Рё
/// РѕС‚Р±СЂР°СЃС‹РІР°РµС‚СЃСЏ вЂ” РІР°Р¶РµРЅ С‚РѕР»СЊРєРѕ С‚Р°Р№РјР»Р°Р№РЅ С„Р°Р· (fetch-document в†’ parse-html в†’
/// run-scripts в†’ fetch-*/layout в†’ paint в†’ first-paint) Рё РїРѕ-СЂРµСЃСѓСЂСЃРЅС‹Рµ fetch-СЃРїР°РЅС‹.
pub(crate) fn run_trace_nav(
    source: &PageSource,
    output: &std::path::Path,
    event_sink: Arc<dyn EventSink>,
) -> ExitCode {
    // PERF-12: the tracer is already recording вЂ” `startup_trace::Startup::begin`
    // switched it on at the top of `run_cli`, anchored at process creation, so
    // that fixed startup lands on the same timeline. Re-enabling here would
    // reset the origin and drop every span taken before dispatch.
    let render = {
        // РљРѕСЂРЅРµРІРѕР№ СЃРїР°РЅ РІСЃРµР№ РЅР°РІРёРіР°С†РёРё; Р·Р°РєСЂС‹РІР°РµС‚СЃСЏ РґРѕ `finish`, РїРѕСЌС‚РѕРјСѓ РїРѕРїР°РґР°РµС‚
        // РІ С‚Р°Р№РјР»Р°Р№РЅ РєР°Рє РѕС…РІР°С‚С‹РІР°СЋС‰Р°СЏ РїРѕР»РѕСЃР°.
        let _nav = lumen_core::trace::span("navigation", "nav");
        render_source_to_png(source, event_sink, None)
    };
    let json = match lumen_core::trace::finish() {
        Some(j) => j,
        None => {
            eprintln!("РћС€РёР±РєР° --trace-nav: С‚СЂРµР№СЃРµСЂ РЅРµ СЃРѕР±СЂР°Р» РґР°РЅРЅС‹С…");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = render {
        // Р РµРЅРґРµСЂ РјРѕРі СѓРїР°СЃС‚СЊ (Р±РёС‚С‹Р№ URL, СЃРµС‚СЊ) вЂ” РЅРѕ С‡Р°СЃС‚РёС‡РЅС‹Р№ С‚Р°Р№РјР»Р°Р№РЅ РІСЃС‘ СЂР°РІРЅРѕ
        // РїРѕР»РµР·РµРЅ, РїРѕСЌС‚РѕРјСѓ РїРёС€РµРј РµРіРѕ Рё СЃРѕРѕР±С‰Р°РµРј РѕР± РѕС€РёР±РєРµ.
        eprintln!("РџСЂРµРґСѓРїСЂРµР¶РґРµРЅРёРµ --trace-nav: СЂРµРЅРґРµСЂ Р·Р°РІРµСЂС€РёР»СЃСЏ СЃ РѕС€РёР±РєРѕР№: {err}");
    }
    match std::fs::write(output, json) {
        Ok(()) => {
            eprintln!("РўСЂРµР№СЃ РЅР°РІРёРіР°С†РёРё СЃРѕС…СЂР°РЅС‘РЅ: {}", output.display());
            eprintln!("  РћС‚РєСЂС‹С‚СЊ РІ Perfetto (ui.perfetto.dev) РёР»Рё chrome://tracing");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("РћС€РёР±РєР° Р·Р°РїРёСЃРё С‚СЂРµР№СЃР° {}: {err}", output.display());
            ExitCode::FAILURE
        }
    }
}

/// Headless CPU-СЃРЅРёРјРѕРє СЃС‚СЂР°РЅРёС†С‹ С†РµР»РёРєРѕРј (РІРєР»СЋС‡Р°СЏ РєРѕРЅС‚РµРЅС‚ РЅРёР¶Рµ РїРµСЂРІРѕРіРѕ СЌРєСЂР°РЅР°).
///
/// РСЃРїРѕР»СЊР·СѓРµС‚ С‚РѕС‚ Р¶Рµ РїРѕР»РЅС‹Р№ pipeline, С‡С‚Рѕ `--dump-layout`/`--print-to-pdf`
/// (`parse_and_layout` в†’ РІРЅРµС€РЅРёР№ CSS, РєР°СЂС‚РёРЅРєРё, `paint_ordered`), Р·Р°С‚РµРј
/// СЂР°СЃС‚РµСЂРёР·СѓРµС‚ display-list РґРµС‚РµСЂРјРёРЅРёСЂРѕРІР°РЅРЅС‹Рј CPU-Р±СЌРєРµРЅРґРѕРј
/// (`Renderer::render_to_image_cpu`, feature `cpu-render`) Рё РєРѕРґРёСЂСѓРµС‚ PNG.
/// Р’ РѕС‚Р»РёС‡РёРµ РѕС‚ GPU-РїСѓС‚Рё РѕРєРЅР° вЂ” РЅРµ РЅСѓР¶РµРЅ wgpu/winit, СЂРµР·СѓР»СЊС‚Р°С‚ РїРёРєСЃРµР»СЊРЅРѕ
/// РІРѕСЃРїСЂРѕРёР·РІРѕРґРёРј Рё СЂР°Р±РѕС‚Р°РµС‚ РЅР° Р»СЋР±РѕР№ РћРЎ/РІ CI.
///
/// Р’С‹СЃРѕС‚Р° СЃРЅРёРјРєР° = РІС‹СЃРѕС‚Р° layout-РєРѕСЂРЅСЏ, Р·Р°Р¶Р°С‚Р°СЏ РІ
/// `[SCREENSHOT_MIN_H, SCREENSHOT_MAX_H]`, С€РёСЂРёРЅР° вЂ” `SCREENSHOT_VP_W`
/// Р»РёР±Рѕ С€РёСЂРёРЅР° РёР· `--viewport`, РµСЃР»Рё РѕРЅ Р·Р°РґР°РЅ.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `(width, height)` СЃРѕС…СЂР°РЅС‘РЅРЅРѕРіРѕ PNG.
pub(crate) fn do_screenshot(
    source: &PageSource,
    output: &std::path::Path,
    event_sink: Arc<dyn EventSink>,
    viewport_override: Option<(f32, f32)>,
) -> Result<(u32, u32), Box<dyn Error>> {
    let (png, width, height) = render_source_to_png(source, event_sink, viewport_override)?;
    std::fs::write(output, &png)?;
    Ok((width, height))
}

/// Headless CPU render of `source` to in-memory PNG bytes.
///
/// Core of both `--screenshot` (writes the bytes to a file) and the
/// `--ipc-server` `Screenshot` command (TAB-5, returns the bytes over IPC).
/// Runs the same full pipeline as `do_screenshot` вЂ” `load_bytes` в†’
/// `parse_and_layout` (external CSS + images, no interactive JS, deterministic)
/// в†’ `paint_ordered` в†’ `Renderer::render_to_image_cpu`.
///
/// Returns `(png_bytes, width, height)`. Width is [`SCREENSHOT_VP_W`]; height is
/// the layout-root height clamped to `[SCREENSHOT_MIN_H, SCREENSHOT_MAX_H]`.
pub(crate) fn render_source_to_png(
    source: &PageSource,
    event_sink: Arc<dyn EventSink>,
    viewport_override: Option<(f32, f32)>,
) -> Result<(Vec<u8>, u32, u32), Box<dyn Error>> {
    use lumen_paint::Renderer;

    // BUG-172: each headless render is its own "navigation" вЂ” clear the previous
    // render's decoded images (bounds memory in the long-lived `--ipc-server`) and
    // guarantee no stale cross-page reuse. No streaming path here, so this pass
    // just decodes each image once.
    crate::image_cache::IMAGE_CACHE.reset_new();
    // PERF-1: `--trace-nav` records this headless load as a timeline. Each
    // `trace::span` is a no-op unless the tracer is enabled, so instrumenting
    // this shared path costs nothing for `--screenshot`/`--ipc-server`.
    let raw = {
        let _s = lumen_core::trace::span("fetch-document", "net");
        source.load_bytes(event_sink.clone(), None)?
    };
    let vp = match viewport_override {
        Some((w, h)) => Size::new(w, h),
        None => Size::new(SCREENSHOT_VP_W, SCREENSHOT_MIN_H),
    };
    let parsed = parse_and_layout(
        &raw.bytes,
        raw.content_type,
        &raw.base,
        &event_sink,
        vp,
        &mut std::collections::HashSet::new(),
        None, // ls_store
        None, // ss_store: a headless one-shot render is its own browsing context
        None, // idb_backend
        None, // sw_backend
        &NullHyphenationProvider,
        // BUG-428: this slot is `cookie_banner_dismiss`, not a JS gate вЂ” the
        // former "headless: no interactive JS" label was wrong. Page scripts DO
        // run here (`parse_and_layout` в†’ `run_scripts_with_dom`); only the live
        // event loop's per-frame pumps (timers/rAF) are absent.
        false, // cookie_banner_dismiss: leave banners as authored
        deterministic::DetConfig { enabled: true, ..Default::default() }, // reproducible pixels across runs/OS
        false, // dark_mode: light
        None,  // cookie_jar
        raw.cross_origin_isolated,
        None,  // sw_worker_store
        None,  // cache_backend
        lumen_core::ColorSpace::Srgb,
        false, // media_print: screenshot uses screen media
    )?;

    // РџРѕР»РЅР°СЏ РІС‹СЃРѕС‚Р° СЃС‚СЂР°РЅРёС†С‹ (РєРѕРЅС‚РµРЅС‚ РјРѕР¶РµС‚ Р±С‹С‚СЊ РґР»РёРЅРЅРµРµ СЌРєСЂР°РЅР°), СЃ РїРѕС‚РѕР»РєРѕРј.
    let content_h = parsed
        .layout
        .rect
        .height
        .clamp(vp.height, SCREENSHOT_MAX_H);
    let width = vp.width as u32;
    let height = content_h.ceil() as u32;

    // BUG-428: Canvas 2D pixels live in per-node CPU buffers inside the JS runtime
    // and only reach paint through a drain (`flush_canvas_updates` в†’ an image
    // registered under `canvas:{nid}`). The live event loop does that drain every
    // frame; this headless path never did, so `DrawImage { src: "canvas:{nid}" }`
    // always resolved to an unregistered key and painted transparent. Drain once вЂ”
    // the page scripts have already run inside `parse_and_layout` вЂ” and append the
    // bitmaps to the image set handed to the CPU rasterizer.
    let mut images = parsed.images;
    images.extend(canvas_updates_as_images(
        parsed
            .js_ctx
            .as_ref()
            .map(|js| js.flush_canvas_updates())
            .unwrap_or_default(),
    ));

    let (png, width, height) = {
        let _s = lumen_core::trace::span("paint", "paint");
        let dl = paint_ordered(&parsed.layout);
        let image = Renderer::render_to_image_cpu(width, height, &dl, &images, 0.0, 0.0)?;
        let png = lumen_image::encode_png_rgba8(&image)?;
        (png, width, height)
    };
    // Pixels are ready вЂ” mark the moment the page is first fully rendered.
    lumen_core::trace::instant("first-paint", "paint");
    Ok((png, width, height))
}

/// Convert a `PersistentJs::flush_canvas_updates` drain into renderer image
/// entries keyed exactly the way the display list refers to them.
///
/// Each drained tuple is `(node_index, width, height, rgba)`; the key is
/// `canvas:{node_index}` вЂ” the same string `display_list.rs` puts into
/// `DisplayCommand::DrawImage.src` for a `<canvas>` box. Both consumers of the
/// drain go through here (the live event loop registers the entries with the
/// renderer, the headless CPU path appends them to its image set), so the key
/// format has a single definition on the shell side (BUG-428).
pub(crate) fn canvas_updates_as_images(
    updates: Vec<(u32, u32, u32, Vec<u8>)>,
) -> Vec<(String, Arc<lumen_image::Image>)> {
    updates
        .into_iter()
        .map(|(nid, width, height, rgba)| {
            let image = lumen_image::Image {
                width,
                height,
                format: lumen_image::PixelFormat::Rgba8,
                data: rgba,
                icc_profile: None,
            };
            (format!("canvas:{nid}"), Arc::new(image))
        })
        .collect()
}

pub(crate) fn do_print_to_pdf(
    source: &PageSource,
    output: &std::path::Path,
    event_sink: Arc<dyn EventSink>,
) -> Result<usize, Box<dyn Error>> {
    use lumen_layout::{paginate, PaginationContext};
    use lumen_paint::{build_print_display_list, split_at_page_breaks, Renderer};

    let raw = source.load_bytes(event_sink.clone(), None)?;
    let vp = Size::new(PDF_PAGE_W as f32, PDF_PAGE_H as f32);
    let parsed = parse_and_layout(
        &raw.bytes,
        raw.content_type,
        &raw.base,
        &event_sink,
        vp,
        &mut std::collections::HashSet::new(),
        None, // ls_store
        None, // ss_store: a headless one-shot render is its own browsing context
        None, // idb_backend
        None, // sw_backend
        &NullHyphenationProvider,
        false, // headless PDF mode: no interactive JS needed
        deterministic::DetConfig::default(), // deterministic: not needed for PDF rendering
        false, // dark_mode: light mode for PDF output
        None,  // cookie_jar: not available in standalone PDF mode
        raw.cross_origin_isolated,
        None,  // sw_worker_store: not needed in headless PDF mode
        None,  // cache_backend: not needed in headless PDF mode
        lumen_core::ColorSpace::Srgb,
        true,  // media_print: apply @media print for PDF output (BUG-270)
    )?;

    let ctx = PaginationContext {
        page_width: PDF_PAGE_W as f32,
        page_height: PDF_PAGE_H as f32,
        margin_top: 48.0,
        margin_bottom: 48.0,
        margin_left: 48.0,
        margin_right: 48.0,
    };
    let mut pages = paginate(&parsed.layout, &ctx);
    let page_count_total = pages.len() as u32;
    // Attach @page margin-box data: page N of M at bottom-center.
    attach_page_boxes(&mut pages, page_count_total, &ctx);
    let cmds = build_print_display_list(&pages);
    let split_pages = split_at_page_breaks(cmds);

    let images = Renderer::render_print_pages(
        INTER_FONT.to_vec(),
        &split_pages,
        PDF_PAGE_W,
        PDF_PAGE_H,
        ColorSpace::Srgb,
    )?;

    let page_count = images.len();
    let pdf_bytes = encode_images_as_pdf(&images, PDF_PAGE_W, PDF_PAGE_H);
    std::fs::write(output, &pdf_bytes)?;
    Ok(page_count)
}

/// Per-job settings for [`do_print_to_pdf_with_opts`] (E-1, `landscape`
/// added [BUG-420](../../../bugs/BUG-420-FIXED.md)) вЂ” grouped into a struct
/// to keep the function under clippy's `too_many_arguments` threshold.
pub(crate) struct PrintOptions {
    /// Top + bottom margin, in CSS px.
    pub(crate) margin_tb: f32,
    /// Left + right margin, in CSS px.
    pub(crate) margin_lr: f32,
    /// Content zoom, in percent (50вЂ“200).
    pub(crate) scale: i32,
    /// `false` strips CSS background fills/images/gradients before rasterising.
    pub(crate) print_backgrounds: bool,
    /// `true` swaps the output page's raster width/height. Independent of
    /// `scale`, which still zooms content within the (possibly swapped) page.
    pub(crate) landscape: bool,
}

/// Print with custom margin values (in CSS px) from the print dialog (E-1).
pub(crate) fn do_print_to_pdf_with_opts(
    source: &PageSource,
    output: &std::path::Path,
    event_sink: Arc<dyn EventSink>,
    opts: PrintOptions,
) -> Result<usize, Box<dyn Error>> {
    use lumen_layout::{paginate, PaginationContext};
    use lumen_paint::{
        build_print_display_list, split_at_page_breaks, strip_background_graphics, Renderer,
    };
    let PrintOptions { margin_tb, margin_lr, scale, print_backgrounds, landscape } = opts;

    let raw = source.load_bytes(event_sink.clone(), None)?;
    let (page_w, page_h) = if landscape { (PDF_PAGE_H, PDF_PAGE_W) } else { (PDF_PAGE_W, PDF_PAGE_H) };
    // Apply scale to viewport (W-2b): 50вЂ“200% zoom
    let scale_factor = scale as f32 / 100.0;
    let scaled_w = (page_w as f32 * scale_factor).ceil();
    let scaled_h = (page_h as f32 * scale_factor).ceil();
    let vp = Size::new(scaled_w, scaled_h);
    let parsed = parse_and_layout(
        &raw.bytes,
        raw.content_type,
        &raw.base,
        &event_sink,
        vp,
        &mut std::collections::HashSet::new(),
        None, // ls_store
        None, // ss_store: a headless one-shot render is its own browsing context
        None, // idb_backend
        None, // sw_backend
        &NullHyphenationProvider,
        false, // cookie_banner_dismiss
        deterministic::DetConfig::default(), // deterministic: not needed for PDF rendering
        false, // dark_mode
        None,
        raw.cross_origin_isolated,
        None, // sw_worker_store: not needed in headless print mode
        None, // cache_backend: not needed in headless print mode
        lumen_core::ColorSpace::Srgb,
        true, // media_print: apply @media print for PDF output (BUG-270)
    )?;

    let ctx = PaginationContext {
        page_width: scaled_w,
        page_height: scaled_h,
        margin_top: margin_tb,
        margin_bottom: margin_tb,
        margin_left: margin_lr,
        margin_right: margin_lr,
    };
    let mut pages = paginate(&parsed.layout, &ctx);
    let page_count_total = pages.len() as u32;
    attach_page_boxes(&mut pages, page_count_total, &ctx);
    let cmds = build_print_display_list(&pages);
    let mut split_pages = split_at_page_breaks(cmds);
    // CC-8: drop CSS background graphics when the dialog toggle is off.
    strip_background_graphics(&mut split_pages, print_backgrounds);

    let images = Renderer::render_print_pages(
        INTER_FONT.to_vec(),
        &split_pages,
        page_w,
        page_h,
        ColorSpace::Srgb,
    )?;

    let page_count = images.len();
    let pdf_bytes = encode_images_as_pdf(&images, page_w, page_h);
    std::fs::write(output, &pdf_bytes)?;
    Ok(page_count)
}

/// Default PDF output path when the caller has no explicit path (JS
/// `window.print()` with no `outputPath`, or the engine chrome's "РџРµС‡Р°С‚СЊ"
/// button, [BUG-420](../../../bugs/BUG-420-FIXED.md)): `document.pdf` in the
/// current directory, or `document_<unix-seconds>.pdf` if that already
/// exists вЂ” avoids silently clobbering a previous export.
pub(crate) fn default_pdf_output_path() -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut path = std::env::current_dir().unwrap_or_default();
    path.push("document.pdf");
    if path.exists() {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        path.set_file_name(format!("document_{}.pdf", ts));
    }
    path
}

/// Attaches `PageBox` data to each page with default @page content: page N of M at bottom-center.
///
/// Uses a fixed-width measurer (8 px/char at any font size) for margin-box text layout,
/// matching the Phase 0 text-measurement approach used in layout tests. Shell has no
/// access to a real `TextMeasurer` outside the full layout pipeline, and margin-box
/// text is short (page numbers) so the approximation is acceptable.
pub(crate) fn attach_page_boxes(
    pages: &mut [lumen_layout::pagination::Page],
    total: u32,
    ctx: &lumen_layout::PaginationContext,
) {
    use lumen_layout::{MarginBoxPosition, PageBox, PageProperties, TextMeasurer};

    /// Fixed 8 px per character at any size вЂ” matches the Phase 0 layout test measurer.
    struct Fixed8;
    impl TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }

    let props = PageProperties {
        width: ctx.page_width,
        height: ctx.page_height,
        orientation: if ctx.page_width > ctx.page_height { "landscape".to_string() } else { "portrait".to_string() },
        margin_top: ctx.margin_top,
        margin_bottom: ctx.margin_bottom,
        margin_left: ctx.margin_left,
        margin_right: ctx.margin_right,
    };

    for page in pages.iter_mut() {
        let mut page_box = PageBox::new(page.number, props.clone());
        page_box.layout_margin_boxes();

        let label = format!("{} / {}", page.number + 1, total);
        let font_size = 10.0_f32;
        let line_height = font_size * 1.5;
        if let Some(mb) = page_box.margin_boxes.get_mut(&MarginBoxPosition::BottomCenter) {
            mb.content = Some(label.clone());
            mb.layout_text(&label, font_size, line_height, &Fixed8);
        }

        page.page_box = Some(page_box);
    }
}

/// РљРѕРґРёСЂСѓРµС‚ РЅР°Р±РѕСЂ СЂР°СЃС‚СЂРѕРІС‹С… РёР·РѕР±СЂР°Р¶РµРЅРёР№ РІ PDF-С„Р°Р№Р» (РїРѕ РѕРґРЅРѕРјСѓ РЅР° СЃС‚СЂР°РЅРёС†Сѓ).
///
/// Р Р°Р·РјРµСЂ СЃС‚СЂР°РЅРёС†С‹ Р·Р°РґР°С‘С‚СЃСЏ `page_w Г— page_h` РІ PDF-РµРґРёРЅРёС†Р°С… (1 unit = 1 px @ 96 DPI).
/// РР·РѕР±СЂР°Р¶РµРЅРёСЏ РІСЃС‚СЂР°РёРІР°СЋС‚СЃСЏ РєР°Рє DeviceRGB XObject Р±РµР· СЃР¶Р°С‚РёСЏ.
pub(crate) fn encode_images_as_pdf(images: &[lumen_image::Image], page_w: u32, page_h: u32) -> Vec<u8> {
    use pdf_writer::{Content, Name, Pdf, Rect, Ref};

    if images.is_empty() {
        return Pdf::new().finish();
    }

    let n = images.len() as i32;
    let mut pdf = Pdf::new();

    // Р Р°СЃРїСЂРµРґРµР»СЏРµРј PDF-РѕР±СЉРµРєС‚ IDs:
    //   1            = catalog
    //   2            = page tree
    //   3 .. 3+n-1   = СЃС‚СЂР°РЅРёС†С‹
    //   3+n .. 3+2n-1 = РїРѕС‚РѕРєРё СЃРѕРґРµСЂР¶РёРјРѕРіРѕ
    //   3+2n .. 3+3n-1 = image XObjects
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_ids: Vec<Ref> = (0..n).map(|i| Ref::new(3 + i)).collect();
    let content_ids: Vec<Ref> = (0..n).map(|i| Ref::new(3 + n + i)).collect();
    let image_ids: Vec<Ref> = (0..n).map(|i| Ref::new(3 + 2 * n + i)).collect();

    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id)
        .kids(page_ids.iter().copied())
        .count(n);

    let media = Rect::new(0.0, 0.0, page_w as f32, page_h as f32);

    for (i, image) in images.iter().enumerate() {
        let idx = i as i32;
        let img_name = format!("Im{idx}");
        let img_w = image.width;
        let img_h = image.height;

        // РЎС‚СЂР°РЅРёС†Р°
        {
            let mut page = pdf.page(page_ids[i]);
            page.media_box(media);
            page.parent(page_tree_id);
            page.contents(content_ids[i]);
            page.resources()
                .x_objects()
                .pair(Name(img_name.as_bytes()), image_ids[i]);
        }

        // РџРѕС‚РѕРє СЃРѕРґРµСЂР¶РёРјРѕРіРѕ: cm-РјР°С‚СЂРёС†Р° + Do РѕРїРµСЂР°С‚РѕСЂ.
        // РњР°С‚СЂРёС†Р° [w 0 0 -h 0 h] СЂР°Р·РјРµС‰Р°РµС‚ РёР·РѕР±СЂР°Р¶РµРЅРёРµ РЅР° РІСЃСЋ СЃС‚СЂР°РЅРёС†Сѓ
        // Рё РїРµСЂРµРІРѕСЂР°С‡РёРІР°РµС‚ РїРѕ Y (PDF: РЅР°С‡Р°Р»Рѕ РєРѕРѕСЂРґРёРЅР°С‚ РІРЅРёР·Сѓ СЃР»РµРІР°).
        let content_bytes = {
            let mut c = Content::new();
            c.save_state();
            c.transform([img_w as f32, 0.0, 0.0, -(img_h as f32), 0.0, img_h as f32]);
            c.x_object(Name(img_name.as_bytes()));
            c.restore_state();
            c.finish()
        };
        pdf.stream(content_ids[i], &content_bytes);

        // Image XObject: DeviceRGB Р±РµР· Р°Р»СЊС„Р°-РєР°РЅР°Р»Р°
        let rgba = image.to_rgba8();
        let rgb: Vec<u8> = rgba
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let mut xobj = pdf.image_xobject(image_ids[i], &rgb);
        xobj.width(img_w as i32);
        xobj.height(img_h as i32);
        xobj.color_space().device_rgb();
        xobj.bits_per_component(8);
    }

    pdf.finish()
}

/// Viewport РґР»СЏ `--dump-layout`/`--dump-display-list`.
///
/// РџРѕ СѓРјРѕР»С‡Р°РЅРёСЋ 1024Г—720 вЂ” С‚РѕС‚ Р¶Рµ СЂР°Р·РјРµСЂ, С‡С‚Рѕ Сѓ `--screenshot` Рё РіСЂР°С„РёС‡РµСЃРєРёС…
/// С‚РµСЃС‚РѕРІ, РїРѕСЌС‚РѕРјСѓ РґР°РјРїС‹ СЃРѕРїРѕСЃС‚Р°РІРёРјС‹ СЃРѕ СЃРЅРёРјРєР°РјРё. `--viewport WxH` РµРіРѕ
/// РїРµСЂРµРѕРїСЂРµРґРµР»СЏРµС‚: Р±РµР· СЌС‚РѕРіРѕ РґРµС„РµРєС‚С‹ СЂР°СЃРєР»Р°РґРєРё, Р·Р°РІРёСЃСЏС‰РёРµ РѕС‚ С€РёСЂРёРЅС‹ РѕРєРЅР°
/// (BUG-424 вЂ” РїСЂР°РІС‹Р№ РєР»Р°СЃС‚РµСЂ С‚СѓР»Р±Р°СЂР° РЅР° 1920), РЅРµР»СЊР·СЏ РЅРё РІРѕСЃРїСЂРѕРёР·РІРµСЃС‚Рё
/// headless, РЅРё Р·Р°РєСЂС‹С‚СЊ С‚РµСЃС‚РѕРј.
pub(crate) fn dump_viewport(viewport_override: Option<(f32, f32)>) -> Size {
    match viewport_override {
        Some((w, h)) => Size::new(w, h),
        None => Size::new(1024.0, 720.0),
    }
}

pub(crate) fn run_dump(
    source: &PageSource,
    kind: DumpKind,
    event_sink: Arc<dyn EventSink>,
    viewport_override: Option<(f32, f32)>,
) -> Result<(), Box<dyn Error>> {
    let cookie_jar = Arc::new(lumen_storage::CookieJar::open_in_memory()?);
    let raw = source.load_bytes(event_sink.clone(), Some(cookie_jar))?;
    let dump_vp = dump_viewport(viewport_override);
    match kind {
        DumpKind::Source => {
            let encoding = lumen_encoding::detect(&raw.bytes, raw.content_type);
            let decoded = lumen_encoding::decode(encoding, &raw.bytes);
            eprintln!("РљРѕРґРёСЂРѕРІРєР°: {}", encoding.name());
            print!("{decoded}");
            Ok(())
        }
        DumpKind::Layout => {
            let vp = dump_vp;
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink, vp, &mut std::collections::HashSet::new(), None, None, None, None, &NullHyphenationProvider, false, deterministic::DetConfig::default(), false, None, false, None, None, lumen_core::ColorSpace::Srgb, false)?;
            print!("{}", lumen_layout::serialize_layout_tree(&parsed.layout));
            Ok(())
        }
        DumpKind::DisplayList => {
            let vp = dump_vp;
            let parsed = parse_and_layout(&raw.bytes, raw.content_type, &raw.base, &event_sink, vp, &mut std::collections::HashSet::new(), None, None, None, None, &NullHyphenationProvider, false, deterministic::DetConfig::default(), false, None, false, None, None, lumen_core::ColorSpace::Srgb, false)?;
            let mut dl = paint_ordered(&parsed.layout);
            // BUG-480 срез 14: дамп обязан показывать то же, что попадёт на
            // экран, — окно вклеивает содержимое под-документов в список
            // страницы (`Lumen::set_display_list`), и без этой строки дамп
            // остался бы единственным местом, где `<iframe>` — серая заглушка.
            // Это же и единственный headless-способ проверить сам срез.
            crate::frames::splice_frame_content(&mut dl, &parsed.frames);
            print!("{}", lumen_paint::serialize_display_list(&dl));
            Ok(())
        }
    }
}
