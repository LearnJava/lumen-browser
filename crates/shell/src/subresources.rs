//! Page subresources fetched by the load pipeline after the document is parsed:
//! `<track>` WebVTT text, `background-image: url(...)` bitmaps, `@font-face`
//! sources and the eager `<img src>` pass (fetch + decode, batch SH-3d).
//!
//! Kept apart from [`crate::tracks`], which is deliberately network-free (its
//! fetching is abstracted behind a closure so the cue logic stays unit-testable)
//! — everything here talks to the network through [`ResourceBase`].
//!
//! Moved out of `main.rs` by the SPLIT track (batches SH-3c, SH-3d); behaviour
//! and signatures are unchanged.

use crate::*;

/// P3-webvtt срез 3: фетчит текст `.vtt` по `src` из `<track>` (файл или URL).
/// `None` — ресурс не скачался; страница продолжает жить без субтитров.
pub(crate) fn fetch_vtt_text(
    src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Option<String> {
    match base.resolve(src) {
        ResolvedResource::File(path) => std::fs::read_to_string(&path).ok(),
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url;
            use lumen_network::RequestDestination;
            let sub_url = Url::parse(&url).ok()?;
            let client = base.http_client_for_subresource(sink.clone(), cookie_jar);
            let bytes = client
                .fetch_subresource(&sub_url, RequestDestination::Media)
                .ok()?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

/// Скачивает и декодирует все `background-image: url(...)` из готового
/// layout-дерева. Дубликаты URL фильтруются на стороне layout
/// (`collect_background_image_requests`). Ошибки скачивания / декодирования
/// логируются в stderr — battle-tested fail-soft: битая bg-картинка не валит
/// страницу, renderer всё равно отобразит background-color поверх.
///
/// GAP-CSPENF срез 18: `csp_gate` — политика top-level документа (то же
/// `document_csp_policy`, что срезы 4/7/17 уже передают своим вызовам), если
/// она есть. URL, запрещённый `img-src`/`default-src`, не доходит до
/// `fetch_image_bytes` вовсе (тот же принцип «ни одного исходящего байта») и
/// вместо этого попадает во второй элемент возврата — резолвленные URL для
/// отложенного `securitypolicyviolation` (эта функция запускается до layout,
/// поэтому у вызывающей стороны есть URL и до, и после фетча — здесь удобнее
/// вернуть уже резолвленный, раз gate его всё равно резолвит).
pub(crate) fn fetch_and_decode_background_images(
    layout: &LayoutBox,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
    csp_gate: Option<(&lumen_network::csp::CspPolicy, Option<&lumen_network::Origin>)>,
) -> (Vec<(String, Arc<lumen_image::Image>)>, Vec<String>) {
    // DPR 1.0 — тот же, что у `build_display_list_ordered` (обёртка без dpr),
    // иначе выбранный здесь кандидат `image-set()` не совпал бы с ключом,
    // который эмиттер кладёт в `DrawBackgroundImage.src`.
    let urls = lumen_layout::collect_background_image_requests(layout, 1.0);
    // Параллельная загрузка+декодирование, порядок сохраняем (ключи уникальны).
    let outcomes = parallel_map(&urls, |_, url| {
        if let Some((policy, self_origin)) = csp_gate {
            let resolved = base.resolve(url);
            let abs = match &resolved {
                ResolvedResource::Url(u) => u.clone(),
                ResolvedResource::File(p) => p.display().to_string(),
            };
            if crate::csp_enforce::img_src_blocked(policy, &abs, self_origin) {
                return Err(abs);
            }
        }
        let bytes = match fetch_image_bytes(url, base, sink, cookie_jar.clone()) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Пропуск bg-картинки {url}: {e}");
                return Ok(None);
            }
        };
        // LIB-4: SVG больше не особый случай — `decode_to` рисует его через
        // resvg наравне с любым растровым форматом.
        let image = match lumen_image::decode_to(&bytes, target) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Не декодируется bg-картинка {url}: {e}");
                return Ok(None);
            }
        };
        eprintln!(
            "Загружена bg-картинка: {url} ({}×{}, {:?})",
            image.width, image.height, image.format
        );
        // BUG-272 срез 17: wrap once in Arc so `register_image` shares the buffer.
        Ok(Some((url.clone(), Arc::new(image))))
    });
    let mut decoded = Vec::new();
    let mut blocked = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(Some(pair)) => decoded.push(pair),
            Ok(None) => {}
            Err(blocked_uri) => blocked.push(blocked_uri),
        }
    }
    (decoded, blocked)
}

/// Загружает шрифты из @font-face правил таблицы стилей в `FontRegistry`.
///
/// Для каждого `FontFaceRule` перебирает `src:` источники в порядке (CSS §4.1:
/// первый успешный wins). `local()` пропускается — `SystemFontIndex` уже
/// покрывает системные шрифты. `url()` загружается так же, как изображения.
/// WOFF/WOFF2 прозрачно декодируются в sfnt перед регистрацией.
///
/// Ошибки загрузки/декодирования отдельных источников не фатальны: пишутся в
/// stderr и переходим к следующему источнику.
/// Convert a FontFaceRule from the CSS parser to a DOM FontFace object.
pub(crate) fn rule_to_font_face(rule: &lumen_css_parser::FontFaceRule) -> lumen_dom::FontFace {
    use lumen_css_parser::FontFaceSourceKind;

    let src_parts: Vec<String> = rule
        .sources
        .iter()
        .map(|src| {
            let kind_str = match src.kind {
                FontFaceSourceKind::Url => "url",
                FontFaceSourceKind::Local => "local",
            };
            format!("{}(\"{}\")", kind_str, src.value)
        })
        .collect();
    let src_str = src_parts.join(", ");

    lumen_dom::FontFace::new(
        rule.family.clone(),
        rule.style.as_deref().unwrap_or("normal").to_string(),
        rule.weight.as_deref().unwrap_or("400").to_string(),
        rule.stretch.clone(),
        rule.unicode_range.clone(),
        src_str,
    )
    .with_extended_descriptors(lumen_dom::FontFaceExtendedDescriptors {
        feature_settings: rule.feature_settings.clone(),
        variation_settings: rule.variation_settings.clone(),
        display: rule.display.clone(),
        ascent_override: rule.ascent_override.clone(),
        descent_override: rule.descent_override.clone(),
        line_gap_override: rule.line_gap_override.clone(),
        size_adjust: rule.size_adjust.clone(),
    })
}

/// PH3-19: загружает @font-face правила, разделяя источники на два прохода:
/// 1. `local()` — синхронно (системные шрифты уже в памяти), результат в `registry`.
/// 2. `url()` — возвращается как `Vec<PendingWebFont>` для фоновой загрузки;
///    первый layout строится на fallback (bundled Inter), web-шрифты приходят позже
///    через `LoadEvent::FontLoaded` и вызывают relayout (FOUT-swap).
///
/// CSS Fonts L4 §4.1: источники в каждом `@font-face` пробуются по порядку;
/// первый успешный `local()` выигрывает и url()-источники этого правила пропускаются.
pub(crate) fn load_font_faces(
    font_faces: &[lumen_css_parser::FontFaceRule],
    _base: &ResourceBase,
    _sink: &Arc<dyn EventSink>,
    _cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> (lumen_font::FontRegistry, Vec<PendingWebFont>) {
    use lumen_css_parser::FontFaceSourceKind;
    use lumen_core::FontStyle;

    let registry = lumen_font::FontRegistry::new();
    let mut pending: Vec<PendingWebFont> = Vec::new();

    for rule in font_faces {
        if rule.family.is_empty() || rule.sources.is_empty() {
            continue;
        }

        let weight = parse_font_weight(rule.weight.as_deref());
        let style = rule
            .style
            .as_deref()
            .and_then(FontStyle::parse_keyword)
            .unwrap_or(FontStyle::Normal);
        // CSS Fonts L4 §4.5: дескриптор `font-stretch` правила участвует в
        // подборе `local()`-источника — `@font-face { src: local("Arial");
        // font-stretch: condensed }` обязан взять узкий face семейства, а не
        // обычный. Диапазон из двух значений сводится к первому (`parse`).
        let stretch = rule
            .stretch
            .as_deref()
            .and_then(lumen_layout::FontStretch::parse)
            .unwrap_or(lumen_layout::FontStretch::NORMAL)
            .as_percent();

        let mut local_resolved = false;
        for src in &rule.sources {
            if src.kind == FontFaceSourceKind::Local {
                // CSS Fonts L4 §4.1 + §4.3: try local() first; case-insensitive
                // match against system fonts. First hit wins the whole rule.
                if let Some(bytes) = registry.resolve_local_bytes(&src.value, weight, style, stretch) {
                    eprintln!(
                        "@font-face загружен: «{}» weight={} src={} (local)",
                        rule.family, weight, src.value,
                    );
                    let ranges = rule
                        .unicode_range
                        .as_deref()
                        .map(lumen_font::parse_unicode_ranges)
                        .unwrap_or_default();
                    // CSS Fonts L4 §14 (FONTLOAD-17, BUG-467): overrides применяются
                    // к face-у независимо от того, откуда взяты байты (local()/url()) —
                    // те же дескрипторы, что `load_font_faces` кладёт в `PendingWebFont`
                    // ниже для url()-ветки.
                    let ascent_override = rule.ascent_override.as_deref()
                        .and_then(lumen_font::parse_metric_override_percent);
                    let descent_override = rule.descent_override.as_deref()
                        .and_then(lumen_font::parse_metric_override_percent);
                    let size_adjust = rule.size_adjust.as_deref()
                        .and_then(lumen_font::parse_metric_override_percent);
                    let line_gap_override = rule.line_gap_override.as_deref()
                        .and_then(lumen_font::parse_metric_override_percent);
                    // FONTLOAD-20 (BUG-467): font-variation-settings дескриптор —
                    // те же местa разбора/проводки, что четыре override-дескриптора
                    // выше, но список осей вместо одного `Option<f32>`.
                    let variation_settings = rule.variation_settings.as_deref()
                        .map(lumen_font::parse_variation_settings)
                        .unwrap_or_default();
                    registry.register_from_bytes(
                        &rule.family, weight, style, &ranges, bytes,
                        ascent_override, descent_override, size_adjust, line_gap_override,
                        variation_settings,
                    );
                    local_resolved = true;
                    break;
                }
            }
        }
        if local_resolved {
            continue;
        }

        // No local() succeeded — queue the first url() source for async fetch.
        if let Some(url_src) = rule.sources.iter().find(|s| s.kind == FontFaceSourceKind::Url) {
            pending.push(PendingWebFont {
                family: rule.family.clone(),
                weight,
                style,
                unicode_range_str: rule.unicode_range.clone(),
                ascent_override_str: rule.ascent_override.clone(),
                descent_override_str: rule.descent_override.clone(),
                size_adjust_str: rule.size_adjust.clone(),
                line_gap_override_str: rule.line_gap_override.clone(),
                variation_settings_str: rule.variation_settings.clone(),
                url: url_src.value.clone(),
            });
        }
    }

    (registry, pending)
}

/// Парсит `font-weight` дескриптор @font-face: ключевые слова + числа.
/// Диапазоны (`400 700`) — берём первое значение. Default: 400.
pub(crate) fn parse_font_weight(s: Option<&str>) -> u16 {
    let Some(s) = s else { return 400 };
    match s.trim() {
        "normal" => 400,
        "bold" => 700,
        other => other
            .split_ascii_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(400),
    }
}

/// Обходит DOM через `lumen_layout::collect_image_requests` — picker учитывает
/// `<picture>`/`srcset`/`sizes`, поэтому ключ совпадает с тем, что layout
/// эмитит в `DisplayCommand::DrawImage.src`. Для каждого запроса скачивает
/// байты и декодирует через `lumen_image::decode` (PNG/JPEG dispatch).
///
/// Побочный эффект: для `<img>` без явных `width`/`height` проставляет
/// intrinsic dimensions из декодированного изображения (HTML5 §10 mapped
/// attributes). Author CSS затем перекроет при необходимости.
///
/// Возвращает `(images, animated_gifs, lazy_pairs, blocked_by_img_src, cross_origin_urls)`:
/// - `images` — декодированные картинки для немедленной регистрации в renderer-е
///   (включает frame 0 каждого анимированного GIF);
/// - `animated_gifs` — многокадровые GIF-анимации для тиканья в `RedrawRequested`;
/// - `lazy_pairs` — `(node_id_u32, url)` для `<img loading="lazy">`, которые
///   не загружаются сейчас и будут зарегистрированы через `_lumen_init_lazy_images`;
/// - `blocked_by_img_src` — GAP-CSPENF срез 4: URL, которые `img-src`/
///   `default-src` документа запретил; фетч для них не выполнялся вовсе
///   (сеть их не видела). Вызывающая сторона диспатчит
///   `securitypolicyviolation` по этому списку — здесь для этого нет JS-рантайма
///   (fetch идёт параллельно, до его создания);
/// - `cross_origin_urls` — GAP-CANVASORIGIN (BUG-941): raw `req.url` (тот же
///   ключ, что несёт `images`) картинок, чей резолвленный origin отличается от
///   `base.origin()` AND либо не несут `crossorigin`, либо несут его и не
///   прошли CORS-проверку ответа. Срез 2: `<img crossorigin>` на такой URL
///   теперь идёт через `decode_image_cors` — реальный `Origin`-header плюс
///   `Access-Control-Allow-Origin`/`-Allow-Credentials` (`lumen_network::
///   HttpClient::fetch_cors`), а не только сравнение origin строк; прошедшая
///   проверку картинка НЕ попадает в этот список (canvas не заражается).
///   Credentials-режим (`crossorigin="anonymous"` → `SameOrigin`,
///   `"use-credentials"` → `Include`) гейтит `Cookie` на actual-запросе
///   (срез 4, BUG-941: `CredentialsMode::cross_origin_credentials()` в
///   `fetch_cors`) — раньше это не было так, теперь Fetch §4.7 шаг 3 не
///   нарушается. Без атрибута — прежнее консервативное поведение (всегда taint).
#[allow(clippy::type_complexity)]
pub(crate) fn fetch_and_decode_images(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: lumen_core::geom::Size,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
) -> (
    Vec<(String, Arc<lumen_image::Image>)>,
    Vec<(String, lumen_image::AnimatedGif)>,
    Vec<(u32, String)>,
    Vec<String>,
    Vec<String>,
) {
    let requests = lumen_layout::collect_image_requests(doc, viewport);

    // GAP-CSPENF срез 4: посчитать политику один раз здесь же, до параллельной
    // фазы — та же одноразовая точка, что `crate::csp_enforce::document_csp_policy`
    // уже использует в `scripts.rs` для script-src.
    let csp_gate = {
        let root = doc.root();
        crate::csp_enforce::document_csp_policy(doc, root)
    };
    let self_origin = base.origin();

    /// Результат параллельной фазы fetch+decode одной картинки. Применение к
    /// документу (intrinsic size) и сборка выходных векторов — отдельной
    /// последовательной фазой, чтобы порядок и `&mut doc` остались под контролем.
    enum ImgOutcome {
        /// `loading="lazy"` — отложить до приближения к вьюпорту.
        Lazy,
        /// Пропуск (ошибка сети/декодирования) — уже залогировано.
        Skip,
        /// `img-src`/`default-src` запретил этот URL — фетч не выполнялся.
        Blocked,
        /// Статическая картинка (включая 1-кадровый GIF). `Arc<Image>` (BUG-272
        /// срез 17): разделяет аллокацию пикселей с `IMAGE_CACHE`, а не копирует.
        Static {
            image: Arc<lumen_image::Image>,
            /// Intrinsic-размеры для HTML-атрибутов, если их не задал автор.
            intrinsic: Option<(u32, u32)>,
            /// GAP-CANVASORIGIN: резолвленный origin картинки отличается от
            /// `self_origin`.
            cross_origin: bool,
        },
        /// Многокадровый GIF: первый кадр + полная анимация.
        Animated {
            first: Arc<lumen_image::Image>,
            gif: lumen_image::AnimatedGif,
            intrinsic: Option<(u32, u32)>,
            /// GAP-CANVASORIGIN — see [`ImgOutcome::Static::cross_origin`].
            cross_origin: bool,
        },
    }

    // GAP-CANVASORIGIN: plain same-origin URL comparison. `self_origin`
    // absent (opaque document origin, e.g. `file:`) never taints — matches
    // the `img-src` gate's "don't invent a restriction" stance above. A
    // cross-origin result from this closure is not final by itself any
    // more (срез 2): `<img crossorigin>` on such a URL runs the real CORS
    // check below (`decode_image_cors`) and only taints if that check fails.
    let is_cross_origin = |resolved_url: &str| -> bool {
        let Some(self_o) = self_origin.as_ref() else { return false; };
        let Ok(parsed) = lumen_core::url::Url::parse(resolved_url) else { return false; };
        match lumen_network::Origin::from_url(&parsed) {
            Ok(img_o) => !self_o.same_origin(&img_o),
            // Opaque scheme (data:/blob:) — HTML LS §4.12.5.1.2 explicitly
            // exempts `data:` from tainting; blob: has no cross-document
            // network fetch here either, so treat the same way.
            Err(_) => false,
        }
    };

    // Фаза 1 (параллельно): сеть + декодирование. Не трогаем `doc`.
    // BUG-172: декод идёт через `IMAGE_CACHE` — картинки, уже загруженные
    // прогрессивным streaming-проходом (`spawn_stream_image_loads`), берутся из
    // кэша без повторного fetch+decode; их `wants_intrinsic`/`is_lazy` решаются
    // здесь, а пиксели только клонируются.
    let outcomes = parallel_map(&requests, |_, req| {
        if req.is_lazy {
            return ImgOutcome::Lazy;
        }
        let resolved_url = base.resolve_str(&req.url);
        if let Some((policy, _original)) = &csp_gate
            && crate::csp_enforce::img_src_blocked(policy, &resolved_url, self_origin.as_ref())
        {
            return ImgOutcome::Blocked;
        }
        let url_cross_origin = is_cross_origin(&resolved_url);
        // BUG-269: apply intrinsic size whenever the author left AT LEAST ONE
        // dimension unset (not only when BOTH are unset). A replaced element
        // with a fixed width and `height: auto` must derive its height from the
        // intrinsic aspect ratio (CSS 2.1 §10.6.2); `apply_intrinsic_size` fills
        // the missing slot from that ratio.
        let wants_intrinsic = !(req.has_explicit_width && req.has_explicit_height);
        // GAP-CANVASORIGIN срез 2 (BUG-941): `<img crossorigin>` on a
        // cross-origin URL takes the real CORS-checked fetch instead of the
        // plain cached one — see `decode_image_cors` doc comment for why it
        // bypasses `IMAGE_CACHE`. A passing check untaints the canvas draw
        // (`cross_origin = false` below); a failing one is a fetch error,
        // same bucket as a network failure (`ImgOutcome::Skip`), not a
        // tainted-but-visible image.
        let (decoded, cross_origin) = match (url_cross_origin, req.crossorigin, self_origin.as_ref()) {
            (true, Some(mode), Some(origin)) => (
                decode_image_cors(CorsImageFetch {
                    resolved_url: &resolved_url,
                    raw_src: &req.url,
                    self_origin: origin,
                    mode,
                    base,
                    sink,
                    cookie_jar: cookie_jar.clone(),
                    target,
                }),
                false,
            ),
            _ => (
                image_cache::IMAGE_CACHE.get_or_decode_current(&req.url, || {
                    decode_image(&req.url, base, sink, cookie_jar.clone(), target)
                }),
                url_cross_origin,
            ),
        };
        match decoded {
            None => ImgOutcome::Skip,
            Some(image_cache::DecodedImage::Static(img)) => {
                // BUG-272 срез 17: share the cache's Arc, not a pixel copy.
                let intrinsic = wants_intrinsic.then_some((img.width, img.height));
                ImgOutcome::Static { image: img, intrinsic, cross_origin }
            }
            Some(image_cache::DecodedImage::Animated { first, gif }) => {
                let intrinsic = wants_intrinsic.then_some((first.width, first.height));
                ImgOutcome::Animated { first, gif: (*gif).clone(), intrinsic, cross_origin }
            }
        }
    });

    // Фаза 2 (последовательно): мутация `doc` + сборка результата в порядке DOM.
    let mut out: Vec<(String, Arc<lumen_image::Image>)> = Vec::new();
    let mut anim_gifs: Vec<(String, lumen_image::AnimatedGif)> = Vec::new();
    let mut lazy_pairs: Vec<(u32, String)> = Vec::new();
    let mut blocked_by_img_src: Vec<String> = Vec::new();
    let mut cross_origin_urls: Vec<String> = Vec::new();
    for (req, outcome) in requests.into_iter().zip(outcomes) {
        match outcome {
            ImgOutcome::Lazy => lazy_pairs.push((req.node_id.index() as u32, req.url)),
            ImgOutcome::Skip => {}
            ImgOutcome::Blocked => blocked_by_img_src.push(base.resolve_str(&req.url)),
            ImgOutcome::Static { image, intrinsic, cross_origin } => {
                if let Some((w, h)) = intrinsic {
                    apply_intrinsic_size(doc, req.node_id, w, h);
                }
                if cross_origin {
                    cross_origin_urls.push(req.url.clone());
                }
                out.push((req.url, image));
            }
            ImgOutcome::Animated { first, gif, intrinsic, cross_origin } => {
                if let Some((w, h)) = intrinsic {
                    apply_intrinsic_size(doc, req.node_id, w, h);
                }
                if cross_origin {
                    cross_origin_urls.push(req.url.clone());
                }
                out.push((req.url.clone(), first));
                anim_gifs.push((req.url, gif));
            }
        }
    }
    (out, anim_gifs, lazy_pairs, blocked_by_img_src, cross_origin_urls)
}

pub(crate) fn fetch_image_bytes(
    raw_src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    fetch_subresource_bytes(raw_src, base, sink, cookie_jar, lumen_network::RequestDestination::Image, "img")
}

/// Same fetch as [`fetch_image_bytes`], tagged as an `@font-face url()` body
/// (BUG-520) instead of an image: both `page_load.rs` and `frames.rs` used
/// to route font bytes through `fetch_image_bytes`, which fed the request
/// through `RequestDestination::Image`. That destination is wrong on three
/// independent axes — Mixed Content classifies `Image` as `OptionallyBlockable`
/// while `Font` is `Blockable` (W3C Mixed Content §5.3), Resource Timing's
/// `initiatorType` came out `"img"` instead of the spec's `"css"`, and
/// ad-block filter matching saw `ResourceType::Image` instead of `::Font`
/// (`$image`/`$font` EasyList options no longer line up with what actually
/// loaded) — despite the bytes themselves decoding fine either way.
pub(crate) fn fetch_font_bytes(
    raw_src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    fetch_subresource_bytes(raw_src, base, sink, cookie_jar, lumen_network::RequestDestination::Font, "font")
}

fn fetch_subresource_bytes(
    raw_src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    destination: lumen_network::RequestDestination,
    span_label: &str,
) -> Result<Vec<u8>, Box<dyn Error>> {
    match base.resolve(raw_src) {
        ResolvedResource::File(path) => std::fs::read(&path).map_err(|e| {
            format!("file://{} {e}", path.display()).into()
        }),
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url;

            // Images/fonts are loaded in no-cors mode: cross-origin allowed, but
            // mixed-content enforcement still applies for HTTPS pages.
            let lumen_url = Url::parse(&url)?;
            let client = base.http_client_for_subresource(sink.clone(), cookie_jar);
            // PERF-1: one span per fetch — back-to-back spans on a lane
            // reveal sequential UI-thread subresource loading.
            let mut fetch_span = lumen_core::trace::span(format!("{span_label} {url}"), "net");
            let bytes = client.fetch_subresource(&lumen_url, destination)?;
            fetch_span.set_bytes(bytes.len());
            Ok(bytes)
        }
    }
}

/// Fetch + decode one `<img src>` into a [`DecodedImage`], or `None` on a
/// fetch/decode failure (already logged).
///
/// BUG-172: single source of truth for the decode logic shared by the streaming
/// progressive loader ([`Lumen::spawn_stream_image_loads`]) and the final pipeline
/// ([`fetch_and_decode_images`]). Both call this through
/// [`image_cache::IMAGE_CACHE`], so each `src` is fetched and decoded exactly once
/// per navigation; the second path clones the cached pixels instead of repeating
/// the network round-trip and the decoder.
pub(crate) fn decode_image(
    raw_src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
) -> Option<image_cache::DecodedImage> {
    let bytes = match fetch_image_bytes(raw_src, base, sink, cookie_jar) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Пропуск картинки {raw_src}: {e}");
            return None;
        }
    };
    decode_image_bytes(raw_src, bytes, target)
}

/// GAP-CANVASORIGIN срез 2 (BUG-941): fetch+decode a cross-origin `<img
/// crossorigin>` request through the real CORS protocol (Fetch §3-§4) —
/// `Origin` header sent, response's `Access-Control-Allow-Origin`/
/// `-Allow-Credentials` validated, not just the same-origin URL comparison
/// `fetch_and_decode_images`'s `is_cross_origin` closure does for every other
/// image. `None` on a network error OR a failed CORS check — HTML LS's media
/// resource fetch algorithm treats both the same way (the request errors,
/// no image loads), it does not fall back to a tainted-but-visible image.
///
/// Bypasses `image_cache::IMAGE_CACHE` deliberately: that cache is keyed by
/// URL alone, with no axis for "was this fetched with credentials/Origin or
/// without" — reusing a plain no-cors cache hit here would skip the very
/// check this function exists to run. The cost is a duplicate network
/// round-trip if the same cross-origin URL also appears as a plain `<img>`
/// elsewhere on the page; acceptable for a first slice, not a correctness bug.
/// Bundles [`decode_image_cors`]'s inputs — plain positional params would
/// trip `clippy::too_many_arguments` at eight.
struct CorsImageFetch<'a> {
    resolved_url: &'a str,
    raw_src: &'a str,
    self_origin: &'a lumen_network::Origin,
    mode: lumen_layout::CrossOriginMode,
    base: &'a ResourceBase,
    sink: &'a Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
}

fn decode_image_cors(req: CorsImageFetch<'_>) -> Option<image_cache::DecodedImage> {
    let CorsImageFetch { resolved_url, raw_src, self_origin, mode, base, sink, cookie_jar, target } = req;
    use lumen_core::url::Url;
    let target_url = Url::parse(resolved_url).ok()?;
    let client = base.http_client_for_subresource(sink.clone(), cookie_jar);
    let credentials_mode = match mode {
        lumen_layout::CrossOriginMode::Anonymous => lumen_network::CredentialsMode::SameOrigin,
        lumen_layout::CrossOriginMode::UseCredentials => lumen_network::CredentialsMode::Include,
    };
    let request = lumen_network::CorsRequest {
        origin: self_origin.clone(),
        target: target_url,
        method: "GET".to_owned(),
        headers: Vec::new(),
        credentials_mode,
    };
    let bytes = match client.fetch_cors(request, Some(lumen_network::RequestDestination::Image)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("CORS-проверка картинки {raw_src} не прошла: {e}");
            return None;
        }
    };
    decode_image_bytes(raw_src, bytes, target)
}

/// Shared decode step for [`decode_image`] and [`decode_image_cors`] — the
/// two differ only in how `bytes` reached them (plain fetch vs CORS-checked).
fn decode_image_bytes(
    raw_src: &str,
    bytes: Vec<u8>,
    target: lumen_core::ColorSpace,
) -> Option<image_cache::DecodedImage> {
    use image_cache::DecodedImage;

    // Animated GIF detection: decode metadata lazily; keep the animation if >1 frame.
    if lumen_image::is_gif(&bytes) {
        return match lumen_image::decode_gif_animated(&bytes) {
            Ok(gif) if gif.frame_count() > 1 => {
                // BUG-272 срез 19: only the first frame is materialised eagerly.
                match gif.frame_image(0) {
                    Ok(first) => {
                        eprintln!(
                            "Загружена GIF-анимация: {} ({}×{}, {} кадров)",
                            raw_src, gif.width, gif.height, gif.frame_count()
                        );
                        Some(DecodedImage::Animated {
                            first: Arc::new(first),
                            gif: Arc::new(gif),
                        })
                    }
                    Err(e) => {
                        eprintln!("Не декодируется GIF {raw_src}: {e}");
                        None
                    }
                }
            }
            Ok(gif) => {
                // Single-frame GIF: treat as static image.
                gif.frame_image(0).ok().map(|image| {
                    eprintln!(
                        "Загружена картинка (GIF, 1 кадр): {} ({}×{})",
                        raw_src, image.width, image.height
                    );
                    DecodedImage::Static(Arc::new(image))
                })
            }
            Err(e) => {
                eprintln!("Не декодируется GIF {raw_src}: {e}");
                None
            }
        };
    }

    // LIB-4: SVG больше не особый случай — `decode_to` рисует его через resvg.
    match lumen_image::decode_to(&bytes, target) {
        Ok(image) => {
            eprintln!(
                "Загружена картинка: {} ({}×{}, {:?})",
                raw_src, image.width, image.height, image.format
            );
            Some(DecodedImage::Static(Arc::new(image)))
        }
        Err(e) => {
            eprintln!("Не декодируется {raw_src}: {e}");
            None
        }
    }
}

// BUG-430: `apply_intrinsic_size` переехала в `lumen-layout` (`box_tree.rs`),
// к picker-у `collect_image_requests`, чей `ImageRequest` она и обслуживает —
// headless-драйверу нужна та же логика заполнения слотов `width`/`height`, а
// дублировать её (спец-правило BUG-269 про aspect ratio) означало бы два
// расходящихся набора размеров у оконного и офлайн-путей.

/// PH3-19: дескриптор @font-face url()-источника, ещё не загруженного в память.
/// Хранится в `ParsedPage` / `LoadedPage`; `apply_loaded_page` спавнит
/// фоновый поток fetch+decode для каждого, результат — `LoadEvent::FontLoaded`.
pub(crate) struct PendingWebFont {
    /// CSS `font-family` дескриптор.
    pub(crate) family: String,
    /// Разрешённый font-weight (400 = normal, 700 = bold).
    pub(crate) weight: u16,
    /// Разрешённый font-style.
    pub(crate) style: lumen_core::FontStyle,
    /// Сырая строка `unicode-range` дескриптора (None → покрывает все кодпоинты).
    pub(crate) unicode_range_str: Option<String>,
    /// Сырая строка `ascent-override` дескриптора (CSS Fonts L4 §14,
    /// FONTLOAD-11) — распаршивается в фоновом потоке fetch-а, тем же
    /// приёмом, что уже применяется к `unicode_range_str`.
    pub(crate) ascent_override_str: Option<String>,
    /// Сырая строка `descent-override` дескриптора, та же семантика.
    pub(crate) descent_override_str: Option<String>,
    /// Сырая строка `size-adjust` дескриптора (CSS Fonts L4 §14.4,
    /// FONTLOAD-12), та же семантика/точка разбора, что у override-строк.
    pub(crate) size_adjust_str: Option<String>,
    /// Сырая строка `line-gap-override` дескриптора (CSS Fonts L4 §14.3,
    /// FONTLOAD-13), та же семантика, что у `ascent_override_str`.
    pub(crate) line_gap_override_str: Option<String>,
    /// Сырая строка `font-variation-settings` дескриптора (CSS Fonts L4
    /// §6.2, FONTLOAD-20) — распаршивается в фоновом потоке fetch-а через
    /// `lumen_font::parse_variation_settings`, той же точкой, что и
    /// `unicode_range_str`.
    pub(crate) variation_settings_str: Option<String>,
    /// URL для fetch (@font-face `src: url(...)`).
    pub(crate) url: String,
}

/// PH3-19: web-шрифт, уже загруженный и декодированный после `FontLoaded`.
/// Список хранится в `Lumen::web_fonts` и используется для пересборки
/// `MultiFontMeasurer` при каждом relayout — иначе resize/scroll-reflow
/// теряет web-метрики и откатывается к Inter.
// weight/style хранятся для будущего CSS font-matching (по weight/style дескрипторам @font-face).
// Clone: ADR-016 M2.2 — off-thread relayout захватывает владеющий снимок web-шрифтов.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct LoadedWebFont {
    /// CSS `font-family` дескриптор.
    pub(crate) family: String,
    /// Разрешённый font-weight.
    pub(crate) weight: u16,
    /// Разрешённый font-style.
    pub(crate) style: lumen_core::FontStyle,
    /// Диапазоны Unicode из @font-face `unicode-range` дескриптора.
    pub(crate) unicode_range: Vec<lumen_font::UnicodeRange>,
    /// `ascent-override` дескриптор (CSS Fonts L4 §14, FONTLOAD-11) — доля
    /// `font-size`, `None` — `normal`/отсутствует.
    pub(crate) ascent_override: Option<f32>,
    /// `descent-override` дескриптор, та же семантика.
    pub(crate) descent_override: Option<f32>,
    /// `size-adjust` дескриптор (CSS Fonts L4 §14.4, FONTLOAD-12) — доля,
    /// на которую масштабируется `font-size` этого face-а, `None` = `100%`.
    pub(crate) size_adjust: Option<f32>,
    /// `line-gap-override` дескриптор (CSS Fonts L4 §14.3, FONTLOAD-13) — та
    /// же семантика, что `ascent_override`.
    pub(crate) line_gap_override: Option<f32>,
    /// Декодированные sfnt-байты (TrueType / OTF после WOFF/WOFF2-распаковки).
    pub(crate) bytes: Vec<u8>,
}
