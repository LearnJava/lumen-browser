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
pub(crate) fn fetch_and_decode_background_images(
    layout: &LayoutBox,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
) -> Vec<(String, Arc<lumen_image::Image>)> {
    // DPR 1.0 — тот же, что у `build_display_list_ordered` (обёртка без dpr),
    // иначе выбранный здесь кандидат `image-set()` не совпал бы с ключом,
    // который эмиттер кладёт в `DrawBackgroundImage.src`.
    let urls = lumen_layout::collect_background_image_requests(layout, 1.0);
    // Параллельная загрузка+декодирование, порядок сохраняем (ключи уникальны).
    let decoded = parallel_map(&urls, |_, url| {
        let bytes = match fetch_image_bytes(url, base, sink, cookie_jar.clone()) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Пропуск bg-картинки {url}: {e}");
                return None;
            }
        };
        // LIB-4: SVG больше не особый случай — `decode_to` рисует его через
        // resvg наравне с любым растровым форматом.
        let image = match lumen_image::decode_to(&bytes, target) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Не декодируется bg-картинка {url}: {e}");
                return None;
            }
        };
        eprintln!(
            "Загружена bg-картинка: {url} ({}×{}, {:?})",
            image.width, image.height, image.format
        );
        // BUG-272 срез 17: wrap once in Arc so `register_image` shares the buffer.
        Some((url.clone(), Arc::new(image)))
    });
    decoded.into_iter().flatten().collect()
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
                    registry.register_from_bytes(&rule.family, weight, style, &ranges, bytes);
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
                url: url_src.value.clone(),
            });
        }
    }

    (registry, pending)
}

/// Парсит `font-weight` дескриптор @font-face: ключевые слова + числа.
/// Диапазоны (`400 700`) — берём первое значение. Default: 400.
fn parse_font_weight(s: Option<&str>) -> u16 {
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
/// Возвращает `(images, animated_gifs, lazy_pairs)`:
/// - `images` — декодированные картинки для немедленной регистрации в renderer-е
///   (включает frame 0 каждого анимированного GIF);
/// - `animated_gifs` — многокадровые GIF-анимации для тиканья в `RedrawRequested`;
/// - `lazy_pairs` — `(node_id_u32, url)` для `<img loading="lazy">`, которые
///   не загружаются сейчас и будут зарегистрированы через `_lumen_init_lazy_images`.
#[allow(clippy::type_complexity)]
pub(crate) fn fetch_and_decode_images(
    doc: &mut Document,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    viewport: lumen_core::geom::Size,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
    target: lumen_core::ColorSpace,
) -> (Vec<(String, Arc<lumen_image::Image>)>, Vec<(String, lumen_image::AnimatedGif)>, Vec<(u32, String)>) {
    let requests = lumen_layout::collect_image_requests(doc, viewport);

    /// Результат параллельной фазы fetch+decode одной картинки. Применение к
    /// документу (intrinsic size) и сборка выходных векторов — отдельной
    /// последовательной фазой, чтобы порядок и `&mut doc` остались под контролем.
    enum ImgOutcome {
        /// `loading="lazy"` — отложить до приближения к вьюпорту.
        Lazy,
        /// Пропуск (ошибка сети/декодирования) — уже залогировано.
        Skip,
        /// Статическая картинка (включая 1-кадровый GIF). `Arc<Image>` (BUG-272
        /// срез 17): разделяет аллокацию пикселей с `IMAGE_CACHE`, а не копирует.
        Static {
            image: Arc<lumen_image::Image>,
            /// Intrinsic-размеры для HTML-атрибутов, если их не задал автор.
            intrinsic: Option<(u32, u32)>,
        },
        /// Многокадровый GIF: первый кадр + полная анимация.
        Animated {
            first: Arc<lumen_image::Image>,
            gif: lumen_image::AnimatedGif,
            intrinsic: Option<(u32, u32)>,
        },
    }

    // Фаза 1 (параллельно): сеть + декодирование. Не трогаем `doc`.
    // BUG-172: декод идёт через `IMAGE_CACHE` — картинки, уже загруженные
    // прогрессивным streaming-проходом (`spawn_stream_image_loads`), берутся из
    // кэша без повторного fetch+decode; их `wants_intrinsic`/`is_lazy` решаются
    // здесь, а пиксели только клонируются.
    let outcomes = parallel_map(&requests, |_, req| {
        if req.is_lazy {
            return ImgOutcome::Lazy;
        }
        // BUG-269: apply intrinsic size whenever the author left AT LEAST ONE
        // dimension unset (not only when BOTH are unset). A replaced element
        // with a fixed width and `height: auto` must derive its height from the
        // intrinsic aspect ratio (CSS 2.1 §10.6.2); `apply_intrinsic_size` fills
        // the missing slot from that ratio.
        let wants_intrinsic = !(req.has_explicit_width && req.has_explicit_height);
        let decoded = image_cache::IMAGE_CACHE.get_or_decode_current(&req.url, || {
            decode_image(&req.url, base, sink, cookie_jar.clone(), target)
        });
        match decoded {
            None => ImgOutcome::Skip,
            Some(image_cache::DecodedImage::Static(img)) => {
                // BUG-272 срез 17: share the cache's Arc, not a pixel copy.
                let intrinsic = wants_intrinsic.then_some((img.width, img.height));
                ImgOutcome::Static { image: img, intrinsic }
            }
            Some(image_cache::DecodedImage::Animated { first, gif }) => {
                let intrinsic = wants_intrinsic.then_some((first.width, first.height));
                ImgOutcome::Animated { first, gif: (*gif).clone(), intrinsic }
            }
        }
    });

    // Фаза 2 (последовательно): мутация `doc` + сборка результата в порядке DOM.
    let mut out: Vec<(String, Arc<lumen_image::Image>)> = Vec::new();
    let mut anim_gifs: Vec<(String, lumen_image::AnimatedGif)> = Vec::new();
    let mut lazy_pairs: Vec<(u32, String)> = Vec::new();
    for (req, outcome) in requests.into_iter().zip(outcomes) {
        match outcome {
            ImgOutcome::Lazy => lazy_pairs.push((req.node_id.index() as u32, req.url)),
            ImgOutcome::Skip => {}
            ImgOutcome::Static { image, intrinsic } => {
                if let Some((w, h)) = intrinsic {
                    apply_intrinsic_size(doc, req.node_id, w, h);
                }
                out.push((req.url, image));
            }
            ImgOutcome::Animated { first, gif, intrinsic } => {
                if let Some((w, h)) = intrinsic {
                    apply_intrinsic_size(doc, req.node_id, w, h);
                }
                out.push((req.url.clone(), first));
                anim_gifs.push((req.url, gif));
            }
        }
    }
    (out, anim_gifs, lazy_pairs)
}

pub(crate) fn fetch_image_bytes(
    raw_src: &str,
    base: &ResourceBase,
    sink: &Arc<dyn EventSink>,
    cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    match base.resolve(raw_src) {
        ResolvedResource::File(path) => std::fs::read(&path).map_err(|e| {
            format!("file://{} {e}", path.display()).into()
        }),
        ResolvedResource::Url(url) => {
            use lumen_core::url::Url;
            use lumen_network::RequestDestination;

            // Images are loaded in no-cors mode: cross-origin allowed, but
            // mixed-content enforcement still applies for HTTPS pages.
            let lumen_url = Url::parse(&url)?;
            let client = base.http_client_for_subresource(sink.clone(), cookie_jar);
            // PERF-1: one span per image fetch — back-to-back spans on a lane
            // reveal sequential UI-thread subresource loading.
            let mut fetch_span = lumen_core::trace::span(format!("img {url}"), "net");
            let bytes = client.fetch_subresource(&lumen_url, RequestDestination::Image)?;
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
    use image_cache::DecodedImage;
    let bytes = match fetch_image_bytes(raw_src, base, sink, cookie_jar) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Пропуск картинки {raw_src}: {e}");
            return None;
        }
    };

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
    /// Декодированные sfnt-байты (TrueType / OTF после WOFF/WOFF2-распаковки).
    pub(crate) bytes: Vec<u8>,
}
