//! Page loading in the shell: reload, the streaming load, image/media
//! subresources of the current document and committing a `LoadedPage`.
//!
//! SPLIT-SH1 (2026-08-26): moved verbatim out of `main.rs`. Behaviour, order of
//! operations and method bodies are unchanged; only module path and visibility
//! (`fn` -> `pub(crate) fn`, required for a caller in the parent module) differ.
//!
//! File-size gate (docs/lint-policy.md §5.1): `<video>` playback ticks live in
//! `video_tick.rs`, `@font-face` fetches in `web_font_fetch.rs`.

use crate::*;
#[cfg(feature = "v8")]
use crate::layout_shift::compute_layout_shift_score;

impl Lumen {
    /// Fetch, decode and register lazy images whose node IDs were queued by JS.
    ///
    /// Called from `relayout()` after `_lumen_deliver_lazy_images()` fires load
    /// requests for images that entered the lazy-load proximity margin.
    /// Fetched images are registered in the renderer immediately so the next
    /// repaint (already requested by `relayout`) shows them.
    #[cfg(feature = "v8")]
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn fetch_and_register_lazy_images(&mut self, requests: Vec<(u32, String)>) {
        let base = match &self.source {
            PageSource::File(p) => ResourceBase::File(p.clone()),
            PageSource::Url { url, .. } => ResourceBase::Url(url.clone()),
            PageSource::Snapshot { base_url, .. } => ResourceBase::Url(base_url.clone()),
            PageSource::Empty | PageSource::AboutBlank | PageSource::Static { .. } => return,
        };
        // GAP-CSPENF срез 9: this is the deferred `loading="lazy"` path — the
        // only image producer left with no `img-src` gate at all (the eager
        // pipeline gates in `fetch_and_decode_images`, the streaming/dynamic
        // producer in `spawn_image_requests`, срез 4 both). A page whose
        // policy blocks an origin could still pull bytes from it just by
        // marking the `<img>` lazy. Same one-shot policy read as those two
        // callers; `self.layout_source` is the live document this JS-queued
        // request came from.
        let csp_gate = self.layout_source.as_ref().and_then(|src| {
            let doc = src.document.lock().unwrap();
            let root = doc.root();
            crate::csp_enforce::document_csp_policy(&doc, root)
        });
        // GAP-REFERRER срез 5: same one-shot read as `csp_gate` above — this
        // deferred path's only point with a live `&Document`.
        let referrer_policy = self.layout_source.as_ref().map_or_else(
            lumen_network::ReferrerPolicy::default_policy,
            |src| crate::resource_base::document_referrer_policy(&src.document.lock().unwrap()),
        );
        // BUG-752: this deferred path re-reads `<base href>` on every batch
        // (not once at page load), same as `_lumen_document_base_url()` on the
        // JS side — a script can insert/change `<base>` well after the initial
        // parse, and by the time a lazy image scrolls into view that is likely.
        // `self_origin` (CSP/mixed-content identity) stays on the un-adjusted
        // `base`: `<base>` changes what a relative URL resolves to, not the
        // document's origin.
        let eff_base = self
            .layout_source
            .as_ref()
            .map(|src| effective_base(&src.document.lock().unwrap(), &base))
            .unwrap_or_else(|| base.clone());
        let self_origin = base.origin();
        for (nid, url) in requests {
            // GAP-CSPENF срез 43: та же перезапись схемы до гейта, что в
            // двух других producer'ах картинок (eager и streaming).
            let upgraded = csp_gate.as_ref().and_then(|(policy, _)| {
                crate::csp_enforce::upgrade_insecure_url(policy, &eff_base.resolve_str(&url))
            });
            if let Some((policy, _)) = &csp_gate {
                let resolved = upgraded.clone().unwrap_or_else(|| eff_base.resolve_str(&url));
                // Срез 56/58: `originalPolicy` — текст КАЖДОЙ нарушенной
                // политики (CSP3 §7.8/§3.4), не только первой; блокировка
                // остаётся однократной.
                let violated: Vec<String> = crate::csp_enforce::violating_fetch_policy(
                    policy, &lumen_network::csp::CspDirective::ImgSrc, &resolved, self_origin.as_ref(),
                ).into_iter().map(str::to_owned).collect();
                if !violated.is_empty() {
                    route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                        for policy_text in &violated {
                            j.fire_csp_violation("img-src", &resolved, policy_text);
                        }
                        j.fire_image_error(nid);
                    });
                    continue;
                }
            }
            // Ключ реестра картинок остаётся сырым `url`; апгрейд (срез 43)
            // меняет только адрес запроса.
            let fetch_url: &str = upgraded.as_deref().unwrap_or(&url);
            // GAP-REFERRER срез 8: element-wins-over-document override, same
            // as the eager/frame-lazy `<img>` paths (срез 7) — this deferred
            // path has no `ImageRequest` in scope, so the attribute is read
            // straight off the live node by `nid` instead of carried in a
            // struct field.
            let img_referrer_policy = self
                .layout_source
                .as_ref()
                .and_then(|src| src.document.lock().ok())
                .and_then(|doc| {
                    doc.try_get(NodeId::from_index(nid as usize))
                        .and_then(|node| node.get_attr("referrerpolicy"))
                        .and_then(lumen_network::ReferrerPolicy::parse)
                })
                .unwrap_or(referrer_policy);
            let bytes = match fetch_image_bytes(
                fetch_url, &eff_base, &self.event_sink, Some(self.active_cookie_jar()), img_referrer_policy,
            ) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Lazy: пропуск {url}: {e}");
                    // BUG-630 (GAP-LOADEV срез 1): the eager pipeline fires
                    // `error` for a decode/fetch failure (`page_pipeline.rs`);
                    // this deferred path must do the same or a script awaiting
                    // `img.onerror` on a lazy image hangs forever.
                    route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                        j.fire_image_error(nid);
                    });
                    continue;
                }
            };

            // Animated GIF detection for lazy-loaded images.
            if lumen_image::is_gif(&bytes) {
                match lumen_image::decode_gif_animated(&bytes) {
                    Ok(gif) if gif.frame_count() > 1 => {
                        // BUG-272 срез 19: only the first frame is decoded eagerly here.
                        let first = match gif.frame_image(0) {
                            Ok(img) => img,
                            Err(e) => {
                                eprintln!("Lazy: не декодируется GIF {url}: {e}");
                                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                                    j.fire_image_error(nid);
                                });
                                continue;
                            }
                        };
                        if let Some(src) = self.layout_source.as_ref() {
                            let mut doc = src.document.lock().unwrap();
                            let node_id = NodeId::from_index(nid as usize);
                            if apply_intrinsic_size(&mut doc, node_id, first.width, first.height, self.relayout_viewport().unwrap_or(Size::new(0.0, 0.0))) {
                                self.note_shell_attr_writes(vec![nid]);
                            }
                        }
                        eprintln!(
                            "Lazy GIF-анимация: {} ({}×{}, {} кадров)",
                            url, gif.width, gif.height, gif.frame_count()
                        );
                        let (w, h) = (first.width, first.height);
                        if let Some(r) = self.renderer.as_mut() {
                            // BUG-272 срез 17: insert into the CPU cache first, then
                            // register the returned Arc handle — raw_images shares the
                            // cache's allocation instead of a second pixel copy.
                            let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), first);
                            if let Err(e) = r.register_image(url.clone(), handle) {
                                eprintln!("Lazy GIF: не зарегистрирована {url}: {e}");
                            }
                        } else {
                            self.pending_images.push((url.clone(), Arc::new(first)));
                        }
                        self.gif_last_frame.remove(&url);
                        self.animated_gifs.insert(url, gif);
                        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                            j.fire_image_load(nid, w, h);
                        });
                        continue;
                    }
                    Ok(gif) => {
                        match gif.frame_image(0) {
                            Ok(img) => {
                                if let Some(src) = self.layout_source.as_ref() {
                                    let mut doc = src.document.lock().unwrap();
                                    let node_id = NodeId::from_index(nid as usize);
                                    if apply_intrinsic_size(&mut doc, node_id, img.width, img.height, self.relayout_viewport().unwrap_or(Size::new(0.0, 0.0))) {
                                        self.note_shell_attr_writes(vec![nid]);
                                    }
                                }
                                eprintln!("Lazy загружена (GIF, 1 кадр): {url} ({}×{})", img.width, img.height);
                                let (w, h) = (img.width, img.height);
                                if let Some(r) = self.renderer.as_mut() {
                                    let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), img);
                                    if let Err(e) = r.register_image(url.clone(), handle) {
                                        eprintln!("Lazy: не зарегистрирована {url}: {e}");
                                    }
                                } else {
                                    self.pending_images.push((url, Arc::new(img)));
                                }
                                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                                    j.fire_image_load(nid, w, h);
                                });
                            }
                            Err(e) => {
                                eprintln!("Lazy: не декодируется GIF {url}: {e}");
                                route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                                    j.fire_image_error(nid);
                                });
                            }
                        }
                        continue;
                    }
                    Err(e) => {
                        eprintln!("Lazy: не декодируется GIF {url}: {e}");
                        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                            j.fire_image_error(nid);
                        });
                        continue;
                    }
                }
            }

            let image = match lumen_image::decode(&bytes) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("Lazy: не декодируется {url}: {e}");
                    route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                        j.fire_image_error(nid);
                    });
                    continue;
                }
            };
            eprintln!("Lazy загружена: {} ({}×{}, {:?})", url, image.width, image.height, image.format);
            // Apply intrinsic size to DOM so next relayout picks up correct dimensions.
            if let Some(src) = self.layout_source.as_ref() {
                let mut doc = src.document.lock().unwrap();
                let node_id = NodeId::from_index(nid as usize);
                if apply_intrinsic_size(&mut doc, node_id, image.width, image.height, self.relayout_viewport().unwrap_or(Size::new(0.0, 0.0))) {
                    self.note_shell_attr_writes(vec![nid]);
                }
            }
            let (w, h) = (image.width, image.height);
            if let Some(r) = self.renderer.as_mut() {
                let handle = self.image_cache.insert(lumen_image::ImageKey::new(&url), image);
                if let Err(e) = r.register_image(url.clone(), handle) {
                    eprintln!("Lazy: не зарегистрирована {url}: {e}");
                }
            } else {
                self.pending_images.push((url, Arc::new(image)));
            }
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                j.fire_image_load(nid, w, h);
            });
        }
    }

    /// CSS Scroll Snap L2 §4 — применить `scroll-initial-target: nearest` к свежей
    /// странице: прокрутить контейнеры внутри `layout_box` и перерисовать display
    /// list (`LoadedPage` собран с нулевыми смещениями). Возвращает смещение
    /// вьюпорта страницы, уже зажатое по размеру документа.
    fn apply_initial_scroll_targets(&self, page: &mut LoadedPage) -> Option<(f32, f32)> {
        let vp = self.relayout_viewport()?;
        let res = lumen_layout::apply_scroll_initial_targets(&mut page.layout_box, vp)?;
        page.display_list = crate::display_list_metrics::paint_ordered_in(&page.layout_box, vp);
        let (px, py) = res.page?;
        let max_y = (content_height_of(&page.display_list) - vp.height).max(0.0);
        let max_x = (content_width_of(&page.display_list) - vp.width).max(0.0);
        Some((px.clamp(0.0, max_x), py.clamp(0.0, max_y)))
    }

    /// Same-page fragment navigation: update `:target` CSS state and scroll to
    /// the target element. `fragment` is the id without the leading `#`; an empty
    /// string scrolls to the top and clears `:target`.
    ///
    /// Triggers a full re-layout so that `:target`-based CSS rules take effect
    /// before the scroll position is calculated.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn navigate_fragment(&mut self, fragment: String) {
        // Same-document fragment navigation must keep the JS side in sync: update
        // `location`, push a same-document history entry, and fire `hashchange`
        // (HTML LS §7.4.2 fragment-navigation). Route through the existing JS
        // `_lumen_navigate_or_fragment` path — it resolves the target, sees only
        // the fragment differs, updates `location`, queues a `HistoryUrlUpdate`
        // (drained into `nav_back` so back/forward works), and fires `hashchange`.
        let new_url = links::fragment_url(self.current_display_url(), &fragment);
        // ADR-016 M2.2c-2d (16): hashchange fire-and-forget void-dispatch через
        // `route_eval_js` — оба `_lumen_*`-вызова чистый void без синхронного чтения
        // результата следом (`location`/`hashchange` фиксируются JS-стороной, а
        // `HistoryUrlUpdate` дренится позже через `take_nav_updates`). Под флагом
        // (`LUMEN_ENGINE_THREAD=1`) уходят off-UI-thread двумя `task` в FIFO-порядке
        // (dispatch → navigate сохранён); без флага (по умолчанию) — синхронные
        // вызовы по UI-хэндлу, **байт-идентично** прежним `js.eval_js`. `escaped`
        // строится до маршрутизации; борроу `engine_thread`/`js_ctx` — раздельный.
        if self.js_present {
            let escaped = new_url.replace('\\', "\\\\").replace('\'', "\\'");
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!("_lumen_dispatch_navigate('fragment', '{escaped}', true, true)"),
            );
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!("_lumen_navigate_or_fragment('{escaped}', false)"),
            );
        }
        // STTF-1: strip a `:~:text=...` scroll-to-text directive before it
        // reaches `:target`/`find_element_by_id` — neither should ever try
        // to match the literal directive text against an `id`.
        let parsed_fragment = text_fragment::parse_fragment(&fragment);
        let id_part = parsed_fragment.element_id;
        if let Some(src) = self.layout_source.as_mut() {
            let mut doc = src.document.lock().unwrap();
            doc.set_target(id_part.as_deref());
        }
        let node_id = id_part.as_deref().and_then(|id| {
            self.layout_source
                .as_ref()
                .and_then(|src| links::find_element_by_id(&src.document.lock().unwrap(), id))
        });
        // GAP-BEFOREMATCH срез 2: HTML LS §6.1 "ancestor revealing algorithm"
        // — run BEFORE the layout below so `beforematch` handlers (which may
        // themselves mutate the DOM, per `beforematch-element-removal-*.html`)
        // settle and the subsequent relayout/rect lookup already see their
        // effect, not a stale pre-reveal tree. `eval_js` here is a void call
        // by the same `route_eval_js` contract every other fire-and-forget
        // dispatch above this line already uses — the shell never reads a
        // return value from it.
        if let Some(nid) = node_id
            && self.js_present
        {
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!("_lumen_ancestor_revealing_algorithm({})", nid.index()),
            );
        }
        // Re-layout so :target cascade AND any hidden/open flips the reveal
        // algorithm just made take effect before the rect lookup below.
        self.relayout();
        if fragment.is_empty() {
            self.scroll_to(0.0);
            return;
        }
        let target_rect = node_id.and_then(|nid| {
            self.layout_box.as_ref().and_then(|lb| forms::find_box_rect(lb, nid))
        });
        let target_y = target_rect.map(|r| r.y);
        click_log::log_fragment(&fragment, target_y.is_some());
        // BUG-338: bring the target into view within any nested scrolling
        // ancestors BEFORE the page-level scroll below — real fragment
        // navigation / `scrollIntoView()` walks the whole scrollable
        // ancestor chain, not just the page.
        if let (Some(nid), Some(rect)) = (node_id, target_rect) {
            self.scroll_nested_ancestors_into_view(nid, rect);
        }
        // STTF-1 срез 2: no id matched (or none present) but the URL carries
        // `:~:text=` directives — search the rendered text for the first one
        // that resolves and scroll to it. Per the spec, the id-fragment
        // target (if it resolved) takes priority over the text directive.
        //
        // Known remaining gap (GAP-BEFOREMATCH): a text directive can only
        // match text that layout actually produced fragments for, and a
        // `hidden=until-found` subtree gets an empty 0×0 box at BUILD time
        // (`content_visibility.rs` Phase 1 — CSS Containment L3 §4), before
        // this search ever runs. Reaching text INSIDE such a subtree (per
        // HTML LS §6.9.2 "skipped contents [...] accessible") needs layout to
        // keep the geometry of a `content-visibility: hidden` subtree around
        // for search — a separate, layout-side slice; not attempted here.
        // A node that DOES resolve below (any ordinary match, or a match
        // whose only obstacle is a closed `<details>`, not `hidden`) still
        // gets the full reveal treatment via the same call as the id path.
        let mut text_match = if target_y.is_none() {
            self.layout_box.as_ref().and_then(|lb| {
                let frags = lumen_layout::collect_visible_text(lb);
                parsed_fragment
                    .directives
                    .iter()
                    .find_map(|d| text_fragment::find_directive_match(&frags, d))
            })
        } else {
            None
        };
        if let Some(m) = &text_match
            && self.js_present
        {
            route_eval_js(
                self.engine_thread.as_ref(),
                self.js_ctx.as_ref(),
                format!("_lumen_ancestor_revealing_algorithm({})", m.node.index()),
            );
            self.relayout();
            // STTF-1 срез 3: re-run the search after the reveal-driven relayout
            // above — a `hidden=until-found` ancestor regaining its box, or a
            // `<details>` opening, can move the match's geometry, so the
            // pre-reveal rect captured before this relayout can be stale.
            text_match = self.layout_box.as_ref().and_then(|lb| {
                let frags = lumen_layout::collect_visible_text(lb);
                parsed_fragment
                    .directives
                    .iter()
                    .find_map(|d| text_fragment::find_directive_match(&frags, d))
            });
        }
        // STTF-1 срез 3: bring a text-directive match into view within any
        // nested scrolling ancestors too — same BUG-338 treatment the
        // id-fragment path above already gets, using the (possibly
        // reveal-refreshed) match's own node and bounding rect.
        if let Some(m) = &text_match {
            self.scroll_nested_ancestors_into_view(m.node, m.bounding_rect());
        }
        // STTF-1 остаток: stash the match's rects so the redraw path can
        // overlay a `::target-text` highlight (mirrors `::selection`'s
        // paint-time rect list). `None` when no text directive resolved —
        // e.g. a plain `#id` fragment or no `:~:text=` in the URL at all.
        self.target_text_highlight = text_match.as_ref().map(|m| m.rects.clone());
        // STTF-1 срез 5: resolve the page's `::target-text` rule (if any)
        // against the match's own node — same `compute_pseudo_element_style`
        // path `::selection` already uses, restricted to the `background-color`
        // the paint-time `FillRect` overlay can express (CSS Pseudo-Elements
        // L4 §5.7 restricts `::target-text` to the same property subset as
        // `::selection`: `color`/`background-color`/`text-decoration-*`/
        // `text-shadow` — the overlay, like `::selection`'s, only reads
        // `background-color`). `None` (no matching rule, or its
        // `background-color` is `currentcolor`/unset) leaves
        // `TARGET_TEXT_HIGHLIGHT_DEFAULT` as the redraw-time fallback.
        self.target_text_highlight_color = text_match.as_ref().and_then(|m| {
            let src = self.layout_source.as_ref()?;
            let doc = src.document.lock().unwrap();
            let viewport = self.relayout_viewport()?;
            let parent = lumen_layout::ComputedStyle::root();
            let style = lumen_layout::compute_target_text_style(
                &doc,
                m.node,
                &src.stylesheet,
                &parent,
                viewport,
                self.dark_mode,
            )?;
            style.background_color.map(|c| c.resolve(style.color))
        });
        let text_match_y = text_match.map(|m| m.bounding_rect().y);
        if let Some(y) = target_y.or(text_match_y) {
            // CSS Scroll Behavior L1 §3: respect scroll-behavior on the scrolling box.
            // The page viewport's scroll-behavior comes from the root (<html>) element.
            if self.page_scroll_behavior() == ScrollBehavior::Smooth {
                self.start_smooth_scroll(y);
            } else {
                self.scroll_to(y);
            }
        }
    }

    /// Returns the effective `scroll-behavior` for the page viewport (CSS Scroll Behavior L1 §3).
    /// Reads from the first non-root layout box (the `<html>` element's style).
    pub(crate) fn page_scroll_behavior(&self) -> ScrollBehavior {
        self.layout_box
            .as_ref()
            .and_then(|lb| lb.children.first())
            .map(|html_box| html_box.style.scroll_behavior)
            .unwrap_or(ScrollBehavior::Auto)
    }

    /// "Proceed anyway" on the cert interstitial (ph3-tls-hardening A6):
    /// record a session-scoped TLS-bypass override for the failed
    /// navigation's host, then retry it. No-op if the interstitial isn't
    /// currently showing anything (double-click, stale event, ...), or if
    /// the failure is not overridable (Known HSTS Host, revoked certificate —
    /// BUG-1615, `CertInterstitial::can_proceed`).
    ///
    /// `self.source` still holds the same `PageSource::Url` the failed
    /// navigation was for — `CertInterstitial::open` never touched it, only
    /// `load_failed`/`load_error_message` — so `reload()` retries the exact
    /// URL the interstitial was blocking.
    pub(crate) fn proceed_cert_interstitial(&mut self) {
        // COMPAT-TRUST-NUC: on a Russian state CA error the button is not a bypass but the
        // consent to that CA for this host; the verifier still demands CT evidence.
        if let Some(host) = self.cert_interstitial.take_nuc_consent() {
            lumen_network::tls::nuc::consent_host(&host);
            self.save_nuc_consents();
            self.relayout_chrome_host();
            self.reload();
            return;
        }
        let is_threat = self.cert_interstitial.threat.is_some();
        let Some((_url, host)) = self.cert_interstitial.proceed() else {
            return;
        };
        if is_threat {
            crate::threat_store::allow_host(&host);
        } else if !lumen_network::tls::bypass::allow_host(&host) {
            return;
        }
        self.relayout_chrome_host();
        self.reload();
    }

    /// UX-LOADING: остановить навигацию активной вкладки. Поколение
    /// увеличивается, поэтому все события фоновой загрузки (`LoadEvent`)
    /// отбрасываются сверкой `generation != load_generation`; страница
    /// остаётся такой, какой её успели нарисовать.
    pub(crate) fn stop_loading(&mut self) {
        if self.nav_start.take().is_none() {
            return;
        }
        self.load_generation = self.load_generation.wrapping_add(1);
        self.stream_builder = None;
        self.relayout_chrome_host();
        self.request_redraw();
    }

    /// Перезагрузить текущий источник: fetch/parse/layout/paint снова. На
    /// `PageSource::Empty` — no-op (грузить нечего). При ошибке — оставляем
    /// предыдущий display_list, печатаем причину в stderr.
    pub(crate) fn reload(&mut self) {
        self.restore_crashed_source();
        if matches!(self.source, PageSource::Empty) {
            return;
        }
        // Record navigation start for PerformanceNavigationTiming (Navigation Timing L2 §4.2).
        self.nav_start = Some(std::time::Instant::now());
        // BUG-640: a DOM-milestone mark from the outgoing document must never
        // leak into the incoming one's entry — same reasoning as
        // `resource_timing::clear()` below.
        nav_timing::clear();
        // A fresh navigation supersedes any prior settled error (BUG-308).
        self.load_failed = false;
        self.load_error_message = None;
        click_log::log_load_start(&self.source.describe());
        println!("Reload: {}", self.source.describe());

        // U-1: неблокирующая навигация. Когда окно уже создано (любая навигация
        // после первого кадра — клик по ссылке, адресная строка, back/forward,
        // JS location.href=), грузим через тот же асинхронный streaming-пайплайн,
        // что и первичная загрузка в `resumed`: тело фетчится в фоновом потоке,
        // окно продолжает рисовать промежуточные кадры, а тяжёлый финальный
        // pipeline (`render_bytes`) исполняется один раз на UI-потоке в
        // `LoadEvent::LoadDone`. Раньше `reload()` делал весь fetch+parse+JS+layout
        // синхронно прямо здесь — окно мёрзло на всё время навигации.
        if self.window.is_some() {
            // Сбрасываем состояние прошлого streaming-цикла — это новая страница
            // (зеркалит блок в `resumed`). `stream_builder = None` обязателен,
            // иначе chunk-и допишутся в DOM предыдущей страницы.
            self.preload_dispatched.clear();
            // BUG-839: a Resource Timing row belongs to the document that asked
            // for the load, and the loads of THIS document are about to start.
            // Clearing here rather than when the runtime appears is the whole
            // point: `set_js_ctx` runs *after* `source.load` has already
            // fetched the page's stylesheets, scripts and images, so a clear
            // there would throw away exactly the rows the page is owed.
            resource_timing::clear();
            self.stream_images_requested = Arc::default();
            self.stream_image_sizes.clear();
            self.stream_image_pixels.clear();
            self.stream_image_sizes_dirty = false;
            self.stream_image_errors.clear();
            self.stream_image_events_fired.clear();
            self.stream_sheet = lumen_css_parser::Stylesheet::default();
            self.stream_layout_seeded = false;
            self.stream_builder = None;
            self.load_generation = self.load_generation.wrapping_add(1);
            let tab_id = self.tab_strip.tabs[self.tab_strip.active].id;
            self.start_streaming_load(tab_id, self.load_generation);
            // E2E-1: поток загрузки уже получил свой клон источника вместе с
            // телом — значит POST отправлен ровно один раз. Стираем тело из
            // `self.source` здесь, а не в отдельной ветке каждого места, где
            // страницу можно перезагрузить: F5, Ctrl+R, палитра команд, back
            // и forward, восстановление сессии и клонирование вкладки читают
            // тот же `self.source`, и любое из них молча ре-постнуло бы форму.
            self.source.forget_nav_body();
            return;
        }

        // Fallback (окна ещё нет — редкий путь, напр. headless/тесты): прежняя
        // синхронная загрузка.
        // Phase 4c: попробовать загрузить через GpuSession (WinitSession)
        // для File и Url; fallback к старому пути для Snapshot
        let load_result = if let Some(page) = self.reload_via_gpu_session() {
            // WinitSession загрузка успешна
            Ok((page, None, None))
        } else {
            // Fallback к старому пути (PageSource::Snapshot, или ошибка WinitSession)
            let viewport = self.renderer.as_ref().map_or_else(
                || Size::new(1024.0, 720.0),
                |r| {
                    let s = r.viewport_size();
                    Size::new(s.width, s.height)
                },
            );
            let ls_store = self.source.origin_str().map(|o| {
                Arc::clone(self.ls_storage.entry(o).or_insert_with(|| {
                    Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
                }))
            });
            // BUG-836: same origin key, tab-scoped map — the entry outlives this
            // document, so the next one loaded here reads what this one wrote.
            let ss_store = self.source.origin_str().map(|o| {
                Arc::clone(self.ss_storage.entry(o).or_insert_with(|| {
                    Arc::new(std::sync::Mutex::new(lumen_core::WebStorage::default()))
                }))
            });
            let idb_backend = self
                .source
                .url_str()
                .and_then(|u| idb_store_for_url(u, self.idb_dir.as_deref()));
            let sw_backend = self.source.origin_str().map(|o| {
                Arc::new(lumen_storage::SwStore::new(Arc::clone(&self.sw_backend), o))
                    as Arc<dyn lumen_core::ext::SwBackend>
            });
            self.source.load(self.event_sink.clone(), viewport, ls_store, ss_store, idb_backend, sw_backend, &*self.hyp_provider, self.cookie_banner_dismiss)
        };
        // E2E-1: см. streaming-ветку выше — тело одноразовое, запрос уже ушёл.
        self.source.forget_nav_body();

        match load_result {
            Ok((page, new_layout_source, new_js_ctx)) => {
                // Drop JS closures before layout_source to release Arc<Mutex<Document>>
                // clones held inside QuickJS closures before LayoutSource's Arc drops.
                self.set_js_ctx(None);
                self.layout_source = new_layout_source;
                self.set_js_ctx(new_js_ctx);
                // ADR-016 M2.2c-2b: зеркалим новый хэндл + DOM в движковый поток.
                self.sync_engine_js_state();
                // The new runtime starts empty; re-seed it with the current Navigation state.
                self.commit_nav_state();
                (self.content_height, self.content_width) =
                    crate::display_list_metrics::page_content_extent(&page.display_list, &page.layout_box, self.viewport_width_css());
                // On full page load, mark all tiles dirty — content has changed completely.
                self.tile_grid.mark_all_dirty(self.content_width, self.content_height);
                // BUG-480 срез 14: под-документы новой страницы заменяют старые
                // (то же, что делает `apply_loaded_page`, — этот резервный путь
                // reload'а без окна о фреймах не знал вовсе и оставлял хэндлы
                // ПРЕДЫДУЩЕГО документа живыми). Строго до записи списка: она
                // вклеивает в него содержимое фреймов.
                frame_dynamic_load::release_frame_handles(std::mem::replace(&mut self.frames, page.frames));
                self.frame_env = page.frame_env;
                // FRAME-4 срез 3: см. тот же сброс в `apply_loaded_page`.
                frames::clear_frame_nav_requests(&mut self.frame_nav_requests);
                // FRAME-8: см. тот же сброс в `apply_loaded_page`.
                self.pending_new_frames.clear();
                self.pending_frame_load_dispatch.clear();
                // BUG-480 срез 16: индекс в этом списке — единственное, чем
                // адресован фрейм под курсором, поэтому пережить его замену он
                // не может: указывал бы на чужой хэндл.
                self.hovered_frame = None;
                self.set_display_list(page.display_list);
                // P3-viewtransnav срез 4: reveal a pending cross-document view
                // transition now that this document's stylesheet exists.
                self.maybe_reveal_mpa_view_transition();
                self.animation_scheduler.clear();
                self.transition_scheduler = TransitionScheduler::new();
                self.starting_style_tracker = StartingStyleTracker::new();
                self.prev_styles.clear();
                collect_box_styles(&page.layout_box, &mut self.prev_styles);
                // BUG-341 S7: this producer bypasses the restyle-aware path
                // entirely — invalidate so the next `try_relayout_raf_incremental`
                // doesn't diff a stale cache against this fresh tree.
                self.page_prev_cascade_styles = None;
                self.m4_full_cost_ms = None;
                self.layout_box = Some(page.layout_box);
                // content-visibility: auto (BB-4): новая страница — ratchet с нуля.
                self.cv_relevant.clear();
                self.cv_events.clear();
                self.cv_skipped.clear();
                self.cv_auto_state.clear();
                self.prev_layout_shift_rects.clear();
                self.refresh_cv_state();
                // GAP-CSSANIM срез 1: stale transitions from the previous
                // document must not surface as events on the new one.
                self.transition_events.clear();
                // GAP-CSSANIM срез 2: same for stale CSS Animations events.
                self.animation_events.clear();
                self.update_snap_containers();
        self.update_scroll_containers();
                // Push initial layout geometry so JS can query bounding rects
                // immediately after page load (before the first relayout).
                // ADR-016 M2.2c-2d: routed off-thread like the relayout push above —
                // the three owned-arg void calls go through `route_task_js`; the
                // `self.js_present` gate keeps the geometry collection JS-gated
                // (byte-identical with the flag off).
                #[cfg(feature = "v8")]
                if self.js_present
                    && let Some(lb_ref) = self.layout_box.as_ref()
                    && let Some(src) = self.layout_source.as_ref()
                    && let Ok(doc_guard) = src.document.lock()
                {
                    let viewport = self.renderer.as_ref().map_or_else(
                        || Size::new(1024.0, 720.0),
                        |r| {
                            let s = r.viewport_size();
                            Size::new(s.width, s.height)
                        },
                    );
                    let rects = collect_layout_rects(lb_ref, &doc_guard);
                    // GAP-LAYOUTSHIFT: seed the CLS diff baseline from this
                    // reload's first rendered frame — `apply_relayout_result`
                    // only diffs against a baseline it set itself, so without
                    // this the first post-reload relayout would compare
                    // against an empty snapshot and score zero every time.
                    // срез 5: baseline is the shift-specific geometry (no
                    // own-node transform, no `visibility: hidden` nodes), not
                    // the gBCR `rects` pushed to JS below — see
                    // `collect_layout_shift_rects`'s doc-comment.
                    self.prev_layout_shift_rects = lumen_layout::collect_layout_shift_rects(lb_ref);
                    let client_rects = collect_client_rects(lb_ref, &doc_guard);
                    let hit_test_tree = Arc::new(lb_ref.clone());
                    let styles = collect_computed_styles(lb_ref, &doc_guard, None, viewport);
                    let pseudo_styles = collect_pseudo_computed_styles(lb_ref);
                    drop(doc_guard);
                    let customs = collect_custom_properties(lb_ref, viewport);
                    let (vw, vh) = (viewport.width, viewport.height);
                    // CSSOM-7 (BUG-977): first stylesheet push for this
                    // document — even a top-level `<script>` running at
                    // initial parse time gets a same-tick flush target,
                    // mirroring `update_stylesheet`'s own doc-comment.
                    let stylesheet = Arc::clone(&src.stylesheet);
                    route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                        js.update_layout_rects(rects);
                        js.update_client_rects(client_rects);
                        js.update_hit_test_tree(hit_test_tree);
                        js.update_computed_styles(styles);
                        js.update_pseudo_computed_styles(pseudo_styles);
                        js.update_custom_properties(customs);
                        js.update_stylesheet(stylesheet);
                        js.update_viewport_size(vw, vh);
                    });
                }
                self.title = page.title;
                if let Some(t) = &self.title {
                    self.tab_strip.set_active_title(t.as_str());
                }
                // ph3-tls-hardening, live-wiring slice: same as `apply_loaded_page`.
                self.cert_info = page.cert_info.map(crate::panels::cert_panel::PanelCertData::from);
                self.anim_frame = None;
                // Display list другой → совпадения пересчитаются; панель с набранным
                // запросом остаётся (BUG-1623), пустая закрывается.
                self.find.close_if_untouched();
                self.address_bar.close_if_untouched();
                // Новая страница — показываем сверху-слева (либо восстанавливаем
                // offset из back/forward, как в `apply_loaded_page`).
                let (rx, ry) = self.pending_restore_scroll.take().unwrap_or((0.0, 0.0));
                self.scroll_x = rx;
                self.scroll_y = ry;
                self.issue_scroll_command();
                // Любой активный drag прерывается (content_height другой,
                // thumb-геометрия пересчитана с нуля).
                self.scroll_drag = None;
                self.hscroll_drag = None;
                self.frame_scroll_drag = None;
                // Активные анимации старой страницы сбрасываем.
                self.scroll_anim = None;
                self.momentum_anim = None;
                self.forward_momentum_stop();
                self.touchpad_vel = (0.0, 0.0);
                // Reset CPU image cache for the reloaded page (10E.4 scroll-discard).
                self.image_cache.clear();
                if let Some(r) = self.renderer.as_mut() {
                    // Старая GPU-cache картинок относится к предыдущей странице
                    // (даже если src совпадает, content мог измениться). Чистим
                    // и регистрируем заново.
                    r.clear_images();
                    for (src, image) in &page.images {
                        // BUG-272 срез 17: `image` — Arc из IMAGE_CACHE; register
                        // клонирует указатель, raw_images разделяет аллокацию.
                        if let Err(err) = r.register_image(src.clone(), Arc::clone(image)) {
                            eprintln!("Картинка {src} не зарегистрирована: {err}");
                        }
                        self.image_cache.insert(lumen_image::ImageKey::new(src), (**image).clone());
                    }
                } else {
                    // Renderer ещё не создан — обычно невозможно (reload идёт
                    // по клавише, окно уже есть), но защитимся: складываем в
                    // pending_images, resumed подхватит.
                    self.pending_images = page.images;
                }
                if let Some(w) = self.window.as_ref() {
                    w.set_title(&window_title(self.title.as_deref()));
                    w.request_redraw();
                }
                // JS may have requested navigation via location.href= etc.
                // Store it for processing in about_to_wait (after first render).
                self.pending_js_navigate = page.js_navigate;
                let title = self.title.as_deref().unwrap_or("");
                // Deliver W3C Navigation Timing L2 entry (§4.2) to JS PerformanceObservers.
                // ADR-016 M2.2c-2d (20): last direct `self.js_ctx` read on the reload
                // nav-timing path → `self.js_present` gate + `route_task_js`. `nav_start`
                // is still taken unconditionally (as the old tuple did), so on the «no JS»
                // / «no start» / «no url» branch it is cleared exactly as before. Delivery
                // is a fire-and-forget void: under the flag (`LUMEN_ENGINE_THREAD=1`) it
                // goes off-UI-thread; flag-off (default) stays byte-identical to the old
                // direct `js.deliver_nav_timing(...)`.
                #[cfg(feature = "v8")]
                {
                    let nav_start = self.nav_start.take();
                    if let (true, Some(start), Some(url)) =
                        (self.js_present, nav_start, self.source.url_str().map(str::to_owned))
                    {
                        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
                        let detail_json = nav_timing::detail_json(page.nav, start);
                        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                            j.deliver_nav_timing(&url, duration_ms, &detail_json);
                        });
                    }
                }
                click_log::log_load_ok(&self.source.describe(), title);
                click_log::log_page_ready(&self.source.describe(), self.scroll_y);
                self.record_render_health();
                self.frustration_page_installed();
            }
            Err(err) => {
                self.nav_start = None;
                click_log::log_load_err(&self.source.describe(), &err.to_string());
                health_log::log_load_error(&self.source.describe(), &err.to_string());
                eprintln!("Ошибка reload {}: {err}", self.source.describe());
                // P3-viewtransnav срез 5: same fallback as the streaming
                // error arms in `user_event.rs` — this synchronous path
                // never reaches `maybe_reveal_mpa_view_transition`, so a
                // captured snapshot must not linger for a later reload.
                self.pending_mpa_view_transition_snapshot = None;
            }
        }
    }

    /// Попытаться загрузить страницу через GpuSession (WinitSession).
    /// Возвращает LoadedPage если успешно, иначе None (fallback к старому пути).
    ///
    /// Phase 4c: использует WinitSession::render_to_gpu() вместо inline pipeline
    /// для PageSource::File и PageSource::Url.
    pub(crate) fn reload_via_gpu_session(&mut self) -> Option<LoadedPage> {
        use lumen_driver::{WinitSession, GpuSession};

        // E2E-1: `WinitSession::navigate` умеет только GET — навигацию с телом
        // отдаём резервному пути (`PageSource::load`), который тело отправит.
        if self.source.nav_body().is_some() {
            return None;
        }
        // Преобразовать PageSource в URL для WinitSession
        let url = match &self.source {
            PageSource::File(path) => path_to_file_url(path),
            PageSource::Url { url, .. } => url.clone(),
            _ => return None, // Snapshot и Empty обработаны отдельно
        };

        let viewport = self.renderer.as_ref().map_or_else(
            || Size::new(1024.0, 720.0),
            |r| {
                let s = r.viewport_size();
                Size::new(s.width, s.height)
            },
        );

        // Создать сессию с нужным viewport
        let mut session = WinitSession::with_viewport(viewport.width, viewport.height);

        // Загрузить страницу через WinitSession
        if session.navigate(&url).is_err() {
            return None;
        }

        // Получить RenderedPage через render_to_gpu()
        let rendered = match session.render_to_gpu() {
            Ok(r) => r,
            Err(_) => return None,
        };

        // Преобразовать RenderedPage в LoadedPage
        // Преобразовать lumen_driver::JsNavigateRequest в shell::JsNavigateRequest
        let js_navigate = rendered.js_navigate.map(|nav| {
            if nav.replace {
                JsNavigateRequest::Replace(nav.url)
            } else {
                JsNavigateRequest::Push(nav.url)
            }
        });

        Some(LoadedPage {
            display_list: rendered.display_list,
            title: rendered.title,
            // BUG-272 срез 17: driver's `RenderedPage.images` is still owned
            // `Image`; wrap at this boundary (driver path unchanged).
            images: rendered.images.into_iter().map(|(s, i)| (s, Arc::new(i))).collect(),
            animated_gifs: Vec::new(), // lumen-driver path has no animated GIF support yet
            lazy_pairs: Vec::new(), // Phase 4c: TODO integrate lazy loading
            layout_box: rendered.layout_box,
            // Driver path renders via lumen-driver which uses Arc<dyn FontProvider>;
            // shell's LoadedPage now requires Arc<FontRegistry> for PH3-19 dynamic
            // registration. Use an empty registry — driver pages have no async fonts.
            font_registry: Arc::new(lumen_font::FontRegistry::new()),
            pending_web_fonts: Vec::new(),
            js_navigate,
            page_tracks: tracks::PageTracks::default(),
            // lumen-driver рендерит через свой headless-пайплайн без
            // sub-документов — фреймов на этом пути нет.
            frames: Vec::new(),
            frame_env: None,
            // BUG-640: `lumen_driver::RenderedPage` carries no response
            // status/redirect signal at all — this is the rare no-window
            // GpuSession path (headless/tests), default to "unknown".
            nav: crate::nav_timing::NavResponseMeta::default(),
            // lumen-driver path has no TLS handshake plumbing to this boundary.
            cert_info: None,
            response_policies: Default::default(),
            // lumen-driver's headless pipeline has no parse-time snapshot pass.
            prescript_layout_rects: None,
        })
    }

    /// Запустить background-поток загрузки текущего `source`.
    ///
    /// Поток fetches байты, затем:
    ///
    /// 1. Для каждого STREAM_CHUNK_BYTES-chunk: прогоняет через `PreloadScanner`
    ///    (PH1-8, HTML LS §13.2.6.4.7), отправляет `EarlyPreloadHints`, затем
    ///    `HtmlChunk`. Hint-ы эмитятся из **каждого** chunk-а, не только первого —
    ///    это даёт реальный выигрыш для stylesheet/шрифтов, стоящих за первыми 8 КБ.
    /// 2. PH1-2: для каждого `Stylesheet`-hint запускает параллельный CSS-загрузчик,
    ///    который присылает `CssLoaded` ещё до `LoadDone`.
    /// 3. По завершении — `LoadDone(raw)` для финального pipeline.
    ///
    /// При ошибке — `LoadError`.
    ///
    /// `generation` (U-1) метит каждое испускаемое событие; `user_event`
    /// отбрасывает события устаревшего поколения, если навигацию успели сменить.
    /// `tab_id` (BUG-1214) — id вкладки, чья навигация это, снятый в момент
    /// запуска: события маршрутизируются к ЭТОЙ вкладке (`self`, если она всё
    /// ещё активна, иначе `bg_tabs[tab_id]`) независимо от того, какая вкладка
    /// активна к моменту прихода события — см. [`Lumen::apply_load_event_for_tab`].
    pub(crate) fn start_streaming_load(&self, tab_id: usize, generation: u64) {
        // Ctrl+F5: фиксируем режим кэша этой навигации до того, как стартуют
        // клиенты документа и подресурсов.
        crate::config::begin_navigation_cache_mode();
        if matches!(self.source, PageSource::Empty | PageSource::AboutBlank) {
            return;
        }
        // PERF-15: close the outgoing page's recording while its
        // `PREFETCH_CACHE` slots still exist — the commit asks them which
        // replayed resources the page really read.
        crate::site_memory::finish_visit();
        // BUG-171: scope the subresource prefetch cache to this navigation. Runs on
        // the UI thread before the streaming thread is spawned, so producer warm-ups
        // and the UI-thread consumer all observe `generation`.
        crate::prefetch::PREFETCH_CACHE.reset(generation);
        // BUG-172: scope the decoded-image cache to this navigation too, so the
        // streaming progressive loader and the final pipeline share one decode per
        // image and a superseded navigation's images are dropped.
        crate::image_cache::IMAGE_CACHE.reset(generation);
        let source = self.source.clone();
        let sink = Arc::clone(&self.event_sink);
        let proxy = self.load_proxy.clone();
        let cookie_jar = self.active_cookie_jar();
        // PERF-15: what this page used on its previous visit starts now,
        // together with the document request, each into its own reserved
        // `PREFETCH_CACHE` slot. A POST navigation is not a repeat visit.
        if let PageSource::Url { url, body: None, .. } = &source {
            let remembered = crate::site_memory::begin_visit(url, generation);
            if !remembered.is_empty() {
                eprintln!("site-memory: {} ресурсов прошлого визита запрошены заранее", remembered.len());
            }
            crate::site_memory::replay(
                remembered,
                generation,
                &ResourceBase::Url(url.clone()),
                &sink,
                &cookie_jar,
            );
        }
        // BUG-268: media-контекст экрана — для гейта speculative-фетча
        // `<link rel=stylesheet media=...>`, чтобы print-only лист не грел
        // кэш и не слал CssLoaded (progressive-кадры не красятся print-стилями).
        let media_ctx = {
            let viewport = self.renderer.as_ref().map_or_else(
                || Size::new(1024.0, 720.0),
                |r| {
                    let s = r.viewport_size();
                    Size::new(s.width, s.height)
                },
            );
            screen_media_context(viewport, self.dark_mode)
        };

        // Имя обязательно (BUG-1027): при аварии рантайм печатает только его,
        // а безымянные потоки дают бесполезное `thread '<unknown>'`. Стек
        // штатный: поток читает сокет и гоняет preload-сканер, деревом не
        // спускается — layout приходящих чанков делает UI-поток.
        let spawned = std::thread::Builder::new()
            .name("lumen-stream".to_owned())
            .spawn(move || {
            // PH1-8: инкрементальный preload-сканер — обрабатывает каждый chunk.
            // Hint-ы отправляются ДО соответствующего HtmlChunk, чтобы fetch
            // начался параллельно с DOM-парсингом (spec §13.2.6.4.7).
            let mut preload_scanner = lumen_html_parser::PreloadScanner::new();

            // PH1-2a: для URL-источников тело стримится прямо с сокета —
            // порции прилетают в `on_chunk` по мере чтения, не дожидаясь полной
            // загрузки. Для File/Snapshot/Static тело уже в памяти, поэтому его
            // достаточно нарезать на STREAM_CHUNK_BYTES (прежнее поведение).
            let raw = if let PageSource::Url { url, .. } = &source {
                // UX-SECURITY-UI: адрес из базы Safe Browsing не загружаем.
                if let Some((list, threat)) = crate::threat_store::check(url) {
                    let host = lumen_core::url::Url::parse(url)
                        .map_or_else(|_| url.clone(), |u| u.host().to_owned());
                    let _ = proxy.send_event(LoadEvent::ThreatBlocked(
                        url.clone(),
                        host,
                        list,
                        threat,
                        tab_id,
                        generation,
                    ));
                    return;
                }
                // BUG-757: база preload-хинтов — адрес, с которого РЕАЛЬНО
                // течёт тело (его приносит сам chunk), а не запрошенный: после
                // редиректа они разные, и относительный `src` уходил на
                // до-редиректный путь ещё до того, как документ получал
                // правильную базу. Пересобираем строку только при смене hop-а.
                let mut base = ResourceBase::Url(url.clone());
                let chunk_proxy = proxy.clone();
                // Separate clones for prefetch warm-up: `cookie_jar`/`sink` below are
                // moved into the streaming call, so the per-chunk closure keeps its own.
                let cj_prefetch = Some(Arc::clone(&cookie_jar));
                let sink_prefetch = Arc::clone(&sink);
                // BUG-1185: CSP заголовка ответа, чьё тело течёт, — разбирается
                // один раз на hop (у каждого ответа цепочки свои заголовки).
                let mut header_csp: Option<(String, Vec<lumen_network::csp::CspPolicy>)> = None;
                let mut on_chunk = |chunk: &[u8], hop_url: &lumen_core::url::Url, headers: &[(String, String)]| {
                    if !matches!(&base, ResourceBase::Url(u) if u == hop_url.as_str()) {
                        base = ResourceBase::Url(hop_url.to_string());
                        // UI-поток резолвит картинки/шрифты частичного DOM от
                        // своей копии базы — сообщаем ему новую (BUG-757).
                        let _ = chunk_proxy
                            .send_event(LoadEvent::DocumentBase(base.clone(), tab_id, generation));
                    }
                    if !header_csp.as_ref().is_some_and(|(hop, _)| hop == hop_url.as_str()) {
                        let policies = crate::page_source::content_security_policy_header(headers)
                            .iter()
                            .map(|p| lumen_network::csp::parse_csp_header(p))
                            .collect();
                        header_csp = Some((hop_url.to_string(), policies));
                    }
                    feed_preload_and_emit(
                        &mut preload_scanner,
                        chunk,
                        &base,
                        header_csp.as_ref().map_or(&[][..], |(_, p)| p),
                        &chunk_proxy,
                        tab_id,
                        generation,
                        &sink_prefetch,
                        cj_prefetch.as_ref(),
                        &media_ctx,
                    );
                    let _ = chunk_proxy.send_event(LoadEvent::HtmlChunk(chunk.to_vec(), tab_id, generation));
                };
                match source.load_bytes_streaming(Arc::clone(&sink), Some(cookie_jar), &mut on_chunk) {
                    Ok(r) => r,
                    Err(e) => {
                        // A6: a TLS cert failure routes to the interstitial
                        // instead of the generic `LoadError` — everything
                        // else (DNS, connect-refused, read timeout, ...)
                        // keeps the existing string-message path.
                        let _ = match e.downcast_ref::<lumen_core::error::Error>() {
                            Some(lumen_core::error::Error::CertInvalid(cert_err)) => {
                                let host = lumen_core::url::Url::parse(url)
                                    .map_or_else(|_| url.clone(), |u| u.host().to_owned());
                                // COMPAT-TRUST-CAUSES: the probe and the chain lookup
                                // block, so they run here, not on the UI thread.
                                let cause = crate::cert_cause::diagnose(&host, cert_err);
                                proxy.send_event(LoadEvent::CertError(
                                    url.clone(),
                                    host,
                                    cert_err.clone(),
                                    cause,
                                    tab_id,
                                    generation,
                                ))
                            }
                            _ => proxy.send_event(LoadEvent::LoadError(
                                e.to_string(),
                                // COMPAT-ERR-01: DNS/тишина/«нет маршрута» + ни один
                                // контрольный адрес не ответил → «нет интернета».
                                crate::net_probe::refine(lumen_core::net_error::NetErrorCode::of_error(&*e), url),
                                tab_id,
                                generation,
                            )),
                        };
                        return;
                    }
                }
            } else {
                let cj_prefetch = Some(Arc::clone(&cookie_jar));
                let raw = match source.load_bytes(Arc::clone(&sink), Some(cookie_jar)) {
                    Ok(r) => r,
                    Err(e) => {
                        let code = crate::net_probe::refine(
                            lumen_core::net_error::NetErrorCode::of_error(&*e),
                            source.url_str().unwrap_or(""),
                        );
                        let _ = proxy.send_event(LoadEvent::LoadError(e.to_string(), code, tab_id, generation));
                        return;
                    }
                };
                let mut pos = 0;
                while pos < raw.bytes.len() {
                    let end = (pos + STREAM_CHUNK_BYTES).min(raw.bytes.len());
                    let chunk = &raw.bytes[pos..end];
                    feed_preload_and_emit(
                        &mut preload_scanner,
                        chunk,
                        &raw.base,
                        &[],
                        &proxy,
                        tab_id,
                        generation,
                        &sink,
                        cj_prefetch.as_ref(),
                        &media_ctx,
                    );
                    if proxy.send_event(LoadEvent::HtmlChunk(chunk.to_vec(), tab_id, generation)).is_err() {
                        return; // event loop завершён
                    }
                    pos = end;
                }
                raw
            };

            // COMPAT-ERRORS: 5xx без тела — показывать нечего, кроме самого
            // статуса; страницу ошибки с автоповтором показывает `show_net_error`.
            if crate::net_error_page::is_empty_server_error(raw.status, &raw.bytes) {
                let msg = lumen_core::net_error::http_status_message(raw.status);
                let code = lumen_core::net_error::NetErrorCode::HttpStatus(raw.status);
                let _ = proxy.send_event(LoadEvent::LoadError(msg, code, tab_id, generation));
                return;
            }

            // Финальные hint-ы из буферизованного хвоста сканера.
            let tail = preload_scanner.end();
            if !tail.is_empty() {
                let _ = proxy.send_event(LoadEvent::EarlyPreloadHints(tail, raw.base.clone(), tab_id, generation));
            }

            let _ = proxy.send_event(LoadEvent::LoadDone(Box::new(raw), tab_id, generation));
        });
        if let Err(err) = spawned {
            eprintln!("не удалось запустить поток стриминговой загрузки: {err}");
        }
    }

    /// Обновить display list на основе снапшота частичного DOM.
    /// Применяет `stream_sheet` — CSS, загруженный параллельными потоками (PH1-2).
    pub(crate) fn paint_partial_dom(&mut self, doc: &lumen_dom::Document) {
        let Some(renderer) = self.renderer.as_ref() else { return };
        let vp_size = renderer.viewport_size();
        let viewport = Size::new(vp_size.width, vp_size.height);

        let font = match lumen_font::Font::parse(INTER_FONT) {
            Ok(f) => f,
            Err(_) => return,
        };
        let measurer = match lumen_paint::FontMeasurer::new(&font) {
            Ok(m) => m,
            Err(_) => return,
        };

        // PH1-2b: после первого («засевающего») кадра релейаутим инкрементально —
        // переиспользуем геометрию неизменённого префикса из прошлого кадра,
        // релейаутим только новые/изменённые поддеревья. Полный layout всего
        // частичного DOM на каждый 16-мс тик тормозил большие страницы.
        let null_hp = lumen_core::ext::NullHyphenationProvider;
        let layout = match (self.stream_layout_seeded, self.layout_box.as_ref()) {
            (true, Some(prev)) => lumen_layout::layout_streaming_incremental(
                doc, &self.stream_sheet, viewport, &measurer, &null_hp, false, prev,
            ),
            _ => lumen_layout::layout_measured(doc, &self.stream_sheet, viewport, &measurer),
        };
        let dl = crate::display_list_metrics::paint_ordered_in(&layout, viewport);

        (self.content_height, self.content_width) = crate::display_list_metrics::page_content_extent(&dl, &layout, self.viewport_width_css());
        self.set_display_list(dl);
        // BUG-341 S7: streaming layout is a separate incremental mechanism
        // (`layout_streaming_incremental`, no `CounterMap`) — invalidate the
        // restyle cache so it isn't diffed against this tree by mistake.
        self.page_prev_cascade_styles = None;
        self.layout_box = Some(layout);
        self.stream_layout_seeded = true;
        self.update_snap_containers();
        self.update_scroll_containers();

        // PH1-2c: запустить параллельную загрузку картинок, появившихся в этом
        // частичном DOM, чтобы они дорисовывались по мере прихода (как CSS),
        // а не разом в финальном `LoadDone`.
        self.spawn_stream_image_loads(doc, viewport);

        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// PH1-2c: найти `<img>` в частичном streaming-DOM и запустить параллельные
    /// потоки fetch+decode для ещё-не-запрошенных картинок. По завершении поток
    /// шлёт `LoadEvent::ImageDecoded`, и `user_event` регистрирует картинку в
    /// renderer-е + просит redraw. Дедуп через `stream_images_requested`, так что
    /// каждый `src` грузится один раз за навигацию; `loading="lazy"` пропускается
    /// (грузится по близости к viewport уже после `LoadDone`).
    pub(crate) fn spawn_stream_image_loads(&mut self, doc: &lumen_dom::Document, viewport: Size) {
        let requests = lumen_layout::collect_image_requests(doc, viewport);
        // GAP-CSPENF срез 4: same one-shot policy read `fetch_and_decode_images`
        // uses — computed here too because this producer runs earlier (during
        // streaming) and on its own images, not the eager pipeline's.
        let csp_gate = {
            let root = doc.root();
            crate::csp_enforce::document_csp_policy(doc, root)
        };
        self.spawn_image_requests(requests, csp_gate);
    }

    /// BUG-730: same pass over the **live** document, run after every relayout
    /// that applied a DOM mutation.
    ///
    /// [`Self::spawn_stream_image_loads`] only ever sees the partial DOM the
    /// HTML streamer hands it, i.e. markup that arrived over the wire. An
    /// `<img>` that a script appends (or an existing one whose `src` a script
    /// rewrites) after the load finished was therefore never fetched at all —
    /// no request, no decode, and paint drew the grey placeholder box forever.
    /// That is the normal way a client-rendered page shows pictures: on
    /// `tbank.ru` not one of its 33 images was ever requested.
    ///
    /// Dedup still goes through `stream_images_requested`, so an image already
    /// fetched during streaming is not re-fetched, and a URL is requested once
    /// per navigation no matter how many relayouts see it.
    pub(crate) fn spawn_dynamic_image_loads(&mut self, viewport: Size) {
        let (requests, csp_gate) = {
            let Some(src) = self.layout_source.as_ref() else { return };
            let Ok(doc) = src.document.lock() else { return };
            let requests = lumen_layout::collect_image_requests(&doc, viewport);
            // GAP-CSPENF срез 4: same policy read as the streaming producer —
            // a script-inserted `<img>` is just as subject to `img-src` as a
            // parser-inserted one.
            let root = doc.root();
            let csp_gate = crate::csp_enforce::document_csp_policy(&doc, root);
            (requests, csp_gate)
        };
        self.spawn_image_requests(requests, csp_gate);
    }

    /// BUG-939: same argument as BUG-730 above, for `background-image` instead
    /// of `<img>`.
    ///
    /// `fetch_and_decode_background_images` (`page_pipeline.rs`) only ever runs
    /// once, against the layout tree the initial pipeline pass built. A
    /// `background-image` set or changed from JS (`el.style.backgroundImage =
    /// "url(...)"`) is never re-collected afterwards — the cascade sees the
    /// mutation (`getComputedStyle` reflects the new value) but no fetch is
    /// ever queued, so the image never appears. Hooked into the same
    /// post-relayout point as [`Self::spawn_dynamic_image_loads`] so every
    /// relayout producer picks it up, whichever one relaid the page out.
    ///
    /// `collect_background_image_requests` returns bare URLs (backgrounds have
    /// no DOM node to anchor intrinsic-size hints on), so each is wrapped into
    /// an [`lumen_layout::ImageRequest`] with a sentinel `node_id` — the same
    /// "no DOM origin" sentinel `collect_bg_image_inner` already uses for
    /// `content: url(...)` segments — purely to reuse
    /// [`Self::spawn_image_requests`]'s fetch/decode/dedup plumbing; that
    /// function never reads `node_id`; only `apply_stream_intrinsic_sizes`
    /// does, and it walks `<img>` requests exclusively, so this sentinel value
    /// on background entries is never consulted downstream.
    pub(crate) fn spawn_dynamic_background_image_loads(&mut self) {
        let (requests, csp_gate) = {
            let Some(layout) = self.layout_box.as_ref() else { return };
            let urls = lumen_layout::collect_background_image_requests(layout, 1.0);
            let Some(src) = self.layout_source.as_ref() else { return };
            let Ok(doc) = src.document.lock() else { return };
            let root = doc.root();
            let csp_gate = crate::csp_enforce::document_csp_policy(&doc, root);
            let requests = urls
                .into_iter()
                .map(|url| lumen_layout::ImageRequest {
                    node_id: lumen_dom::NodeId::from_index(0),
                    url,
                    has_explicit_width: false,
                    has_explicit_height: false,
                    is_lazy: false,
                    fetch_priority: None,
                    crossorigin: None,
                    referrer_policy_attr: None,
                    embedded_content: false,
                })
                .collect();
            (requests, csp_gate)
        };
        self.spawn_image_requests(requests, csp_gate);
    }

    /// BUG-935 срез 94: сообщить трекеру мутаций страницы, что оболочка дописала
    /// `width`/`height` узлам `nids` (`apply_intrinsic_size`) — иначе флаш движкового
    /// потока не знает о записи и оставляет узлам стиль, посчитанный без неё.
    /// Задача встаёт в очередь движка после записи в DOM и раньше следующего флаша.
    pub(crate) fn note_shell_attr_writes(&self, nids: Vec<u32>) {
        if nids.is_empty() {
            return;
        }
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.note_shell_attr_writes(&nids);
        });
    }

    /// BUG-735: разнести intrinsic-размеры уже декодированных картинок по `<img>`
    /// живого документа и, если DOM от этого изменился, запросить релейаут.
    ///
    /// Третий путь загрузки картинок — streaming/динамический
    /// ([`Self::spawn_image_requests`]) — регистрировал пиксели в рендерере, но
    /// никогда не сообщал размер DOM-у: `apply_intrinsic_size` звали только
    /// финальный `fetch_and_decode_images` и lazy-путь. Для клиентски
    /// отрисованной страницы (где к моменту появления `<img>` финальный pipeline
    /// уже отработал — BUG-730) это значит **все** картинки: без intrinsic-пары
    /// нет и соотношения сторон, поэтому `height: auto` даёт ноль.
    ///
    /// Проход коалесцирован: арм `ImageDecoded` только копит размеры и взводит
    /// флаг, а разнос+релейаут делается один раз за кадр — сотня декодов стоит
    /// одного релейаута, а не сотни. Сходимость держится на том, что
    /// `apply_intrinsic_size` возвращает `false`, когда дописывать нечего:
    /// второй проход по тем же узлам DOM не меняет и релейаут не заказывает.
    /// Петли «релейаут → новый запрос → новый декод» нет — `spawn_image_requests`
    /// дедуплицируется через `stream_images_requested`.
    ///
    /// BUG-1048: тем же проходом теперь диспатчит `load`/`error` на `<img>`,
    /// который эту intrinsic-пару (или отказ) ждёт — до этого события у
    /// streaming/динамического пути не было вовсе, поэтому скриптовый `<img>`
    /// (BUG-730 «streaming/dynamic») фетчился и рисовался, но `img.complete`
    /// оставался `false` навсегда. `stream_image_events_fired` дедуплицирует по
    /// `(node, url)`: несколько `<img>` могут делить один URL, и сам проход
    /// перезапускается на каждый новый декод, так что без него один и тот же
    /// узел получил бы `load` повторно.
    pub(crate) fn apply_stream_intrinsic_sizes(&mut self) {
        if !self.stream_image_sizes_dirty {
            return;
        }
        self.stream_image_sizes_dirty = false;
        let Some(viewport) = self.relayout_viewport() else {
            // Вьюпорта ещё нет (рендерер не сконфигурирован) — размеры остаются
            // в карте, проход повторится на кадре, когда он появится.
            self.stream_image_sizes_dirty = true;
            return;
        };
        // (node index, размер) — `None` значит decode-неудачу (`fire_image_error`).
        let mut fires: Vec<(u32, Option<(u32, u32)>)> = Vec::new();
        // BUG-938: (node index, decoded pixels) — впервые увиденные пары
        // `(nid, url)` этого прохода, чью картинку canvas `drawImage`/
        // `createImageBitmap` до сих пор не видел (регистрация была только
        // одноразовым проходом по снапшоту разбора, см. `page_pipeline.rs`).
        let mut bitmap_regs: Vec<(u32, Arc<lumen_image::Image>)> = Vec::new();
        // BUG-935 срез 94: узлы, которым этот проход дописал `width`/`height`.
        let mut written: Vec<u32> = Vec::new();
        let changed = {
            let Some(src) = self.layout_source.as_ref() else { return };
            let Ok(mut doc) = src.document.lock() else { return };
            // Тот же picker, что эмитит ключи `src` в `DrawImage`, — url из
            // запроса совпадает с ключом карты по построению.
            let requests = lumen_layout::collect_image_requests(&doc, viewport);
            let walked: std::collections::HashSet<u32> =
                requests.iter().map(|r| r.node_id.raw()).collect();
            let mut changed = false;
            for req in requests {
                if req.is_lazy {
                    continue;
                }
                let nid = req.node_id.raw();
                // OBJECT-1: `<object>`/`<embed>` получают размер, но не
                // image-события — `load`/`error` им шлёт JS-шим.
                if req.embedded_content {
                    if let Some(&(w, h)) = self.stream_image_sizes.get(&req.url)
                        && apply_intrinsic_size(&mut doc, req.node_id, w, h, viewport)
                    {
                        changed = true;
                        written.push(nid);
                    }
                    continue;
                }
                if let Some(&(w, h)) = self.stream_image_sizes.get(&req.url) {
                    if apply_intrinsic_size(&mut doc, req.node_id, w, h, viewport) {
                        changed = true;
                        written.push(nid);
                    }
                    if self.stream_image_events_fired.insert((nid, req.url.clone())) {
                        fires.push((nid, Some((w, h))));
                        if let Some(image) = self.stream_image_pixels.get(&req.url) {
                            bitmap_regs.push((nid, Arc::clone(image)));
                        }
                    }
                } else if self.stream_image_errors.contains(&req.url)
                    && self.stream_image_events_fired.insert((nid, req.url.clone()))
                {
                    fires.push((nid, None));
                }
            }
            // BUG-1048: `<img>` nodes the tree walk above cannot see — a
            // `new Image()` that was never inserted. Their fetch was started by
            // `DynamicImgFetchHook`, which recorded who asked. A node the walk
            // did see is dropped here (it got its event above, keyed by the
            // picker's URL, which may differ from the raw `src` under
            // `srcset`), as is one that died or has since changed `src`.
            let mut ledger = self.stream_images_requested.lock().unwrap_or_else(|e| e.into_inner());
            ledger.script_nodes.retain(|(nid, url)| {
                if walked.contains(nid) {
                    return false;
                }
                let current_src = doc.resolve(*nid).and_then(|id| doc.try_get(id)).and_then(|n| n.get_attr("src"));
                if current_src != Some(url.as_str()) {
                    return false;
                }
                let settled = if let Some(&size) = self.stream_image_sizes.get(url) {
                    Some(Some(size))
                } else if self.stream_image_errors.contains(url) {
                    Some(None)
                } else {
                    None
                };
                let Some(size) = settled else { return true };
                if self.stream_image_events_fired.insert((*nid, url.clone())) {
                    fires.push((*nid, size));
                    if let (Some(_), Some(image)) = (size, self.stream_image_pixels.get(url)) {
                        bitmap_regs.push((*nid, Arc::clone(image)));
                    }
                }
                false
            });
            changed
        };
        // BUG-938: GAP-CANVASORIGIN (BUG-941) isn't checked on this
        // streaming/dynamic producer at all yet (only the eager pipeline in
        // `page_pipeline.rs` does) — `tainted=false` matches that existing
        // gap, not a regression introduced here.
        // BUG-1048: queued before the events — `route_task_js` runs tasks in
        // order, and an `onload` that draws the image into a canvas (the
        // usual reason to wait for it) must find the pixels already there.
        for (nid, image) in bitmap_regs {
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                j.set_img_bitmap(nid, image, false);
            });
        }
        for (nid, size) in fires {
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
                match size {
                    Some((w, h)) => j.fire_image_load(nid, w, h),
                    None => j.fire_image_error(nid),
                }
            });
        }
        if !changed {
            return;
        }
        // Дописанные `width`/`height` — презентационный хинт, то есть вход
        // каскада. Кэш инкрементального рестайла (BUG-341) знает только о
        // мутациях, пришедших из JS, поэтому мутацию со стороны шелла ему нужно
        // объявить сбросом кэша — иначе стиль `<img>` переиспользуется прежний.
        // То же для базиса флаша движкового потока (BUG-935 срез 94): ему мутацию
        // объявляют записью атрибутов в трекере.
        self.note_shell_attr_writes(written);
        self.page_prev_cascade_styles = None;
        self.relayout_raf_dirty();
    }

    /// База для разрешения относительных подресурсов текущего документа.
    ///
    /// BUG-757: это НЕ `self.source.resource_base()` — там лежит запрошенный
    /// адрес, а после серверного редиректа документ приехал с другого, и от
    /// него же обязаны резолвиться его картинки, шрифты и ссылки. Реальная база
    /// приходит из загрузчика (`LoadEvent::DocumentBase`) и годится только для
    /// той навигации, в которой получена, — отсюда сверка generation. Событие
    /// шлётся ровно при расхождении адресов, поэтому без редиректа (и для
    /// несетевых источников) ответ ровно прежний.
    pub(crate) fn document_resource_base(&self) -> Option<ResourceBase> {
        match &self.document_base {
            Some((base, generation)) if *generation == self.load_generation => Some(base.clone()),
            _ => self.source.resource_base(),
        }
    }

    /// BUG-1214: is `tab_id` the CURRENTLY active tab (the one `self`'s
    /// per-page fields actually represent right now)? Every streaming
    /// `LoadEvent` handler in `user_event.rs` gates its `self`-mutation on
    /// this before checking `generation` — checking generation alone is not
    /// enough once two tabs can stream concurrently, because
    /// `self.load_generation` only ever describes the ACTIVE tab, and a
    /// background tab's generation happening to coincide is possible after
    /// `wrapping_add` wraps around (vanishingly unlikely, but `tab_id` is the
    /// correct invariant regardless — see [`LoadEvent::EarlyPreloadHints`]).
    pub(crate) fn is_active_tab(&self, tab_id: usize) -> bool {
        self.tab_strip.tabs.get(self.tab_strip.active).is_some_and(|t| t.id == tab_id)
    }

    /// BUG-1214: a streaming `LoadEvent` arrived for a tab that is no longer
    /// `self` (`window.open()` or a manual tab switch moved it to
    /// `bg_tabs` mid-load). Before this fix such an event was either
    /// silently dropped by the single process-wide `load_generation` check
    /// (starving the background tab's navigation, `bugs/BUG-1214-OPEN.md`'s
    /// original symptom) — or, under a naive keyed-generation patch, wrongly
    /// applied to `self`'s fields, corrupting the tab that IS active
    /// instead. Neither is acceptable: this tab's own `PageSnapshot` in
    /// `bg_tabs` is not being live-updated field-by-field (that would need
    /// every `LoadEvent` arm duplicated against a `PageSnapshot`, the
    /// "полная архитектурная переделка" the bug asks for at full scope);
    /// instead the background tab is marked for a full `reload()` the moment
    /// it becomes active again (`switch_tab` already checks this exact flag
    /// for the unrelated queue_task/UserInteraction reload case) — the
    /// in-flight network fetch's bytes are discarded, but the navigation
    /// itself is no longer silently lost, and — critically — the active
    /// tab's own DOM/stream state is never touched by a load event that
    /// isn't its own.
    pub(crate) fn mark_bg_tab_needs_reload(&mut self, tab_id: usize) {
        if let Some(snap) = self.bg_tabs.get(&tab_id) {
            snap.pending_reload.set(true);
        }
    }

    /// Fetch+decode every not-yet-requested non-lazy image in `requests` on its
    /// own thread, reporting back through `LoadEvent::ImageDecoded`. Shared by
    /// the streaming ([`Self::spawn_stream_image_loads`]) and post-load
    /// ([`Self::spawn_dynamic_image_loads`]) producers.
    pub(crate) fn spawn_image_requests(
        &mut self,
        requests: Vec<lumen_layout::ImageRequest>,
        csp_gate: Option<(Vec<lumen_network::csp::CspPolicy>, String)>,
    ) {
        let Some(base) = self.document_resource_base() else { return };
        let self_origin = base.origin();
        // BUG-172: stamp the decode with this navigation's generation so the cache
        // entry is shared with the final pipeline pass (same generation) and a
        // stale producer from a superseded navigation bypasses the cache.
        let generation = self.load_generation;
        // GAP-REFERRER срез 5: this producer's only point with a live
        // `&Document` — same one-shot read the other deferred image
        // producers in this file use.
        let referrer_policy = self.layout_source.as_ref().and_then(|src| {
            src.document.lock().ok().map(|d| crate::resource_base::document_referrer_policy(&d))
        }).unwrap_or_else(lumen_network::ReferrerPolicy::default_policy);
        for req in requests {
            if req.is_lazy {
                continue;
            }
            if !self
                .stream_images_requested
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .urls
                .insert(req.url.clone())
            {
                continue;
            }
            // GAP-CSPENF срез 4: the fetch must never start at all — unlike
            // the eager pipeline's own gate (`fetch_and_decode_images`,
            // which still runs after this and fires the actual
            // `securitypolicyviolation` — this producer only needs to keep
            // the request off the wire and let `onerror` follow the normal
            // decode-failure path below).
            // GAP-CSPENF срез 43: `upgrade-insecure-requests` переписывает
            // схему до гейта (Fetch §4.1, шаг 5 раньше шага 6) — этот
            // producer стартует раньше eager-прохода, поэтому без апгрейда
            // здесь `http://`-байты успевали уйти в сеть первыми.
            let resolved_url = base.resolve_str(&req.url);
            let upgraded = csp_gate
                .as_ref()
                .and_then(|(policy, _)| crate::csp_enforce::upgrade_insecure_url(policy, &resolved_url));
            let resolved_url = upgraded.clone().unwrap_or(resolved_url);
            if let Some((policy, _original)) = &csp_gate
                && if req.embedded_content {
                    crate::csp_enforce::object_src_blocked(policy, &resolved_url, self_origin.as_ref())
                } else {
                    crate::csp_enforce::img_src_blocked(policy, &resolved_url, self_origin.as_ref())
                }
            {
                let _ = self.load_proxy.send_event(LoadEvent::ImageDecodeFailed { src: req.url });
                continue;
            }
            let base = base.clone();
            let sink = Arc::clone(&self.event_sink);
            let cookie_jar = self.active_cookie_jar();
            let proxy = self.load_proxy.clone();
            let target = self.target_color_space();
            std::thread::spawn(move || {
                // Fill the shared cache so the final `fetch_and_decode_images` pass
                // reuses these pixels instead of re-fetching+re-decoding (BUG-172).
                // Ключ кэша — сырой `req.url` (его знают layout/рендерер);
                // апгрейд меняет только адрес запроса (срез 43).
                let fetch_url: &str = upgraded.as_deref().unwrap_or(&req.url);
                let decoded = image_cache::IMAGE_CACHE.get_or_decode(generation, &req.url, || {
                    decode_image(fetch_url, &base, &sink, Some(cookie_jar), target, referrer_policy)
                });
                match decoded {
                    // BUG-1048: was a silent drop ("streaming best-effort: финальный
                    // pipeline залогирует/применит") — true for the streaming
                    // prologue (a later full pass over the same URL retries), but
                    // this fn's OTHER caller (`spawn_dynamic_image_loads`) is that
                    // final pass for a script-created `<img>`: nothing runs after
                    // it, so a fetch/decode failure needs its own signal or
                    // `onerror` never fires and `img.complete` stays `false` forever.
                    None => {
                        let _ = proxy.send_event(LoadEvent::ImageDecodeFailed { src: req.url });
                    }
                    Some(image_cache::DecodedImage::Static(img)) => {
                        let _ = proxy.send_event(LoadEvent::ImageDecoded {
                            src: req.url,
                            image: Box::new((*img).clone()),
                            animated: None,
                        });
                    }
                    Some(image_cache::DecodedImage::Animated { first, gif }) => {
                        let _ = proxy.send_event(LoadEvent::ImageDecoded {
                            src: req.url,
                            image: Box::new((*first).clone()),
                            animated: Some(Box::new((*gif).clone())),
                        });
                    }
                }
            });
        }
    }

    /// PERF-6: emit a broken-render / white-screen health signal for the page
    /// that just finished loading. No-op unless the health journal is enabled.
    /// The signal fires only when a content-bearing DOM painted nothing at all
    /// (see [`health_log::log_render_health`] for the heuristic and its limits).
    pub(crate) fn record_render_health(&self) {
        if !health_log::is_enabled() {
            return;
        }
        let layout_boxes = self.layout_box.as_ref().map(count_layout_boxes).unwrap_or(0);
        let rendered_units = self
            .layout_box
            .as_ref()
            .map(count_rendered_units)
            .unwrap_or(0);
        let dom_nodes = self
            .layout_source
            .as_ref()
            .and_then(|ls| ls.document.lock().ok().map(|d| d.node_count()))
            .unwrap_or(0);
        health_log::log_render_health(
            &self.source.describe(),
            dom_nodes,
            layout_boxes,
            rendered_units,
        );
    }

    /// UX-PARTITION-10: след политик загруженной страницы в профильных хранилищах,
    /// чтобы «Очистить» данные сайта их находило. Манифест — только ссылка (тело
    /// не скачивается).
    fn record_page_policies(&self, origin: &str, p: &crate::page_pipeline::ResponsePolicies, now: i64) {
        if !p.csp.is_empty() {
            let _ = self.csp_policies.store(origin, &p.csp.join(", "), now);
        }
        if let Some(policy) = p
            .referrer
            .as_deref()
            .and_then(|h| h.rsplit(',').find_map(lumen_storage::ReferrerPolicy::parse))
        {
            let _ = self.referrer_policies.set(origin, policy, now);
        }
        if let Some(h) = p.permissions_policy.as_deref() {
            let _ = self.permissions_policies.store(origin, h, now);
        }
        let href = self
            .layout_source
            .as_ref()
            .and_then(|ls| ls.document.lock().ok().and_then(|d| crate::doc_extract::manifest_link_href(&d)));
        if let Some(href) = href {
            let resolved = self
                .source
                .url_str()
                .and_then(|base| lumen_core::url::Url::parse(base).ok())
                .and_then(|b| b.resolve(&href).ok())
                .map_or(href, |u| u.as_str().to_owned());
            let _ = self.web_manifests.store(origin, &resolved, "", now);
        }
    }

    /// Применить результат полного pipeline (fetch + parse + CSS + images).
    /// Используется и при streaming `LoadDone`, и может быть переиспользован
    /// в будущем для других путей загрузки.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn apply_loaded_page(&mut self, mut page: LoadedPage, new_layout_source: Option<LayoutSource>, new_js_ctx: Option<Arc<dyn PersistentJs>>) {
        // Drop JS closures before layout_source to release Arc clones in QuickJS.
        self.set_js_ctx(None);
        self.layout_source = new_layout_source;
        self.set_js_ctx(new_js_ctx);
        // ADR-016 M2.2c-2b: зеркалим новый хэндл + DOM в состояние движкового
        // потока (no-op при выключенном `LUMEN_ENGINE_THREAD`).
        self.sync_engine_js_state();
        // The new runtime starts empty; re-seed it with the current Navigation state.
        self.commit_nav_state();
        self.autofill_saved_login();
        // Cross-document unification (see `pending_post_reload_traversal`): a
        // multi-step traversal landed on a same-document entry of the document
        // that just finished loading — apply its popstate/URL update now, on
        // top of the fresh runtime `commit_nav_state` just seeded.
        if let Some((state_json, display_url)) = self.pending_post_reload_traversal.take() {
            self.apply_post_reload_traversal(state_json, display_url);
        }
        // CSS Scroll Snap L2 §4: `scroll-initial-target: nearest` — прокрутить цели в
        // видимость до первого paint. Back/forward-восстановление позиции главнее.
        let initial_page_scroll = if self.pending_restore_scroll.is_none() {
            self.apply_initial_scroll_targets(&mut page)
        } else {
            None
        };
        (self.content_height, self.content_width) =
            crate::display_list_metrics::page_content_extent(&page.display_list, &page.layout_box, self.viewport_width_css());
        // Full page load: force all tiles dirty.
        self.tile_grid.mark_all_dirty(self.content_width, self.content_height);
        self.animation_scheduler.clear();
        self.transition_scheduler = TransitionScheduler::new();
        self.starting_style_tracker = StartingStyleTracker::new();
        self.prev_styles.clear();
        collect_box_styles(&page.layout_box, &mut self.prev_styles);
        self.layout_box = Some(page.layout_box);
        self.page_tracks = page.page_tracks;
        // BUG-480 срез 1: под-документы новой страницы заменяют старые целиком —
        // прежние фреймы (и их JS-контексты) падают вместе со страницей —
        // вне UI-потока (BUG-1682, `release_frame_handles`).
        frame_dynamic_load::release_frame_handles(std::mem::replace(&mut self.frames, page.frames));
        self.frame_env = page.frame_env;
        // FRAME-4 срез 3: слоты старой страницы адресуют документы, которых
        // больше нет — новый ответ на них уже не придёт (generation этого
        // хозяина у следующей страницы начнётся заново), а старый, если
        // всё ещё летит, и так уйдёт в `apply_frame_navigation`'s "old_doc
        // отсутствует" ветку; чистка здесь — только против утечки памяти.
        frames::clear_frame_nav_requests(&mut self.frame_nav_requests);
        // FRAME-8: та же причина — брони старой страницы адресуют документы,
        // которых больше нет.
        self.pending_new_frames.clear();
        self.pending_frame_load_dispatch.clear();
        // BUG-480 срез 16: см. тот же сброс в резервном пути reload'а выше.
        self.hovered_frame = None;
        // BUG-480 срез 14: список страницы пишется ПОСЛЕ замены фреймов, а не
        // до неё — `set_display_list` вклеивает в него содержимое под-документов
        // из `self.frames`, и на прежнем порядке первый кадр новой страницы
        // склеивался с фреймами ПРЕДЫДУЩЕЙ (а на первой загрузке — ни с чем,
        // так что фрейм оставался серой заглушкой до первого relayout).
        self.set_display_list(page.display_list);
        // P3-viewtransnav срез 4: reveal a pending cross-document view
        // transition now that this document's stylesheet exists.
        self.maybe_reveal_mpa_view_transition();
        self.sync_text_track_store();
        // content-visibility: auto (BB-4): новая страница — ratchet с нуля.
        self.cv_relevant.clear();
        self.cv_events.clear();
        self.cv_skipped.clear();
        self.cv_auto_state.clear();
        self.prev_layout_shift_rects.clear();
        self.refresh_cv_state();
        // GAP-CSSANIM срез 1: stale transitions from the previous document
        // must not surface as events on the new one.
        self.transition_events.clear();
        // GAP-CSSANIM срез 2: same for stale CSS Animations events.
        self.animation_events.clear();
        self.update_snap_containers();
        self.update_scroll_containers();
        // BUG-382: publish the primary layout's geometry + computed styles into the
        // JS runtime unconditionally, right here, before any page-visible callback
        // of this load can run (`load`/`pageshow` below, and every timer/rAF/promise
        // job the shell drains afterwards).
        //
        // `getComputedStyle()` and `getBoundingClientRect()` do not query the layout
        // engine — both read the snapshot the shell pushes. Until this call the only
        // pushes lived inside the relayout path (`relayout()`, `reload()`) and inside
        // the lazy-image block below, which collects geometry **only** when the page
        // has `loading="lazy"` images and never pushes computed styles at all. A
        // freshly loaded page therefore answered `""` / all-zeros unless some
        // unrelated relayout (resize, font swap, scroll) happened to race ahead of
        // the first script — the reported "works in one load out of four".
        //
        // ADR-016 M2.2c-2d: routed through `route_task_js` like the other seeds; the
        // owned `HashMap`s make the closure `Send + 'static`, and the `js_present`
        // gate keeps the (side-effect-free) collection JS-gated.
        //
        // BUG-504 (`overflow-rtl-scroll-left.html`): the same gap as BUG-382, just
        // for `scrollWidth`/`scrollHeight`/`scrollTop`/`scrollLeft` — those read
        // `_lumen_get_scroll_state`'s per-node cache (`crate::PersistentJs::
        // update_scroll_states`), which BUG-382's fix never seeded here, only inside
        // `relayout()`'s path and the wheel-scroll path (`try_scroll_overflow_
        // container`). A freshly loaded page with an overflow container therefore
        // answered `scrollWidth`/`scrollHeight` with the fallback border-box size
        // (`_lumen_get_bounding_rect`) instead of the real scrollable-overflow extent
        // until some unrelated relayout raced ahead of the first script — verified via
        // `content_width`/`collect_scroll_containers` being computed *correctly*
        // (confirmed by a direct `lumen_layout` unit probe returning the spec value)
        // while a live `--mcp-live-port` read immediately after `wait: stable` still
        // returned the container's own width. Same `collect_scroll_containers` +
        // `update_scroll_states` pairing `relayout()` already uses (`relayout.rs`).
        // GAP-LAYOUTSHIFT срез 4 (BUG-809): moved out of `page` before the
        // block below so `page.title`/`page.cert_info` (read further down,
        // after `page` is otherwise consumed) stay intact — a plain field
        // move, not a clone, since nothing else reads it.
        #[cfg(feature = "v8")]
        let prescript_layout_rects = page.prescript_layout_rects;
        #[cfg(feature = "v8")]
        if self.js_present
            && let Some(lb_ref) = self.layout_box.as_ref()
            && let Some(src) = self.layout_source.as_ref()
            && let Ok(doc_guard) = src.document.lock()
        {
            let viewport = self.renderer.as_ref().map_or_else(
                || Size::new(1024.0, 720.0),
                |r| {
                    let s = r.viewport_size();
                    Size::new(s.width, s.height)
                },
            );
            let rects = collect_layout_rects(lb_ref, &doc_guard);
            // GAP-LAYOUTSHIFT срез 5: scored off `collect_layout_shift_rects`,
            // not the gBCR `rects` above (see that function's doc-comment) —
            // `prescript_layout_rects` (page_pipeline.rs) is now the same
            // shift-flavoured geometry, so both sides of this diff agree.
            let shift_rects = lumen_layout::collect_layout_shift_rects(lb_ref);
            // GAP-LAYOUTSHIFT срез 4: a synchronous parse-time `<script>` may
            // have already shifted layout before this, this page's very first
            // settled frame — the normal relayout diff never sees that shift,
            // since `prev_layout_shift_rects` is seeded from `shift_rects` (the
            // POST-script geometry) right below, same as before this slice.
            // Diffing against the pre-script snapshot here, once, delivers
            // that shift into the performance buffer with the same
            // `buffered: true` semantics a real browser gives it — this is
            // the `layout-instability/buffered-flag.html` case срез 1/3 left
            // open (see `bugs/BUG-809-OPEN.md`).
            let buffered_shift = prescript_layout_rects
                .as_ref()
                .map(|pre| compute_layout_shift_score(pre, &shift_rects, viewport.width, viewport.height))
                .filter(|shift| shift.score > 0.0);
            // Seed the CLS diff baseline from this freshly loaded page's first
            // settled frame — see the reload() site above for why an unseeded
            // baseline silently scores every subsequent shift as zero.
            self.prev_layout_shift_rects = shift_rects;
            let client_rects = collect_client_rects(lb_ref, &doc_guard);
            let hit_test_tree = Arc::new(lb_ref.clone());
            let styles = collect_computed_styles(lb_ref, &doc_guard, None, viewport);
            let pseudo_styles = collect_pseudo_computed_styles(lb_ref);
            drop(doc_guard);
            let customs = collect_custom_properties(lb_ref, viewport);
            let (vw, vh) = (viewport.width, viewport.height);
            // CSSOM-7 (BUG-977): same stylesheet push as the block above —
            // this is the "LoadDone, deferred settle" producer, the other
            // point where a fresh page's cascade first becomes flushable.
            let stylesheet = Arc::clone(&src.stylesheet);
            let scroll_states: HashMap<u32, [f32; 4]> = collect_scroll_containers_for_js_state(lb_ref)
                .iter()
                .map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height]))
                .collect();
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                js.update_layout_rects(rects);
                js.update_client_rects(client_rects);
                js.update_hit_test_tree(hit_test_tree);
                js.update_computed_styles(styles);
                js.update_pseudo_computed_styles(pseudo_styles);
                js.update_custom_properties(customs);
                js.update_stylesheet(stylesheet);
                js.update_viewport_size(vw, vh);
                js.update_scroll_states(scroll_states);
                // Delivered last, like `deliver_layout_shift`'s ordinary
                // relayout call site — `had_input` is unconditionally `false`
                // here, since page load precedes any user input by definition.
                if let Some(shift) = buffered_shift {
                    js.deliver_layout_shift(shift.score, &shift.sources, false);
                }
            });
        }
        self.title = page.title.clone();
        if let Some(t) = &self.title {
            self.tab_strip.set_active_title(t.as_str());
        }
        // ph3-tls-hardening, live-wiring slice: real cert info for the new
        // top-level document, or `None` for plain HTTP / non-network sources.
        self.cert_info = page.cert_info.clone().map(crate::panels::cert_panel::PanelCertData::from);
        self.anim_frame = None;
        self.find.close_if_untouched();
        self.address_bar.close_if_untouched();
        // U-1: новая страница встаёт сверху-слева; но back/forward (и bfcache)
        // просят восстановить прежний scroll-offset через `pending_restore_scroll`,
        // т.к. навигация теперь асинхронна и сброс происходит здесь, в LoadDone,
        // а не сразу после `reload()`. Координаты докламплятся при первом redraw.
        let (restore_x, restore_y) = self.pending_restore_scroll.take().unwrap_or(initial_page_scroll.unwrap_or((0.0, 0.0)));
        self.scroll_x = restore_x;
        self.scroll_y = restore_y;
        self.issue_scroll_command();
        self.scroll_drag = None;
        self.hscroll_drag = None;
        self.frame_scroll_drag = None;
        self.scroll_anim = None;
        self.momentum_anim = None;
        self.forward_momentum_stop();
        self.touchpad_vel = (0.0, 0.0);
        self.form_state.clear();
        self.frame_text_cursor.clear();
        self.frame_text_selection_anchor.clear();
        self.field_history.clear();
        self.text_drag = None;
        self.doc_select = None;
        self.validation_tooltip = None;
        self.color_picker_node = None;
        self.date_picker_node = None;
        self.date_picker_year = 0;
        self.date_picker_month = 0;
        self.select_dropdown_node = None;
        // FRAME-6: same reset, frame-scoped overlays — see `frame_text_cursor`
        // above for why the page's own document load invalidates them too.
        self.frame_color_picker = None;
        self.frame_date_picker = None;
        self.frame_date_picker_year = 0;
        self.frame_date_picker_month = 0;
        self.frame_select_dropdown = None;
        // Reset paint timing guards so new page fires fresh PerformancePaintTiming entries.
        self.first_paint_delivered = false;
        self.first_contentful_paint_delivered = false;
        // A page was applied successfully — clear any prior settled-error flag
        // (BUG-308) so `document_ready` reflects this real load, not a stale one.
        self.load_failed = false;
        self.load_error_message = None;

        // Индексировать страницу в history_fts для omnibox (@history) и записать
        // в history_store для панели истории (Ctrl+H).
        // Пропускаем Empty и File sources — только HTTP(S) и bfcache snapshots.
        // DS-16: while Anonymous is the active profile, skip both writes —
        // an ephemeral session must leave no trace in the history a
        // Personal/Work switch-back would then see (ADR-020).
        if let Some(url) = self.source.url_str()
            && !self.active_profile_is_anonymous()
        {
            let title = page.title.as_deref().unwrap_or("");
            let _ = self.history_fts.index(self.next_history_id, url, title, "");
            self.next_history_id += 1;
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let _ = self.history_store.record_visit(url, title, now_secs);
            // UX-PARTITION-9: счётчик визитов по origin, чтобы «Очистить» находило след.
            if let Some(origin) = self.source.origin_str() {
                let _ = self.site_engagement.record_visit(&origin, now_secs);
                self.record_page_policies(&origin, &page.response_policies, now_secs);
            }
        }
        // Clear GIF animation state from previous page.
        self.animated_gifs.clear();
        self.gif_last_frame.clear();
        // Populate animated GIFs from new page; reset frame tracking.
        for (url, gif) in page.animated_gifs {
            self.animated_gifs.insert(url, gif);
        }

        // Clear video GIF state from previous page.
        self.video_gif_store.playback.lock().unwrap().clear();
        self.video_gif_store.pending_loads.lock().unwrap().clear();
        self.video_gif_last_frame.clear();
        self.video_gif_frames.clear();
        // GAP-MEDIADECODE срез 7: clear container-backed video state from previous page.
        self.video_gif_store.pending_container_loads.lock().unwrap().clear();
        self.clear_mse_sessions();
        self.video_sessions.clear();
        self.video_last_ms.clear();
        // GAP-MEDIADECODE срез 14: drop any open PCM audio sinks/timing from
        // the previous page's videos — a session-less sink playing on into
        // the new page would be audible with nothing on screen to match it.
        self.video_audio_sinks.clear();
        self.video_last_audio_ms.clear();
        // GAP-MEDIADECODE срез 9: previous page's failure records must not leak
        // onto a same-index node in the new page.
        self.video_gif_store.load_failures.lock().unwrap().clear();
        // COMPAT-MEDIA-OSDEC-WIN-BAR: подсказка относилась к прежней странице.
        self.media_bar.reset_page();
        // Same reasoning for volume/muted — a same-index node on the new page
        // must start at the spec default, not the previous page's setting.
        self.video_gif_store.audio_levels.lock().unwrap().clear();
        // Same reasoning for playbackRate (GAP-MEDIADECODE срез 18).
        self.video_gif_store.playback_rates.lock().unwrap().clear();

        // Update shields panel domain and clear per-page blocked counts.
        {
            let domain = self.source.url_str().and_then(|u| {
                // Extract hostname from the loaded URL for the shields panel.
                let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://"))?;
                let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                let host = &rest[..host_end];
                let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
                if host.is_empty() { None } else { Some(host.to_ascii_lowercase()) }
            });
            self.shields.clear_log();
            // COMPAT-PACK-IND: правки пакета для нового хоста + по console.info на запись.
            self.refresh_compat_applied(domain.as_deref());
            self.shields.set_domain(domain);
            // BUG-411: the new host may carry its own shields exception —
            // re-point the live filter at it.
            self.sync_adblock_filter();
        }

        // Clear the network panel log so each page starts with a fresh request list.
        self.network_panel.clear_log();

        // Update permission panel origin on navigation.
        {
            // Bare origin (scheme + host) for permission keying.
            let origin = self.source.url_str().and_then(crate::panels::permission_panel::permission_origin);
            self.permission.set_origin(origin);
            // UX-USER-ACTIVATION: the blocked-popup indicator belongs to the
            // document that was blocked, not to the one that replaces it.
            self.blocked_popups.remove(&self.tab_strip.tabs[self.tab_strip.active].id);
            // UX-PERMISSIONS-3: a saved answer shows in `Notification.permission`
            // without the page having to call requestPermission().
            use crate::panels::permission_panel::{PermissionKind, PermissionState};
            let saved = self.permission.state_for(PermissionKind::Notifications);
            if saved != PermissionState::Ask {
                self.settle_page_permission(PermissionKind::Notifications, saved);
            }
        }

        // PH3-19: store the page's FontRegistry for dynamic web-font registration
        // via FontLoaded events. Also clear previous page's web_fonts list so
        // relayout_with_web_fonts uses only fonts for the current page.
        self.page_font_registry = page.font_registry.clone();
        self.web_fonts.clear();
        self.requested_web_fonts.clear();
        self.deferred_web_fonts.clear();
        self.web_font_fetches.clear();

        // PH3-19: spawn one background thread per pending @font-face url() source.
        // Each thread fetch+decodes the font and sends FontLoaded; the handler
        // registers it in page_font_registry, rebuilds MultiFontMeasurer, and
        // triggers a relayout — FOUT (Flash Of Unstyled Text) swap pattern.
        self.spawn_web_font_fetches(page.pending_web_fonts);

        // Reset CPU image cache for the new page (10E.4 scroll-discard).
        self.image_cache.clear();
        if let Some(r) = self.renderer.as_mut() {
            r.set_font_provider(Some(Arc::clone(&page.font_registry) as Arc<dyn lumen_core::FontProvider>));
            // Warm the curated system-font fallback chain once, now that a
            // FontProvider (this page's FontRegistry, which wraps the system
            // font index) is available. Loads emoji / CJK / RTL / Indic / Thai
            // faces into the renderer so the codepoint cascade can resolve
            // glyphs Inter lacks. One-time: the faces persist across pages and
            // the curated families are system fonts identical for every page.
            if !self.fallbacks_preloaded {
                r.preload_curated_fallbacks();
                self.fallbacks_preloaded = true;
            }
            r.clear_images();
            for (src, image) in &page.images {
                // BUG-272 срез 17: share the Arc; raw_images no longer deep-copies.
                if let Err(err) = r.register_image(src.clone(), Arc::clone(image)) {
                    eprintln!("Картинка {src} не зарегистрирована: {err}");
                }
                self.image_cache.insert(lumen_image::ImageKey::new(src), (**image).clone());
            }
        } else {
            self.pending_images = page.images;
        }
        if let Some(w) = self.window.as_ref() {
            w.set_title(&window_title(self.title.as_deref()));
            w.request_redraw();
        }
        // Register lazy images with JS so _lumen_deliver_lazy_images can check them
        // on subsequent redraws (scroll, resize) via proximity threshold.
        //
        // After registration we run an immediate proximity check: push fresh
        // layout rects into JS, fire the IntersectionObserver, drain and fetch.
        // Without this, above-the-fold `loading="lazy"` images (most cards on
        // sites like lenta.ru) never load on first paint — `relayout()` is the
        // only other path that delivers observers, and it only runs on
        // scroll/resize/zoom, not on the initial load. (BUG-163)
        // ADR-016 M2.2c-2d: lazy-image регистрация + immediate proximity check через
        // `route_query_js` — снимаем прямое `self.js_ctx`-обращение. Вся упорядоченная
        // последовательность (register → push rects/viewport → deliver observers →
        // deliver lazy → drain requests) обёрнута в **один** `route_query_js`, чтобы под
        // флагом (`LUMEN_ENGINE_THREAD=1`) она исполнилась атомарно **в порядке** на
        // движковом потоке (value-read `take_lazy_image_requests` после void-push
        // сохраняет read-after-write), блокируя лишь ради одного результата. Owned-данные
        // (`owned_pairs`/`geom`) собираются на UI-потоке до маршрутизации (замыкание
        // `Send + 'static`); гейт `self.js_present` держит сбор геометрии
        // JS-гейтнутым — байт-идентично флаг-офф (`route_query_js(…, Some(js), …)` =
        // синхронный вызов по UI-хэндлу).
        // THREAD-9 срез 4: запросы lazy-картинок больше не ждут движок блокирующим
        // `route_query_js` — задача кладёт их в `pending_lazy_image_reqs`, их забирает
        // `drain_pending_lazy_image_reqs` в `about_to_wait` (и просит перерисовку).
        #[cfg(feature = "v8")]
        if self.js_present {
            let owned_pairs: Vec<(u32, String)> =
                page.lazy_pairs.iter().map(|(n, u)| (*n, u.clone())).collect();
            type LazyImageGeom = (
                HashMap<u32, [f32; 4]>,
                HashMap<u32, Vec<[f32; 4]>>,
                Arc<lumen_layout::LayoutBox>,
                f32,
                f32,
            );
            let geom: Option<LazyImageGeom> = if !owned_pairs.is_empty() {
                self.layout_box.as_ref().and_then(|lb_ref| {
                    self.layout_source
                        .as_ref()
                        .and_then(|ls| ls.document.lock().ok())
                        .map(|doc_guard| {
                            let viewport = self.renderer.as_ref().map_or_else(
                                || Size::new(1024.0, 720.0),
                                |r| {
                                    let s = r.viewport_size();
                                    Size::new(s.width, s.height)
                                },
                            );
                            (
                                collect_layout_rects(lb_ref, &doc_guard),
                                collect_client_rects(lb_ref, &doc_guard),
                                Arc::new(lb_ref.clone()),
                                viewport.width,
                                viewport.height,
                            )
                        })
                })
            } else {
                None
            };
            // GAP-LAYOUTSHIFT: refresh the CLS baseline with this (possibly
            // newer) rect snapshot too — cheap, and keeps it from going
            // stale relative to whatever the lazy-image registration below
            // just changed. Done here, before `geom` moves into the closure.
            // срез 5: shift-flavoured geometry, not `geom`'s gBCR rects — see
            // `collect_layout_shift_rects`'s doc-comment.
            if geom.is_some()
                && let Some(lb_ref) = self.layout_box.as_ref()
            {
                self.prev_layout_shift_rects = lumen_layout::collect_layout_shift_rects(lb_ref);
            }
            let reqs_slot = Arc::clone(&self.pending_lazy_image_reqs);
            route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
                let pairs: Vec<(u32, &str)> =
                    owned_pairs.iter().map(|(n, u)| (*n, u.as_str())).collect();
                js.register_lazy_images(&pairs);
                if let Some((rects, client_rects, hit_test_tree, vw, vh)) = geom {
                    js.update_layout_rects(rects);
                    js.update_client_rects(client_rects);
                    js.update_hit_test_tree(hit_test_tree);
                    js.update_viewport_size(vw, vh);
                    js.deliver_layout_observers();
                    js.deliver_lazy_images();
                    let reqs = js.take_lazy_image_requests();
                    if !reqs.is_empty()
                        && let Ok(mut slot) = reqs_slot.lock()
                    {
                        slot.extend(reqs);
                    }
                }
            });
        }
        // JS may have requested navigation via location.href= etc.
        self.pending_js_navigate = page.js_navigate;
        // HTML LS §8.2.3 — all resources loaded: readyState → "complete" + window.load event.
        // HTML LS §8.6 — `pageshow` fires right after `load`. `persisted=true`
        // only when this page was restored from bfcache (set by navigate_back/
        // navigate_forward); a fresh load fires `persisted=false`.
        let pageshow_persisted = std::mem::take(&mut self.pending_pageshow_persisted);
        // ADR-016 M2.2c-2d: pageshow-lifecycle void-вызовы через `route_task_js` —
        // снимаем прямое `self.js_ctx`-обращение. Оба чистый fire-and-forget без
        // синхронного чтения результата следом; под флагом (`LUMEN_ENGINE_THREAD=1`)
        // уходят off-UI-thread одним `task` (порядок сохранён), без флага (по
        // умолчанию) — синхронный вызов по UI-хэндлу, байт-идентично прежнему.
        #[cfg(feature = "v8")]
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |js| {
            // BUG-640: `domComplete`/`loadEventStart` share the "before"
            // instant, `loadEventEnd` is the "after" one.
            crate::nav_timing::record_load_start();
            js.notify_window_loaded();
            crate::nav_timing::record_load_end();
            js.fire_page_lifecycle("pageshow", pageshow_persisted);
        });
        #[cfg(not(feature = "v8"))]
        let _ = pageshow_persisted;

        // Rebuild accessibility tree and push to OS platform bridge (O-5).
        self.update_platform_ax_tree();

        // If zoom or <meta viewport initial-scale> is active, relayout with the
        // correct effective viewport. The initial load used the raw physical size.
        let zoom = self.zoom_factor;
        let meta_scale = self.layout_source.as_ref().map(meta_initial_scale).unwrap_or(1.0);
        if (zoom - 1.0).abs() > 0.001 || (meta_scale - 1.0).abs() > 0.001 {
            self.relayout();
        }
    }

}

/// BUG-1225: число render-blocking таблиц стилей из `<head>`, для которых
/// preload-сканер уже запустил загрузку, а `CssLoaded` ещё не обработан
/// event loop-ом: `(generation, счётчик)`. Пока оно ненулевое, streaming-кадр
/// не публикуется — иначе на экране виден сырой HTML без CSS. Каждая
/// посчитанная таблица шлёт `CssLoaded` ровно один раз (при сбое — пустой
/// таблицей), поэтому счётчик не зависает.
static STREAM_CSS_PENDING: std::sync::Mutex<(u64, usize)> = std::sync::Mutex::new((0, 0));

pub(crate) fn stream_css_pending_add(generation: u64) {
    let mut g = STREAM_CSS_PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if g.0 != generation {
        *g = (generation, 0);
    }
    g.1 += 1;
}

pub(crate) fn stream_css_pending_done(generation: u64) {
    let mut g = STREAM_CSS_PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if g.0 == generation {
        g.1 = g.1.saturating_sub(1);
    }
}

/// `true`, пока для навигации `generation` ждут таблицы стилей (см. выше).
pub(crate) fn stream_css_pending(generation: u64) -> bool {
    let g = STREAM_CSS_PENDING.lock().unwrap_or_else(|e| e.into_inner());
    g.0 == generation && g.1 > 0
}

/// Прогнать порцию HTML через preload-сканер, эмитнуть `EarlyPreloadHints` и
/// параллельно загрузить найденные стили (PH1-2 / PH1-8). Общая логика для обоих
/// путей `start_streaming_load`: сетевого streaming-а (URL) и нарезки
/// уже-загруженного буфера (File/Snapshot/Static). Hint-ы шлются ДО передачи
/// chunk-а DOM-парсеру, чтобы fetch подресурсов стартовал раньше.
///
/// `header_csp` — политики `Content-Security-Policy` ответа, чьё тело течёт
/// (пусто для несетевого источника); к ним добавляются `<meta>`-политики,
/// которые сканер уже встретил. Stylesheet/script, запрещённые хоть одной из
/// них, не прогреваются вовсе: CSP запрещает само обращение к источнику, а не
/// только применение ответа (BUG-1185, CSP3 §4.1.2).
#[allow(clippy::too_many_arguments)]
fn feed_preload_and_emit(
    scanner: &mut lumen_html_parser::PreloadScanner,
    chunk: &[u8],
    base: &ResourceBase,
    header_csp: &[lumen_network::csp::CspPolicy],
    proxy: &EventLoopProxy<LoadEvent>,
    tab_id: usize,
    generation: u64,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<&Arc<lumen_storage::CookieJar>>,
    media_ctx: &lumen_css_parser::MediaContext,
) {
    use lumen_network::RequestDestination;

    let early = scanner.feed_bytes(chunk);
    if early.is_empty() {
        return;
    }
    let _ = proxy.send_event(LoadEvent::EarlyPreloadHints(early.clone(), base.clone(), tab_id, generation));
    // BUG-1116: `preload`/`modulepreload`/`prefetch` hints warm the same
    // process-global cache as the stylesheet/script warm-up right below —
    // started here, as early as the streaming scanner sees them, so the
    // element's own fetch (`HttpClient::fetch_preload_cached`, driven by the
    // JS shim once the DOM is ready) and a real consumer elsewhere on the
    // page share one network round trip instead of each doing its own.
    let meta_csp: Vec<_> = scanner.meta_csp().iter().map(|p| lumen_network::csp::parse_csp_header(p)).collect();
    // BUG-1491: gated by the same header + `<meta>` policies as the
    // stylesheet/script warm-up below.
    let doc_csp: Vec<_> = header_csp.iter().chain(&meta_csp).cloned().collect();
    crate::page_pipeline::warm_preload_cache(&early, base, sink, cookie_jar.cloned(), &doc_csp);
    let self_origin = base.origin();
    // PH1-2 + BUG-171: speculatively fetch subresources off the UI thread while the
    // HTML is still streaming. Linked stylesheets AND external classic scripts are
    // warmed into the process-global prefetch cache using the SAME subresource
    // client `parse_and_layout` uses, so the final UI-thread pass reads identical
    // bytes instantly instead of blocking on the socket (cascade + script order
    // untouched — a cache miss simply re-fetches there). Stylesheets additionally
    // parse here to feed progressive intermediate frames (the previous PH1-2 path).
    for hint in &early {
        let (raw_url, dest, is_css) = match hint {
            lumen_html_parser::PreloadHint::Stylesheet { url, media, .. } => {
                // BUG-268: print-only лист финальный pipeline всё равно не
                // возьмёт (media-гейт в collect_link_hrefs) — не греем кэш
                // и, главное, не эмитим CssLoaded: промежуточные progressive-
                // кадры не должны красить страницу print-правилами.
                if media.as_deref().is_some_and(|m| !link_media_matches(m, media_ctx)) {
                    continue;
                }
                (url, RequestDestination::Style, true)
            }
            lumen_html_parser::PreloadHint::Script { url, .. } => {
                (url, RequestDestination::Script, false)
            }
            _ => continue,
        };
        let resolved_str = base.resolve_str(raw_url);
        if [header_csp, &meta_csp[..]].into_iter().any(|policies| {
            crate::csp_enforce::speculative_fetch_blocked(policies, hint, &resolved_str, self_origin.as_ref())
        }) {
            eprintln!("  ⤷ preload пропущен (CSP): {resolved_str}");
            continue;
        }
        if is_css {
            stream_css_pending_add(generation);
        }
        match base.resolve(raw_url) {
            // Local files: read is instant — no cache benefit. Only CSS needs a
            // CssLoaded event for the progressive frame; scripts are read in
            // `parse_and_layout`.
            ResolvedResource::File(path) => {
                if is_css {
                    let sheet = std::fs::read_to_string(&path)
                        .map(|text| lumen_css_parser::parse(&text))
                        .unwrap_or_default();
                    let _ = proxy.send_event(LoadEvent::CssLoaded(Box::new(sheet), tab_id, generation));
                }
            }
            ResolvedResource::Url(resolved) => {
                let proxy2 = proxy.clone();
                let base = base.clone();
                let sink = Arc::clone(sink);
                let cookie_jar = cookie_jar.cloned();
                std::thread::spawn(move || {
                    use lumen_core::url::Url;
                    let Ok(parsed) = Url::parse(&resolved) else {
                        if is_css {
                            let _ = proxy2.send_event(LoadEvent::CssLoaded(
                                Box::default(), tab_id, generation));
                        }
                        return;
                    };
                    let resource = crate::prefetch::PREFETCH_CACHE.fetch(generation, &resolved, || {
                        let client = base.http_client_for_subresource(sink, cookie_jar);
                        client
                            .fetch_subresource_with_content_type(&parsed, dest)
                            .map(|(body, content_type)| crate::prefetch::CachedResource {
                                body,
                                content_type,
                            })
                            .map_err(|e| e.to_string())
                    });
                    if is_css {
                        // Progressive preview frame only — the authoritative
                        // parse (BUG-509 fallback-encoding algorithm) happens
                        // later in `stylesheets::fetch_stylesheet_text` once
                        // the full document (and its own charset) is known.
                        // Сбой загрузки → пустая таблица: `CssLoaded` обязан
                        // прийти ровно один раз (BUG-1225, счётчик ожидания).
                        let sheet = resource
                            .map(|r| lumen_css_parser::parse(&String::from_utf8_lossy(&r.body[..])))
                            .unwrap_or_default();
                        let _ = proxy2.send_event(LoadEvent::CssLoaded(Box::new(sheet), tab_id, generation));
                    }
                });
            }
        }
    }
}

/// Событие от background-потока загрузки страницы в event loop.
///
/// Загрузка разбита на четыре фазы: (0) `EarlyPreloadHints` — хинты из первых
/// байт HTML для раннего старта subresource fetch-ов; (1) chunks сырых байт для
/// инкрементального парсинга и промежуточных кадров через
/// `IncrementalTreeBuilder::feed_bytes`; (2) `LoadDone` — все байты доступны,
/// запускаем полный pipeline (CSS + изображения); (3) `LoadError` — ошибка fetch.
pub(crate) enum LoadEvent {
    /// ADR-032: поток браузера просит главный поток завершить цикл winit.
    /// Обслуживается в `MainForwarder`, до `Lumen` не доходит.
    MainExit,
    /// ADR-032: создать окно на главном потоке (`ActiveEventLoop` есть только там).
    MainCreateWindow(Box<crate::browser_thread::CreateWindowRequest>),
    /// No-op wake-up (SDC-2). `winit`'s `ControlFlow::Wait` genuinely parks
    /// the event loop until an OS window event, a scheduled `WaitUntil`
    /// deadline, or a proxied user event arrives — an `AutomationCommand`
    /// enqueued from a BiDi/MCP thread is none of those, so without this the
    /// loop could sit parked indefinitely and never drain it.
    /// `AutomationHandle::execute` sends this through `load_proxy` right
    /// after queuing a command; `user_event` below does nothing with it —
    /// merely *receiving* a proxied event is what interrupts `Wait` and
    /// triggers the next `about_to_wait` (where automation commands are
    /// actually drained).
    AutomationWake,
    /// Subresource-хинты из первого chunk HTML (HTML LS §13.2.6.4.7
    /// «Speculative HTML parsing»). Отправляются ДО первого `HtmlChunk`,
    /// чтобы sink мог начать загружать CSS/шрифты ещё в процессе парсинга.
    /// Дедупликация с финальными хинтами из `LoadDone` — через
    /// `preload_dispatched` в `Lumen`.
    /// Предпоследнее поле — id вкладки (BUG-1214), стриминг которой породил
    /// событие: снят ОДИН раз в `reload`/`resumed` в момент запуска этой
    /// навигации и не меняется, даже если пользователь переключит активную
    /// вкладку, пока фоновый поток ещё грузит эту страницу. Последнее поле —
    /// generation навигации (U-1): идентификатор load-цикла, присвоенный в
    /// `reload`/`resumed`. `user_event` отбрасывает событие, если его
    /// generation не совпадает с ожидаемым generation-ом ЭТОЙ вкладки —
    /// защита от устаревших событий гонки навигаций (быстрый back/forward или
    /// клик по двум ссылкам подряд), которые иначе подмешали бы DOM/CSS
    /// прошлой страницы. До BUG-1214 сверка шла с единственным
    /// process-wide `Lumen::load_generation`, поэтому вторая вкладка,
    /// открытая тем же тиком `window.open()`, роняла ЛЮБОЕ событие первой —
    /// `user_event` теперь сверяет generation вкладки `tab_id`, а не активной.
    EarlyPreloadHints(Vec<lumen_html_parser::PreloadHint>, ResourceBase, usize, u64),
    /// BUG-757: база документа стала известна и отличается от запрошенного
    /// адреса (сервер ответил редиректом). Отправляется из streaming-потока,
    /// как только тело потекло с финального hop-а — то есть ДО того, как
    /// частичный DOM начнёт заказывать картинки и шрифты, которые UI-поток
    /// резолвит относительно базы. Предпоследнее поле — id вкладки, последнее —
    /// generation навигации (U-1); см. [`Self::EarlyPreloadHints`] (BUG-1214).
    DocumentBase(ResourceBase, usize, u64),
    /// Очередной chunk сырых байт HTML. UTF-8 границы не выравниваются —
    /// `IncrementalTreeBuilder::feed_bytes` буферизует незавершённые
    /// code-point-ы внутри. Предпоследнее поле — id вкладки, последнее —
    /// generation навигации (U-1); см. [`Self::EarlyPreloadHints`] (BUG-1214).
    HtmlChunk(Vec<u8>, usize, u64),
    /// CSS загружен параллельным потоком для промежуточных streaming-кадров.
    /// Мёрджится в `Lumen::stream_sheet` и применяется в `paint_partial_dom`.
    /// Предпоследнее поле — id вкладки, последнее — generation навигации
    /// (U-1); см. [`Self::EarlyPreloadHints`] (BUG-1214).
    CssLoaded(Box<lumen_css_parser::Stylesheet>, usize, u64),
    /// PH1-2c: картинка `<img>` декодирована параллельным потоком во время
    /// streaming. Регистрируется в renderer-е по ключу `src` и вызывает redraw —
    /// картинки появляются по мере прихода, а не разом в финальном `LoadDone`.
    /// Для анимированного GIF `animated` несёт все кадры (тикаются в
    /// `RedrawRequested`); `image` — нулевой кадр для немедленной отрисовки.
    ImageDecoded {
        src: String,
        image: Box<lumen_image::Image>,
        animated: Option<Box<lumen_image::AnimatedGif>>,
    },
    /// BUG-1048: the streaming/dynamic decode pipeline ([`Lumen::spawn_image_requests`])
    /// gave up on `src` (fetch or decode failure), same fork the eager pipeline
    /// already has (`page_pipeline.rs`'s `None` arm calling `fire_image_error`).
    /// Before this event the streaming path had no failure signal at all — a
    /// script-created `<img>` whose fetch 404s never got `onerror` and
    /// `complete` stayed `false` forever.
    ImageDecodeFailed { src: String },
    /// PH3-19: web-шрифт из @font-face url() декодирован в фоновом потоке.
    /// Регистрируется в FontRegistry + MultiFontMeasurer и вызывает relayout —
    /// текст появляется в fallback-шрифте сразу, подменяется по приходу (FOUT).
    FontLoaded {
        family: String,
        weight: u16,
        style: lumen_core::FontStyle,
        unicode_range: Vec<lumen_font::UnicodeRange>,
        /// `ascent-override` дескриптор (CSS Fonts L4 §14, FONTLOAD-11) — доля
        /// `font-size`, `None` — `normal`/отсутствует. Разобран в фоновом
        /// потоке fetch-а, той же функцией, что и `unicode_range`.
        ascent_override: Option<f32>,
        /// `descent-override` дескриптор, та же семантика.
        descent_override: Option<f32>,
        /// `size-adjust` дескриптор (CSS Fonts L4 §14.4, FONTLOAD-12), та же
        /// точка разбора, `None` = `100%`.
        size_adjust: Option<f32>,
        /// `line-gap-override` дескриптор (CSS Fonts L4 §14.3, FONTLOAD-13),
        /// та же точка разбора, что `ascent_override`.
        line_gap_override: Option<f32>,
        /// `font-variation-settings` дескриптор (CSS Fonts L4 §6.2,
        /// FONTLOAD-20) — variable-font axis defaults для этого face-а,
        /// разобранные в фоновом потоке fetch-а.
        variation_settings: Vec<([u8; 4], f32)>,
        bytes: Vec<u8>,
    },
    /// Все байты получены — для финального полного pipeline. Предпоследнее
    /// поле — id вкладки, последнее — generation навигации (U-1); см.
    /// [`Self::EarlyPreloadHints`] (BUG-1214).
    LoadDone(Box<RawPage>, usize, u64),
    /// Ошибка при загрузке страницы. Предпоследнее поле — id вкладки,
    /// последнее — generation навигации (U-1); см. [`Self::EarlyPreloadHints`]
    /// (BUG-1214).
    /// Сообщение, код сбоя (COMPAT-ERRORS: определён в месте возникновения,
    /// а не разобран из сообщения), вкладка, поколение.
    LoadError(String, lumen_core::net_error::NetErrorCode, usize, u64),
    /// ph3-tls-hardening A6: the fetch failed because the TLS handshake's
    /// certificate did not verify (`lumen_core::error::Error::CertInvalid`,
    /// downcast out of the `Box<dyn Error>` the streaming/static fetch paths
    /// return before it collapses to a string). Routes to the cert
    /// interstitial instead of the generic `LoadError` path. Fields: full
    /// navigation URL, hostname (the key `tls::bypass::allow_host` needs for
    /// "Proceed anyway"), the structured reason, its diagnosed cause
    /// (COMPAT-TRUST-CAUSES), tab id, and generation (U-1);
    /// see [`Self::EarlyPreloadHints`] (BUG-1214).
    CertError(String, String, lumen_core::error::CertError, crate::cert_cause::CertCause, usize, u64),
    /// UX-SECURITY-UI срез 2: адрес навигации найден в локальной базе Safe
    /// Browsing, загрузка не начата. Поля: URL, хост, имя списка, тип угрозы,
    /// вкладка, поколение.
    ThreatBlocked(String, String, String, lumen_storage::ThreatType, usize, u64),
    /// BUG-171 этап 2: финальный pipeline (parse → JS → fetch подресурсов →
    /// layout) выполнен на фоновом потоке; готовый результат применяется на
    /// UI-потоке (`apply_loaded_page`) без блокировки event loop. Предпоследнее
    /// поле — id вкладки, последнее — generation навигации (U-1); см.
    /// [`Self::EarlyPreloadHints`] (BUG-1214).
    RenderDone(Box<RenderOutcome>, usize, u64),
    /// FRAME-4 срез 3: навигация ОДНОГО фрейма (клик по ссылке/сабмит формы/
    /// шаг истории внутри него) выполнена на фоновом потоке — та же сеть+
    /// парсинг+скрипты+layout, что раньше блокировали UI-поток целиком внутри
    /// `frames::navigate_frame`, теперь `frames::run_frame_navigation` там же,
    /// но за пределами event loop. `host_doc`+`host` — ключ generation-слота
    /// (`frames::FrameNavRequest`), `generation` — снятый при отправке номер:
    /// `Lumen::on_frame_nav_done` сверяет их и роняет ответ, если хозяин успел
    /// навигировать ещё раз, пока этот запрос был в полёте. `old_doc` —
    /// документ, который заменяет `handles` (пусто, если фрейма к этому
    /// моменту уже нет — предок навигировал сам, либо страница
    /// перезагрузилась целиком).
    FrameNavDone {
        host_doc: Arc<Mutex<Document>>,
        host: NodeId,
        old_doc: Arc<Mutex<Document>>,
        generation: u64,
        handles: Vec<frames::FrameHandle>,
    },
    /// FRAME-8: первичная загрузка ОДНОГО host-элемента, которого не видел
    /// первичный проход `frames::load_frame_sub_documents`, — вставлен
    /// скриптом позже, либо получил `src` тогда же. Тот же фоновый путь, что
    /// у [`Self::FrameNavDone`] (`frames::run_new_frame_load`), но это
    /// ДОБАВЛЕНИЕ хэндла, не замена — `host` раньше был в дереве без него.
    /// `host_doc`+`host` — ключ брони `Lumen::pending_new_frames`; `is_top` —
    /// снятая на момент отправки принадлежность («страница» или конкретный
    /// документ фрейма), нужна применяющей стороне ровно как
    /// `frames::scan_dynamic_frames`.
    FrameNewLoadDone {
        host_doc: Arc<Mutex<Document>>,
        host: NodeId,
        is_top: bool,
        handles: Vec<frames::FrameHandle>,
    },
}

/// Размер одного HTML-chunk при разбивке для инкрементального парсинга.
pub(crate) const STREAM_CHUNK_BYTES: usize = 8 * 1024;
/// Минимальный интервал между промежуточными кадрами при streaming (мс) — ~60 Гц.
pub(crate) const STREAM_PAINT_INTERVAL_MS: u128 = 16;
