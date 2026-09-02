//! FRAME-5 срез 2: fetch, decode and register `<img loading="lazy">` inside an
//! `<iframe>` sub-document once it enters the frame's OWN proximity margin.
//!
//! Split from `crate::frames` because the two halves of the job run on
//! different sides of the UI-thread boundary. The JS-side proximity check
//! (`frames::harvest_frame_lazy_requests`, called from
//! `sync_frame_viewports`/`relayout_frame_content`) only touches the frame's
//! own `Arc<dyn PersistentJs>` — safe off the UI thread, including during the
//! initial `parse_and_layout`. Turning a fired `(nid, url)` request into
//! pixels needs a network fetch (also off-thread-safe, srez 15 already does
//! this for eager `<img>`) plus, to actually SHOW them, `Lumen::renderer`/
//! `image_cache` — UI-thread-only, and with no page-commit step to
//! piggy-back on outside the initial load (unlike srez 15's eager images,
//! which ride `LoadedPage::images` into the normal commit path).

use crate::*;
use crate::frames::{frame_image_key, FrameHandle};

/// Newly decoded pixels/animations from one [`fetch_frame_lazy_images`] call —
/// already folded into the frame's own `images`/`animated_gifs` (so the
/// initial-load merge in `page_pipeline.rs` picks them up for free), and
/// handed back so a relayout-time caller (`&mut Lumen`, no page-commit step)
/// can register them into the live renderer right away.
#[derive(Default)]
pub(crate) struct FrameLazyLoaded {
    pub(crate) images: Vec<(String, Arc<lumen_image::Image>)>,
    pub(crate) animated_gifs: Vec<(String, lumen_image::AnimatedGif)>,
}

/// Drain `frame.pending_lazy` (harvested by `frames::harvest_frame_lazy_requests`
/// from the frame's own `IntersectionObserver`) and fetch+decode each URL —
/// same network/decode path `fetch_frame_subresources` already uses for eager
/// `<img>` (срез 15): `IMAGE_CACHE` dedup by [`frame_image_key`], intrinsic
/// size applied to the frame's OWN document when the author did not set both
/// dimensions.
///
/// Safe to call off the UI thread (the initial load, before `frame` is even
/// pushed into `Lumen::frames`) — nothing here touches `Lumen::renderer`/
/// `image_cache`.
pub(crate) fn fetch_frame_lazy_images(
    frame: &mut FrameHandle,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
) -> FrameLazyLoaded {
    let pending = std::mem::take(&mut frame.pending_lazy);
    if pending.is_empty() {
        return FrameLazyLoaded::default();
    }
    let base = frame.base.clone();
    // Each thread gets its own `Arc` clone rather than sharing the caller's
    // `&Arc<dyn EventSink>` reference across the pool — same pattern
    // `fetch_frame_subresources` uses for the same reason (srez 11).
    let decoded = parallel_map(&pending, |_, (_nid, url)| {
        let sink: &Arc<dyn EventSink> = &sink.clone();
        let key = frame_image_key(&base, url);
        let img = crate::image_cache::IMAGE_CACHE.get_or_decode_current(&key, || {
            decode_image(url, &base, sink, cookie_jar.clone(), target)
        });
        (key, img)
    });
    let mut loaded = FrameLazyLoaded::default();
    for ((nid, url), (key, img)) in pending.iter().zip(decoded) {
        let image = match img {
            None => {
                eprintln!("iframe lazy: не загрузилась {url}");
                continue;
            }
            Some(crate::image_cache::DecodedImage::Static(i)) => i,
            Some(crate::image_cache::DecodedImage::Animated { first, gif }) => {
                let gif = (*gif).clone();
                frame.animated_gifs.push((key.clone(), gif.clone()));
                loaded.animated_gifs.push((key.clone(), gif));
                first
            }
        };
        // BUG-269, как у срез-15 eager-пути: intrinsic нужен, если автор не
        // задал ХОТЯ БЫ одно измерение.
        let wants_intrinsic = frame
            .lazy_requests
            .iter()
            .find(|r| r.node_id.index() as u32 == *nid)
            .is_some_and(|r| !(r.has_explicit_width && r.has_explicit_height));
        if wants_intrinsic {
            let node_id = NodeId::from_index(*nid as usize);
            if let Ok(mut doc) = frame.doc.lock() {
                lumen_layout::apply_intrinsic_size(&mut doc, node_id, image.width, image.height);
            }
        }
        frame.images.push((key.clone(), Arc::clone(&image)));
        loaded.images.push((key, image));
    }
    loaded
}

impl Lumen {
    /// FRAME-5 срез 2: register every frame's newly lazy-loaded images (if
    /// any) into the live renderer/CPU-snapshot cache and the page-wide
    /// animated-GIF ticker.
    ///
    /// Called once per relayout pass, right after `sync_frame_viewports`/
    /// `relayout_frame_content` have had a chance to harvest fresh proximity
    /// hits — mirrors `Self::fetch_and_register_lazy_images`'s page-level
    /// registration loop (`page_load.rs`), just batched across frames.
    pub(crate) fn register_frame_lazy_images(&mut self) {
        if self.frames.iter().all(|h| h.pending_lazy.is_empty()) {
            return;
        }
        let sink = Arc::clone(&self.event_sink);
        let cookie_jar = self.active_cookie_jar();
        let target = self.target_color_space();
        for i in 0..self.frames.len() {
            let loaded = frame_lazy::fetch_frame_lazy_images(
                &mut self.frames[i],
                &sink,
                Some(Arc::clone(&cookie_jar)),
                target,
            );
            for (key, image) in loaded.images {
                if let Some(r) = self.renderer.as_mut()
                    && let Err(e) = r.register_image(key.clone(), Arc::clone(&image))
                {
                    eprintln!("iframe lazy: картинка не зарегистрирована {key}: {e}");
                }
                self.image_cache.insert(lumen_image::ImageKey::new(&key), (*image).clone());
            }
            for (key, gif) in loaded.animated_gifs {
                self.gif_last_frame.remove(&key);
                self.animated_gifs.insert(key, gif);
            }
        }
        self.request_redraw();
    }
}
