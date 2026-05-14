//! Picker для `<picture>` элемента и одиночного `<img>` с `srcset` (HTML5
//! §4.8.4.4 «The picture element» + §4.8.4.3.7 «Updating the source set»).
//!
//! Алгоритм для `<picture>` (упрощённый):
//!  1. Walk дочерних узлов picture-элемента в source-order.
//!  2. Для каждого `<source>`:
//!     * пропустить, если у него нет атрибута `srcset`;
//!     * пропустить, если `type` задан и не входит в `supported_types`;
//!     * пропустить, если `media` задан и не матчит viewport;
//!     * иначе попытаться выбрать кандидата из `srcset` + `sizes`. Если
//!       picker отдал URL — это финальный результат.
//!  3. Если ни один `<source>` не подошёл — fallback на первый `<img>`
//!     ребёнок: `pick_img_source` поверх его `srcset`/`sizes`/`src`.
//!  4. Если ни `<source>`, ни `<img>` не дают результата — `None`.
//!
//! Phase 0: `media`-парсер у нас разделяет тот же AST, что и для
//! `sizes`-атрибута (см. [`crate::srcset::parse_media_condition`]) —
//! поддерживаются `(min-width: ...)`, `(max-width: ...)`, `(min-height: ...)`,
//! `(max-height: ...)`, `(orientation: portrait|landscape)` и AND-список
//! через ` and `. Это покрывает подавляющее большинство `<source media>`
//! в дикой природе; полный media-query parser CSS-уровня — отдельная
//! задача, когда упрёмся в `prefers-color-scheme` / `not` / `only`.
//!
//! Замечания:
//!  * picker НЕ резолвит URL относительно base — это работа shell-а
//!    (`Url::resolve`). Возвращаем строку как-есть из атрибута;
//!  * picker не делает DOM-обход глубже первого уровня детей `<picture>`:
//!    `<source>`/`<img>` по HTML5 спеке должны быть прямыми детьми;
//!  * если `srcset` атрибут есть, но в нём нет валидных кандидатов
//!    (например, пустая строка / только запятые) — `<source>` молча
//!    пропускается, **не** считается, что он «нашёл pick=None и
//!    выпал из дальнейшей цепочки» — это даёт шанс следующему `<source>`
//!    или fallback `<img>`. Поведение лояльнее spec, но удобнее на
//!    реальных страницах с ошибками разметки.

use lumen_dom::{Document, NodeData, NodeId};

use crate::srcset::{
    SizesViewport, SrcsetCandidate, SrcsetDescriptor, evaluate_sizes, parse_media_condition,
    parse_sizes, parse_srcset, pick_best_for_density, pick_best_for_width,
};

/// Финальный URL выбранного источника. Дополнительные поля (intrinsic
/// dimensions, MIME) добавим, когда понадобятся paint-pipeline-у.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedSource {
    pub url: String,
}

/// Параметры picker-а.
#[derive(Debug, Clone, Copy)]
pub struct PictureParams<'a> {
    /// Текущий viewport — для резолва `sizes`-атрибута и `media`-условий
    /// у `<source>`.
    pub viewport: SizesViewport,
    /// Device pixel ratio (типично 1.0 / 1.5 / 2.0 / 3.0).
    pub dpr: f32,
    /// Список MIME-типов, которые движок умеет декодировать. `None` —
    /// фильтр отключён (все `<source type=...>` принимаются). При `Some`
    /// сравнение ASCII case-insensitive.
    pub supported_types: Option<&'a [&'a str]>,
}

impl Default for PictureParams<'_> {
    fn default() -> Self {
        Self {
            viewport: SizesViewport::DEFAULT,
            dpr: 1.0,
            supported_types: None,
        }
    }
}

/// Выбрать источник для `<picture>` элемента. См. модульный заголовок.
///
/// Если `picture_node` не указывает на `<picture>` элемент — возвращает
/// `None` (защита от misuse).
pub fn pick_picture_source(
    document: &Document,
    picture_node: NodeId,
    params: &PictureParams,
) -> Option<PickedSource> {
    let node = document.get(picture_node);
    if !element_local_name_eq(node, "picture") {
        return None;
    }

    // Первый pass — пробуем каждый <source> в source-order.
    for &child_id in &node.children {
        let child = document.get(child_id);
        if !element_local_name_eq(child, "source") {
            continue;
        }
        if let Some(picked) = try_pick_source(document, child_id, params) {
            return Some(picked);
        }
    }

    // Второй pass — fallback на <img>. По спеке — последний или один из
    // детей; lenient: первый встретившийся.
    for &child_id in &node.children {
        let child = document.get(child_id);
        if element_local_name_eq(child, "img") {
            return pick_img_source(document, child_id, params.viewport, params.dpr);
        }
    }

    None
}

/// Выбрать источник для одиночного `<img>` элемента (`srcset` + `sizes` +
/// `src`). Полезно, когда `<img>` стоит вне `<picture>`.
///
/// Алгоритм:
///  1. Если `srcset` есть и в нём есть валидные кандидаты — picker по
///     descriptor-у: width-picker если хоть один Nw-кандидат, иначе
///     density-picker. Sizes используется только в width-picker-е;
///     отсутствие sizes даёт fallback `100vw`.
///  2. Если srcset-кандидат не выбрался (нет кандидатов или picker отдал
///     None) — fallback на атрибут `src`. Пустой `src` → `None`.
///  3. Без обоих атрибутов — `None`.
///
/// Если `img_node` не указывает на `<img>` — `None`.
pub fn pick_img_source(
    document: &Document,
    img_node: NodeId,
    viewport: SizesViewport,
    dpr: f32,
) -> Option<PickedSource> {
    let node = document.get(img_node);
    if !element_local_name_eq(node, "img") {
        return None;
    }

    if let Some(srcset) = node.get_attr("srcset")
        && let Some(picked) = pick_from_srcset(srcset, node.get_attr("sizes"), viewport, dpr)
    {
        return Some(picked);
    }

    let src = node.get_attr("src")?.trim();
    if src.is_empty() {
        return None;
    }
    Some(PickedSource {
        url: src.to_string(),
    })
}

/// Попытаться выбрать URL из одного `<source>` элемента.
/// Возвращает `None`, если source отфильтрован (type / media) или srcset
/// не дал кандидата.
fn try_pick_source(
    document: &Document,
    source_node: NodeId,
    params: &PictureParams,
) -> Option<PickedSource> {
    let node = document.get(source_node);
    let srcset = node.get_attr("srcset")?;

    if let Some(ty) = node.get_attr("type")
        && !type_is_supported(ty, params.supported_types)
    {
        return None;
    }

    if let Some(media) = node.get_attr("media")
        && !parse_media_condition(media).matches(params.viewport)
    {
        return None;
    }

    pick_from_srcset(srcset, node.get_attr("sizes"), params.viewport, params.dpr)
}

/// Picker для пары (srcset, sizes). Width-picker используется, если есть
/// хоть один `Nw`-кандидат (mixed Nw+Nx нарушают spec, но мы lenient —
/// density-кандидаты будут проигнорированы width-picker-ом); иначе
/// density-picker.
fn pick_from_srcset(
    srcset: &str,
    sizes: Option<&str>,
    viewport: SizesViewport,
    dpr: f32,
) -> Option<PickedSource> {
    let candidates = parse_srcset(srcset);
    if candidates.is_empty() {
        return None;
    }
    let has_width = candidates
        .iter()
        .any(|c: &SrcsetCandidate| matches!(c.descriptor, SrcsetDescriptor::Width(_)));
    let picked = if has_width {
        let source_size_px = match sizes {
            Some(s) => evaluate_sizes(&parse_sizes(s), viewport),
            None => viewport.width_px, // HTML5 §4.8.4.4 default = 100vw
        };
        pick_best_for_width(&candidates, source_size_px, dpr)
    } else {
        pick_best_for_density(&candidates, dpr)
    };
    picked.map(|c| PickedSource {
        url: c.url.clone(),
    })
}

/// `type` matcher. `None` в supported_types — фильтр отключён; иначе
/// ASCII case-insensitive lookup. Пустая `type` строка (после trim)
/// трактуется как «отсутствует» (= match-everything).
fn type_is_supported(ty: &str, supported: Option<&[&str]>) -> bool {
    let Some(list) = supported else {
        return true;
    };
    let trimmed = ty.trim();
    if trimmed.is_empty() {
        return true;
    }
    list.iter().any(|s| s.eq_ignore_ascii_case(trimmed))
}

/// ASCII case-insensitive проверка local name HTML-элемента.
fn element_local_name_eq(node: &lumen_dom::Node, want: &str) -> bool {
    match &node.data {
        NodeData::Element { name, .. } => name.local.eq_ignore_ascii_case(want),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_builder::parse;
    use lumen_dom::Document;

    // ──────── helpers ────────

    /// Найти первый элемент с указанным local name в документе.
    fn first_element(doc: &Document, local: &str) -> Option<NodeId> {
        let mut stack: Vec<NodeId> = vec![doc.root()];
        while let Some(id) = stack.pop() {
            let node = doc.get(id);
            if element_local_name_eq(node, local) {
                return Some(id);
            }
            for &c in node.children.iter().rev() {
                stack.push(c);
            }
        }
        None
    }

    fn viewport_1024() -> SizesViewport {
        SizesViewport {
            width_px: 1024.0,
            height_px: 768.0,
            root_font_size_px: 16.0,
            prefers_dark: false,
        }
    }

    fn viewport_500() -> SizesViewport {
        SizesViewport {
            width_px: 500.0,
            height_px: 800.0,
            root_font_size_px: 16.0,
            prefers_dark: false,
        }
    }

    fn params(viewport: SizesViewport, dpr: f32) -> PictureParams<'static> {
        PictureParams {
            viewport,
            dpr,
            supported_types: None,
        }
    }

    // ──────── pick_img_source ────────

    #[test]
    fn img_with_src_only() {
        let doc = parse(r#"<img src="cat.png">"#);
        let id = first_element(&doc, "img").unwrap();
        let picked = pick_img_source(&doc, id, viewport_1024(), 1.0).unwrap();
        assert_eq!(picked.url, "cat.png");
    }

    #[test]
    fn img_with_srcset_density_chooses_best() {
        let doc = parse(r#"<img srcset="lo.png 1x, hi.png 2x" src="fallback.png">"#);
        let id = first_element(&doc, "img").unwrap();
        let picked = pick_img_source(&doc, id, viewport_1024(), 2.0).unwrap();
        assert_eq!(picked.url, "hi.png");
    }

    #[test]
    fn img_srcset_width_with_sizes() {
        // viewport=1024, sizes="100vw" → source-size=1024. dpr=1.
        // effective: 480w/1024 = 0.47, 1024w/1024 = 1.0, 2048w/1024 = 2.0.
        // smallest sufficient >= 1.0 → 1024w.
        let doc = parse(
            r#"<img srcset="s.png 480w, m.png 1024w, l.png 2048w" sizes="100vw" src="x.png">"#,
        );
        let id = first_element(&doc, "img").unwrap();
        let picked = pick_img_source(&doc, id, viewport_1024(), 1.0).unwrap();
        assert_eq!(picked.url, "m.png");
    }

    #[test]
    fn img_srcset_width_no_sizes_defaults_to_100vw() {
        // Без sizes → source_size = viewport.width = 1024.
        let doc = parse(r#"<img srcset="s.png 480w, l.png 1024w" src="x.png">"#);
        let id = first_element(&doc, "img").unwrap();
        let picked = pick_img_source(&doc, id, viewport_1024(), 1.0).unwrap();
        assert_eq!(picked.url, "l.png");
    }

    #[test]
    fn img_srcset_empty_falls_back_to_src() {
        // Только запятые → нет валидных кандидатов; должны откатиться на src.
        let doc = parse(r#"<img srcset=",,," src="fallback.png">"#);
        let id = first_element(&doc, "img").unwrap();
        let picked = pick_img_source(&doc, id, viewport_1024(), 1.0).unwrap();
        assert_eq!(picked.url, "fallback.png");
    }

    #[test]
    fn img_no_src_no_srcset_returns_none() {
        let doc = parse(r#"<img alt="no source">"#);
        let id = first_element(&doc, "img").unwrap();
        assert!(pick_img_source(&doc, id, viewport_1024(), 1.0).is_none());
    }

    #[test]
    fn img_empty_src_returns_none() {
        let doc = parse(r#"<img src="">"#);
        let id = first_element(&doc, "img").unwrap();
        assert!(pick_img_source(&doc, id, viewport_1024(), 1.0).is_none());
    }

    #[test]
    fn img_misuse_on_non_img_returns_none() {
        let doc = parse(r#"<div src="x.png"></div>"#);
        let id = first_element(&doc, "div").unwrap();
        assert!(pick_img_source(&doc, id, viewport_1024(), 1.0).is_none());
    }

    // ──────── pick_picture_source: media фильтр ────────

    #[test]
    fn picture_media_matching_source_wins() {
        // viewport.width = 1024 → min-width:600 матчит, min-width:1200 нет.
        let html = r#"
            <picture>
              <source media="(min-width: 1200px)" srcset="huge.png">
              <source media="(min-width: 600px)" srcset="medium.png">
              <img src="small.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "medium.png");
    }

    #[test]
    fn picture_no_media_match_falls_back_to_img() {
        let html = r#"
            <picture>
              <source media="(min-width: 2000px)" srcset="huge.png">
              <source media="(min-width: 1500px)" srcset="medium.png">
              <img src="small.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "small.png");
    }

    #[test]
    fn picture_source_without_media_always_matches() {
        // Source без media считается всегда подходящим. Source-order: первый
        // подходящий выигрывает.
        let html = r#"
            <picture>
              <source srcset="any.png">
              <source media="(min-width: 600px)" srcset="medium.png">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "any.png");
    }

    #[test]
    fn picture_media_orientation_portrait() {
        // viewport_500 — высокий: 500×800, портрет.
        let html = r#"
            <picture>
              <source media="(orientation: portrait)" srcset="portrait.png">
              <source media="(orientation: landscape)" srcset="landscape.png">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_500(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "portrait.png");
    }

    // ──────── pick_picture_source: type фильтр ────────

    #[test]
    fn picture_type_filter_skips_unsupported() {
        // webp в supported_types нет → этот source скипается.
        let html = r#"
            <picture>
              <source type="image/webp" srcset="hero.webp">
              <source type="image/jpeg" srcset="hero.jpg">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let supported = ["image/jpeg", "image/png"];
        let p = PictureParams {
            viewport: viewport_1024(),
            dpr: 1.0,
            supported_types: Some(&supported),
        };
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "hero.jpg");
    }

    #[test]
    fn picture_type_filter_disabled_takes_first() {
        // supported_types=None → фильтр отключён, type-атрибут игнорится.
        let html = r#"
            <picture>
              <source type="image/webp" srcset="hero.webp">
              <source type="image/jpeg" srcset="hero.jpg">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "hero.webp");
    }

    #[test]
    fn picture_type_filter_case_insensitive() {
        let html = r#"
            <picture>
              <source type="Image/JPEG" srcset="hero.jpg">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let supported = ["image/jpeg"];
        let p = PictureParams {
            viewport: viewport_1024(),
            dpr: 1.0,
            supported_types: Some(&supported),
        };
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "hero.jpg");
    }

    // ──────── pick_picture_source: srcset интеграция ────────

    #[test]
    fn picture_source_uses_density_picker() {
        let html = r#"
            <picture>
              <source srcset="lo.png 1x, hi.png 2x">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 2.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "hi.png");
    }

    #[test]
    fn picture_source_uses_width_picker_with_sizes() {
        // sizes=50vw, viewport=1024 → source-size=512. dpr=1.
        // effective: 480w/512≈0.94, 1024w/512=2.0. smallest sufficient >= 1.0 = 1024w.
        let html = r#"
            <picture>
              <source srcset="s.png 480w, l.png 1024w" sizes="50vw">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "l.png");
    }

    #[test]
    fn picture_source_without_srcset_skipped() {
        // Source без srcset не считается кандидатом, идём к следующему.
        let html = r#"
            <picture>
              <source media="(min-width: 600px)" type="image/jpeg">
              <source media="(min-width: 600px)" srcset="real.jpg">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "real.jpg");
    }

    // ──────── pick_picture_source: fallback и edge cases ────────

    #[test]
    fn picture_no_img_no_match_returns_none() {
        let html = r#"
            <picture>
              <source media="(min-width: 5000px)" srcset="huge.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        assert!(pick_picture_source(&doc, id, &p).is_none());
    }

    #[test]
    fn picture_only_img_child() {
        let html = r#"<picture><img src="only.png"></picture>"#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "only.png");
    }

    #[test]
    fn picture_misuse_on_non_picture_returns_none() {
        let doc = parse(r#"<div><source srcset="x.png"><img src="y.png"></div>"#);
        let id = first_element(&doc, "div").unwrap();
        let p = params(viewport_1024(), 1.0);
        assert!(pick_picture_source(&doc, id, &p).is_none());
    }

    #[test]
    fn picture_falls_back_to_img_srcset_if_present() {
        // У <img> своя srcset + sizes — должна работать даже без <source>.
        let html = r#"
            <picture>
              <img srcset="lo.png 1x, hi.png 2x" src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 2.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "hi.png");
    }

    #[test]
    fn picture_first_source_in_order_wins_on_tie() {
        // Оба source матчат — берётся первый по порядку.
        let html = r#"
            <picture>
              <source media="(min-width: 100px)" srcset="first.png">
              <source media="(min-width: 100px)" srcset="second.png">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "first.png");
    }

    #[test]
    fn picture_source_invalid_media_never_matches() {
        // Не закрытая скобка → Unsupported, никогда не матчит. Следующий
        // valid source с матчем выигрывает.
        let html = r#"
            <picture>
              <source media="(min-width broken" srcset="broken.png">
              <source srcset="ok.png">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "ok.png");
    }

    #[test]
    fn picture_media_and_type_both_required() {
        // media матчит, но type — нет. Skip и идём к следующему.
        let html = r#"
            <picture>
              <source media="(min-width: 600px)" type="image/avif" srcset="hero.avif">
              <source media="(min-width: 600px)" type="image/jpeg" srcset="hero.jpg">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let supported = ["image/jpeg"];
        let p = PictureParams {
            viewport: viewport_1024(),
            dpr: 1.0,
            supported_types: Some(&supported),
        };
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "hero.jpg");
    }

    #[test]
    fn picture_empty_type_attribute_treated_as_absent() {
        // type="" → match-everything (даже с фильтром).
        let html = r#"
            <picture>
              <source type="" srcset="ok.png">
              <img src="x.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let supported = ["image/jpeg"]; // не содержит "" — но empty проходит
        let p = PictureParams {
            viewport: viewport_1024(),
            dpr: 1.0,
            supported_types: Some(&supported),
        };
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "ok.png");
    }

    // ──────── picture с text-узлами между source/img ────────

    #[test]
    fn picture_with_text_children_between_sources() {
        // Whitespace и текстовые узлы между source / img не должны мешать.
        let html = r#"
            <picture>
              text1
              <source media="(min-width: 600px)" srcset="medium.png">
              text2
              <img src="small.png">
            </picture>
        "#;
        let doc = parse(html);
        let id = first_element(&doc, "picture").unwrap();
        let p = params(viewport_1024(), 1.0);
        let picked = pick_picture_source(&doc, id, &p).unwrap();
        assert_eq!(picked.url, "medium.png");
    }

    // ──────── PictureParams::default ────────

    #[test]
    fn default_params_use_desktop_viewport() {
        let p = PictureParams::default();
        assert!((p.viewport.width_px - 1024.0).abs() < f32::EPSILON);
        assert!((p.dpr - 1.0).abs() < f32::EPSILON);
        assert!(p.supported_types.is_none());
    }
}
