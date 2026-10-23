//! Houdini Paint API host glue: from the laid-out page to the bitmaps `paint()` draws.
//!
//! The display list refers to a `background-image: paint()` layer by a key
//! (`lumen_paint::collect_paint_requests`); the paint worklet in the JS runtime draws
//! the bitmap on request. This file is the shell's half of that hand-over, shared by
//! the live window ([`Lumen::pump_paint_worklet`], every frame) and the headless
//! snapshot ([`paint_worklet_images`], once).

use std::sync::Arc;

use crate::persistent_js::PersistentJs;

/// Renderer image entries for the bitmaps a paint worklet drew, keyed the way the
/// display list refers to them.
pub(crate) fn paint_images_as_images(
    drawn: Vec<lumen_core::PaintImage>,
) -> Vec<(String, Arc<lumen_image::Image>)> {
    drawn
        .into_iter()
        .map(|(key, width, height, rgba)| {
            let image = lumen_image::Image {
                width,
                height,
                format: lumen_image::PixelFormat::Rgba8,
                data: rgba,
                icc_profile: None,
                vector_source: None,
            };
            (key, Arc::new(image))
        })
        .collect()
}

/// Headless snapshot: run the paint worklet over every `paint()` layer of `layout`
/// once and return the bitmaps. Empty — without a JS round trip — when the page never
/// called `registerPaint()`.
#[cfg(feature = "v8")]
pub(crate) fn paint_worklet_images(
    js: &dyn PersistentJs,
    layout: &lumen_layout::LayoutBox,
    dpr: f32,
) -> Vec<(String, Arc<lumen_image::Image>)> {
    if lumen_js::paint_worklet_epoch() == 0 {
        return Vec::new();
    }
    let jobs = lumen_paint::collect_paint_requests(layout, dpr);
    if jobs.is_empty() {
        return Vec::new();
    }
    js.set_paint_jobs(jobs);
    paint_images_as_images(js.flush_paint_images())
}

#[cfg(feature = "v8")]
impl crate::Lumen {
    /// Live window: keep the paint worklet's bitmaps in step with the page. Once a
    /// page has registered a paint class, every frame collects the `paint()` layers of
    /// the current layout, hands them to the worklet when they changed and registers
    /// whatever it redrew (new layer, new size, changed `inputProperties`) with the
    /// renderer. A page that never used the Paint API pays one atomic load.
    pub(crate) fn pump_paint_worklet(&mut self) {
        if lumen_js::paint_worklet_epoch() == 0 {
            return;
        }
        let Some(layout) = self.layout_box.as_ref() else { return };
        let jobs = lumen_paint::collect_paint_requests(layout, self.zoom_factor);
        if jobs.is_empty() && self.paint_jobs_sent.is_empty() {
            return;
        }
        let changed = jobs != self.paint_jobs_sent;
        let to_send = changed.then(|| jobs.clone());
        let drawn = self.drain_query_js(move |j| {
            if let Some(jobs) = to_send {
                j.set_paint_jobs(jobs);
            }
            j.flush_paint_images()
        });
        let Some(drawn) = drawn else { return };
        if changed {
            self.paint_jobs_sent = jobs;
        }
        if drawn.is_empty() {
            return;
        }
        if let Some(r) = self.renderer.as_mut() {
            for (key, image) in paint_images_as_images(drawn) {
                if let Err(e) = r.register_image(key.clone(), image) {
                    eprintln!("Paint worklet: не зарегистрирован {key}: {e}");
                }
            }
        }
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}
