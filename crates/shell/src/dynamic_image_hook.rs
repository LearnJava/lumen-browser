//! BUG-1118: `lumen_core::ext::ImageLoadHook` implementation wired into
//! `lumen_js::v8_runtime::V8JsRuntime` for the top-level document's own
//! runtime, so a script assigning `<img>.src` starts the fetch immediately
//! (HTML LS §4.8.4.3), instead of waiting for the post-relayout sweep
//! ([`crate::Lumen::spawn_dynamic_image_loads`]) — which cannot run at all
//! while the runtime is still inside the very script that made the
//! assignment (`run_scripts_with_dom` blocks the calling thread until every
//! parser/inline script returns).
//!
//! Mirrors the per-request body of [`crate::Lumen::spawn_image_requests`]
//! for exactly one `(node, raw src)` pair — same decode path
//! ([`crate::subresources::decode_image`] through
//! [`crate::image_cache::IMAGE_CACHE`]), same CSP gate, same dedup set (see
//! [`DynamicImageHookCtx`]), same `LoadEvent` completion signal — just
//! triggered from the JS runtime's own thread instead of a shell-thread
//! relayout pass.

use std::sync::{Arc, Mutex};

use lumen_core::ext::EventSink;
use lumen_network::csp::CspPolicy;

use crate::page_load::LoadEvent;
use crate::resource_base::ResourceBase;
use crate::{image_cache, subresources};

/// Everything [`DynamicImgFetchHook`] needs that only a live `Lumen` (not a
/// headless/test render) can provide — see [`crate::page_pipeline::parse_and_layout`]'s
/// doc comment on its `dynamic_image_hook_ctx` parameter for why this is one
/// bundle instead of three parameters.
pub(crate) struct DynamicImageHookCtx {
    /// `Lumen::load_generation` at the time this navigation started —
    /// stamps the decode so it shares the cache entry with the final
    /// pipeline pass (BUG-172), same as `spawn_image_requests`.
    pub(crate) generation: u64,
    /// `Lumen::stream_images_requested` — shared with `spawn_image_requests`/
    /// `spawn_stream_image_loads` so an `<img>` this hook already fetched is
    /// never re-fetched by the later sweep, and vice versa.
    pub(crate) dedup: Arc<Mutex<std::collections::HashSet<String>>>,
    /// `Lumen::load_proxy` — posts `LoadEvent::ImageDecoded`/
    /// `ImageDecodeFailed` back to the shell's event loop, same as every
    /// other background decode thread.
    pub(crate) proxy: winit::event_loop::EventLoopProxy<LoadEvent>,
}

/// See the module doc comment.
pub(crate) struct DynamicImgFetchHook {
    pub(crate) base: ResourceBase,
    pub(crate) csp_gate: Option<(Vec<CspPolicy>, String)>,
    pub(crate) sink: Arc<dyn EventSink>,
    pub(crate) cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    pub(crate) target: lumen_core::ColorSpace,
    pub(crate) referrer_policy: lumen_network::ReferrerPolicy,
    pub(crate) ctx: DynamicImageHookCtx,
}

impl lumen_core::ext::ImageLoadHook for DynamicImgFetchHook {
    fn queue_image_load(&self, raw_src: &str) {
        // Same dedup set `spawn_image_requests` inserts into — whichever of
        // the two producers gets here first wins, the other skips.
        {
            let mut requested = self.ctx.dedup.lock().unwrap_or_else(|e| e.into_inner());
            if !requested.insert(raw_src.to_string()) {
                return;
            }
        }
        let self_origin = self.base.origin();
        let resolved_url = self.base.resolve_str(raw_src);
        // GAP-CSPENF срез 43: upgrade before the block gate, same order as
        // `spawn_image_requests`.
        let upgraded = self
            .csp_gate
            .as_ref()
            .and_then(|(policy, _)| crate::csp_enforce::upgrade_insecure_url(policy, &resolved_url));
        let resolved_url = upgraded.clone().unwrap_or(resolved_url);
        if let Some((policy, _original)) = &self.csp_gate
            && crate::csp_enforce::img_src_blocked(policy, &resolved_url, self_origin.as_ref())
        {
            let _ = self
                .ctx
                .proxy
                .send_event(LoadEvent::ImageDecodeFailed { src: raw_src.to_string() });
            return;
        }
        let raw_src = raw_src.to_string();
        let base = self.base.clone();
        let sink = Arc::clone(&self.sink);
        let cookie_jar = self.cookie_jar.clone();
        let target = self.target;
        let referrer_policy = self.referrer_policy;
        let generation = self.ctx.generation;
        let proxy = self.ctx.proxy.clone();
        std::thread::spawn(move || {
            let fetch_url: &str = upgraded.as_deref().unwrap_or(&raw_src);
            let decoded = image_cache::IMAGE_CACHE.get_or_decode(generation, &raw_src, || {
                subresources::decode_image(fetch_url, &base, &sink, cookie_jar, target, referrer_policy)
            });
            match decoded {
                None => {
                    let _ = proxy.send_event(LoadEvent::ImageDecodeFailed { src: raw_src });
                }
                Some(image_cache::DecodedImage::Static(img)) => {
                    let _ = proxy.send_event(LoadEvent::ImageDecoded {
                        src: raw_src,
                        image: Box::new((*img).clone()),
                        animated: None,
                    });
                }
                Some(image_cache::DecodedImage::Animated { first, gif }) => {
                    let _ = proxy.send_event(LoadEvent::ImageDecoded {
                        src: raw_src,
                        image: Box::new((*first).clone()),
                        animated: Some(Box::new((*gif).clone())),
                    });
                }
            }
        });
    }
}
