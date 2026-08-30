use super::*;

/// Запрос на предзагрузку изображения: URL после picking-а по
/// `<picture>`/`srcset`/`sizes` плюс признаки явного задания размеров
/// author-ом (нужны shell для `apply_intrinsic_size`).
pub struct ImageRequest {
    pub node_id: NodeId,
    pub url: String,
    pub has_explicit_width: bool,
    pub has_explicit_height: bool,
    /// `loading="lazy"` (HTML LS §2.6.6.9): defer fetch until element is near viewport.
    /// Shell skips eager fetch and instead registers the image for IntersectionObserver
    /// proximity check; loaded once the element scrolls within one viewport of the fold.
    pub is_lazy: bool,
    /// `fetchpriority` (HTML LS §2.5.7): нормализованное `"high"`/`"low"`;
    /// `auto`, мусор и отсутствие атрибута → `None`.
    pub fetch_priority: Option<String>,
}

/// Обходит DOM и возвращает запросы на загрузку для всех `<img>`-элементов.
/// URL выбирается через тот же picker, что layout использует при построении
/// `BoxKind::Image { src }` — гарантирует совпадение ключей в
/// `Renderer::register_image` и `DisplayCommand::DrawImage.src`.
pub fn collect_image_requests(doc: &Document, viewport: Size) -> Vec<ImageRequest> {
    let mut out = Vec::new();
    collect_requests_inner(doc, doc.root(), viewport, &mut out);
    out
}

/// Обходит готовое layout-дерево и возвращает уникальные URL-ы из
/// `background-image: url(...)` (CSS Backgrounds L3 §3.10) — те же ключи,
/// что эмиттер кладёт в `DisplayCommand::DrawBackgroundImage.src`.
///
/// Background-image не участвует в расчёте размеров, поэтому собирается
/// уже после layout — shell вызывает функцию между layout-ом и paint-ом,
/// дозагружает байты и регистрирует через `Renderer::register_image`.
///
/// Возвращает `Vec<String>` (а не `Vec<ImageRequest>`): для background-image
/// нет node-anchored intrinsic-size hint-ов (CSS Backgrounds L3 §3.9 говорит
/// о `background-size` в стилях, intrinsic-размер картинки в layout не
/// влияет). Дубликаты отфильтрованы — одна и та же картинка на разных
/// элементах загружается один раз.
///
/// `dpr` — device pixel ratio, по которому разрешается `image-set()`
/// (CSS Images L4 §5). Значение **обязано** совпадать с тем, что получит
/// `build_display_list_ordered_dpr`: эмиттер кладёт в `src` уже выбранного
/// кандидата, и ключ загрузки должен быть тем же. `1.0` — дефолт
/// `build_display_list_ordered`.
#[must_use]
pub fn collect_background_image_requests(root: &LayoutBox, dpr: f32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_bg_image_inner(root, dpr, &mut out);
    out
}

/// Кладёт в `out` URL-ы, под которыми эмиттер будет искать картинки слоя.
///
/// `image-set()` хранится в слое дословно, а в display list попадает уже
/// выбранный кандидат — поэтому здесь функция разворачивается, иначе shell
/// уходил бы качать текст `image-set(…)` как имя файла. `cross-fade()` рисуется
/// одной командой из **двух** источников, и обе стороны (каждая сама может быть
/// `image-set()`) должны быть загружены. Пустые и уже собранные URL-ы
/// пропускаются.
fn push_bg_image_urls(image: &BackgroundImage, dpr: f32, out: &mut Vec<String>) {
    match image {
        BackgroundImage::Url(src) => {
            let resolved = if crate::image_set::is_image_set(src) {
                crate::image_set::select_image_set_url(src, dpr)
            } else {
                src.clone()
            };
            if !resolved.is_empty() && !out.contains(&resolved) {
                out.push(resolved);
            }
        }
        // CSS Images L4 §4 — обе стороны попадают в `DrawCrossFade`.
        BackgroundImage::CrossFade { a, b, .. } => {
            push_bg_image_urls(a, dpr, out);
            push_bg_image_urls(b, dpr, out);
        }
        _ => {}
    }
}

fn collect_bg_image_inner(b: &LayoutBox, dpr: f32, out: &mut Vec<String>) {
    for layer in &b.style.background_layers {
        push_bg_image_urls(&layer.image, dpr, out);
    }
    // CSS Lists L3 §2.3: a `list-style-image` marker also needs its URL fetched
    // and registered, same as a background image.
    if let BoxKind::Marker { image: Some(src), .. } = &b.kind
        && !src.is_empty()
        && !out.iter().any(|u| u == src)
    {
        out.push(src.clone());
    }
    // CSS Generated Content L3 §2.1: `content: url(...)` produces an inline-replaced
    // image segment that the shell would otherwise never fetch — unlike `<img>`, it
    // has no DOM element for `collect_image_requests` to walk. Such segments are
    // tagged with `source_node == NodeId::from_index(0)` ("no DOM origin"), which
    // distinguishes them from real inline `<img>` frags (already fetched, and
    // possibly `loading="lazy"`). Piggy-back on the post-layout background pass.
    if let BoxKind::InlineRun { segments, .. } = &b.kind {
        for seg in segments {
            if let Some(src) = &seg.img_src
                && seg.source_node == NodeId::from_index(0)
                && !src.is_empty()
                && !out.iter().any(|u| u == src)
            {
                out.push(src.clone());
            }
        }
    }
    for child in &b.children {
        collect_bg_image_inner(child, dpr, out);
    }
}

/// Доставляет intrinsic-размеры декодированной картинки в layout, дописывая
/// `<img>` пустые презентационные атрибуты `width`/`height`.
///
/// Возвращает `true`, если атрибут действительно был дописан — то есть DOM
/// изменился и странице нужен релейаут. `false` (ничего не изменилось) — когда
/// автор задал оба размера сам или размеры уже дописаны прошлым вызовом; на нём
/// держится сходимость повторного прохода `Lumen::apply_stream_intrinsic_sizes`
/// (BUG-735), иначе «применили → релейаут → применили» зациклилось бы.
///
/// Живёт рядом с [`collect_image_requests`] (BUG-430): и шелл, и headless-драйвер
/// сначала берут URL у picker-а, потом сообщают размеры декодированной картинки
/// обратно в DOM — правило заполнения слотов обязано быть у обоих одно.
pub fn apply_intrinsic_size(doc: &mut Document, node_id: NodeId, width: u32, height: u32) -> bool {
    use lumen_dom::{Attribute, QualName};
    let NodeData::Element { attrs, .. } = &mut doc.get_mut(node_id).data else {
        return false;
    };
    // Presence of the author's width/height content attributes (any value —
    // including percentages — counts as "set" and must never be duplicated).
    let has_w = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("width"));
    let has_h = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("height"));
    // Author values parsed as non-negative integer px (HTML dimension-attr
    // grammar). `None` when absent OR non-integer (e.g. `width="50%"`).
    let attr_w = attrs
        .iter()
        .find(|a| a.name.local.eq_ignore_ascii_case("width"))
        .and_then(|a| a.value.trim().parse::<u32>().ok());
    let attr_h = attrs
        .iter()
        .find(|a| a.name.local.eq_ignore_ascii_case("height"))
        .and_then(|a| a.value.trim().parse::<u32>().ok());

    // BUG-269: fill the missing dimension from the intrinsic aspect ratio
    // (CSS 2.1 §10.6.2) rather than from the raw intrinsic value, so a
    // fixed-width `<img width="240">` (intrinsic 120×80) becomes 240×160, not
    // 240×80 — and, crucially, is not left with a collapsed `height: auto` = 0.
    // Push only into empty attribute slots (presentational hint, specificity 0
    // — authored CSS still wins). A pixel-parsed author dimension drives the
    // ratio; a non-integer one (percentage) falls back to the raw intrinsic
    // value for the other axis.
    let (new_w, new_h) = match (attr_w, attr_h) {
        (Some(w), None) => {
            let h = if width > 0 {
                ((w as u64 * height as u64 + width as u64 / 2) / width as u64) as u32
            } else {
                height
            };
            (None, Some(h))
        }
        (None, Some(h)) => {
            let w = if height > 0 {
                ((h as u64 * width as u64 + height as u64 / 2) / height as u64) as u32
            } else {
                width
            };
            (Some(w), None)
        }
        // Both integers set, or one/both present but non-integer: fill any
        // still-empty slot with the raw intrinsic value.
        _ => (
            (!has_w).then_some(width),
            (!has_h).then_some(height),
        ),
    };

    let mut changed = false;
    if !has_w && let Some(w) = new_w {
        attrs.push(Attribute {
            name: QualName::html("width"),
            value: w.to_string(),
        });
        changed = true;
    }
    if !has_h && let Some(h) = new_h {
        attrs.push(Attribute {
            name: QualName::html("height"),
            value: h.to_string(),
        });
        changed = true;
    }
    changed
}

fn collect_requests_inner(doc: &Document, id: NodeId, viewport: Size, out: &mut Vec<ImageRequest>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "img"
    {
        let has_explicit_width = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("width"));
        let has_explicit_height =
            attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("height"));
        let is_lazy = attrs.iter().any(|a| {
            a.name.local.eq_ignore_ascii_case("loading")
                && a.value.as_str().eq_ignore_ascii_case("lazy")
        });
        // HTML LS §2.5.7: нормализация fetchpriority — только "high"/"low".
        let fetch_priority = attrs
            .iter()
            .find(|a| a.name.local.eq_ignore_ascii_case("fetchpriority"))
            .map(|a| a.value.trim().to_ascii_lowercase())
            .filter(|v| v == "high" || v == "low");
        let source = resolve_image_source(doc, id, viewport);
        if !source.url.is_empty() {
            out.push(ImageRequest {
                node_id: id,
                url: source.url,
                has_explicit_width,
                has_explicit_height,
                is_lazy,
                fetch_priority,
            });
        }
        return; // void element — нет children
    }
    // BUG-848: three more elements carry an image URL the display list will
    // key on but never produced a request at all — `<video poster>`, an
    // `<input type=image>` control and an SVG `<image>` (href/xlink:href).
    // None of the three support srcset/loading/fetchpriority, so the request
    // just carries the "not set" defaults for those fields.
    if let Some(url) = image_subresource_url(node) {
        out.push(ImageRequest {
            node_id: id,
            url,
            has_explicit_width: false,
            has_explicit_height: false,
            is_lazy: false,
            fetch_priority: None,
        });
    }
    for &child in &node.children {
        collect_requests_inner(doc, child, viewport, out);
    }
}

/// URL for the three BUG-848 element kinds that carry an image but are not
/// `<img>`: `<video poster>`, `<input type=image src>`, SVG `<image
/// href|xlink:href>`. `None` for every other element, or when the relevant
/// attribute is absent/empty — same "nothing to fetch" rule `<img>` uses.
fn image_subresource_url(node: &lumen_dom::Node) -> Option<String> {
    let name = node.element_name()?;
    let url = match name.local.as_str() {
        "video" => node.get_attr("poster"),
        "input" if node.input_type() == Some(lumen_dom::InputType::Image) => node.get_attr("src"),
        // SVG `<image>`; legacy `xlink:href` (SVG 1.1) alongside the plain
        // `href` this parser keeps as one attribute, same fallback `<use>`
        // resolution already uses a few lines up.
        "image" => node.get_attr("href").or_else(|| node.get_attr("xlink:href")),
        _ => None,
    }?;
    (!url.is_empty()).then(|| url.to_string())
}

/// Выбрать источник для `<img>`-элемента с учётом окружающего контекста:
///  1. Если parent — `<picture>`, прогоняем picture-picker
///     (выбирает `<source>` или fallback на `<img>` по `media`/`type`/
///     `srcset`/`sizes`).
///  2. Иначе — `<img>`-picker, учитывающий собственный `srcset`/`sizes`/`src`.
///  3. Если оба picker-а вернули `None` (нет ни `srcset`, ни `src`) —
///     fallback на голый `src` атрибут как раньше: для битой разметки
///     лучше отрисовать пустую коробку, чем ничего.
///
/// Phase 0: DPR=1.0 (layout не знает про device pixel ratio renderer-а —
/// это интегрирует P3 при relayout-on-resize), `prefers_dark` = false.
/// `supported_types` заполняется из `lumen_image::supported_mime_types()`:
/// picker пропускает `<source type="image/webp">` и аналогичные пока
/// неподдерживаемые форматы вместо того чтобы выбирать их и показывать пустую коробку.
pub(crate) fn resolve_image_source(doc: &Document, img_id: NodeId, viewport: Size) -> ImageSource {
    let sizes_vp = SizesViewport {
        width_px: viewport.width,
        height_px: viewport.height,
        root_font_size_px: 16.0,
        prefers_dark: false,
    };
    let params = PictureParams {
        viewport: sizes_vp,
        dpr: 1.0,
        supported_types: Some(lumen_image::supported_mime_types()),
    };

    if let Some(parent_id) = doc.get(img_id).parent
        && is_picture_element(doc, parent_id)
        && let Some(picked) = pick_picture_source(doc, parent_id, &params)
    {
        return ImageSource {
            url: picked.url,
            intrinsic_width: picked.intrinsic_width,
            intrinsic_height: picked.intrinsic_height,
        };
    }

    if let Some(picked) = pick_img_source(doc, img_id, sizes_vp, params.dpr) {
        return ImageSource {
            url: picked.url,
            intrinsic_width: picked.intrinsic_width,
            intrinsic_height: picked.intrinsic_height,
        };
    }

    let raw_src = doc.get(img_id).get_attr("src").unwrap_or("").to_string();
    ImageSource { url: raw_src, intrinsic_width: None, intrinsic_height: None }
}
