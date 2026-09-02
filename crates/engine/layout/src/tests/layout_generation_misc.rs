use super::*;

// ── CSS Grid L2 Subgrid ───────────────────────────────────────────────────

/// `grid-template-columns: subgrid` parses to the sentinel `[Subgrid]`.
#[test]
fn grid_subgrid_parse_columns() {
    let root = lay(
        "<body><div id='g'><div id='sg'></div></div></body>",
        "#g { display: grid; grid-template-columns: 100px 200px; } \
         #sg { grid-template-columns: subgrid; }",
    );
    let grid = first_element_child(&root);
    let subgrid = first_element_child(grid);
    assert_eq!(subgrid.style.grid_template_columns.len(), 1);
    assert_eq!(subgrid.style.grid_template_columns[0], GridTrackSize::Subgrid);
}

/// `grid-template-rows: subgrid` parses to the sentinel `[Subgrid]`.
#[test]
fn grid_subgrid_parse_rows() {
    let root = lay(
        "<body><div id='g'><div id='sg'></div></div></body>",
        "#g { display: grid; grid-template-rows: 50px 100px; } \
         #sg { grid-template-rows: subgrid; }",
    );
    let grid = first_element_child(&root);
    let subgrid = first_element_child(grid);
    assert_eq!(subgrid.style.grid_template_rows.len(), 1);
    assert_eq!(subgrid.style.grid_template_rows[0], GridTrackSize::Subgrid);
}

/// A subgrid item spanning 2 columns inherits those column widths from the parent.
/// Two items inside the subgrid are placed in the inherited columns (100px + 200px).
#[test]
fn grid_subgrid_column_layout() {
    let root = lay(
        "<body>\
           <div id='g'>\
             <div id='sg'>\
               <span id='a'></span>\
               <span id='b'></span>\
             </div>\
           </div>\
         </body>",
        r#"
        body { width: 400px; }
        #g {
            display: grid;
            grid-template-columns: 100px 200px;
            grid-template-rows: 50px;
            width: 300px;
        }
        #sg {
            display: grid;
            grid-template-columns: subgrid;
            grid-column: 1 / 3;
        }
        #a { height: 30px; }
        #b { height: 30px; }
        "#,
    );
    let grid = first_element_child(&root);
    // The subgrid item spans both columns → width = 300px.
    let sg = first_element_child(grid);
    assert!(
        (sg.rect.width - 300.0).abs() < 2.0,
        "subgrid width should be ~300, got {}",
        sg.rect.width
    );
    // Items inside subgrid are placed in the inherited 100px and 200px columns.
    let items: Vec<_> = sg.children.iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 2, "expected 2 items in subgrid");
    let a = &items[0];
    let b = &items[1];
    // a in col 1 (x=0, w=100), b in col 2 (x=100, w=200).
    assert!((a.rect.x - sg.rect.x).abs() < 2.0, "a.x rel={}", a.rect.x - sg.rect.x);
    assert!((a.rect.width - 100.0).abs() < 2.0, "a.w={}", a.rect.width);
    assert!((b.rect.x - sg.rect.x - 100.0).abs() < 2.0, "b.x rel={}", b.rect.x - sg.rect.x);
    assert!((b.rect.width - 200.0).abs() < 2.0, "b.w={}", b.rect.width);
}

/// `collect_subgrid_items` finds both column-subgrid and row-subgrid containers.
#[test]
fn grid_collect_subgrid_items() {
    use crate::subgrid::collect_subgrid_items;
    let root = lay(
        "<body>\
           <div id='g'>\
             <div id='col_sg'></div>\
             <div id='row_sg'></div>\
             <div id='both_sg'></div>\
             <div id='normal'></div>\
           </div>\
         </body>",
        r#"
        #g { display: grid; grid-template-columns: 100px 200px; grid-template-rows: 50px 50px; }
        #col_sg { grid-template-columns: subgrid; grid-column: 1 / 3; }
        #row_sg { grid-template-rows: subgrid; grid-row: 1 / 3; }
        #both_sg { grid-template-columns: subgrid; grid-template-rows: subgrid; }
        "#,
    );
    let items = collect_subgrid_items(&root);
    // col_sg, row_sg, both_sg should appear; normal should not.
    assert_eq!(items.len(), 3, "expected 3 subgrid items, got {:?}", items.len());
    let col_sg = items.iter().find(|it| it.subgrid_columns && !it.subgrid_rows);
    assert!(col_sg.is_some(), "missing col-subgrid item");
    let row_sg = items.iter().find(|it| it.subgrid_rows && !it.subgrid_columns);
    assert!(row_sg.is_some(), "missing row-subgrid item");
    let both_sg = items.iter().find(|it| it.subgrid_columns && it.subgrid_rows);
    assert!(both_sg.is_some(), "missing both-subgrid item");
}

// ── collect_image_requests ────────────────────────────────────────────────

fn vp() -> Size {
    Size::new(800.0, 600.0)
}

/// Обычный `<img src>` → один запрос с тем же URL.
#[test]
fn collect_plain_img_src() {
    let doc = lumen_html_parser::parse(r#"<body><img src="photo.jpg"></body>"#);
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "photo.jpg");
    assert!(!reqs[0].has_explicit_width);
    assert!(!reqs[0].has_explicit_height);
}

/// `<img src width height>` → has_explicit_width/height == true.
#[test]
fn collect_img_with_explicit_dims() {
    let doc = lumen_html_parser::parse(
        r#"<body><img src="a.png" width="100" height="50"></body>"#,
    );
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].has_explicit_width);
    assert!(reqs[0].has_explicit_height);
}

/// Пустой `src` → запрос не включается.
#[test]
fn collect_img_empty_src_skipped() {
    let doc = lumen_html_parser::parse(r#"<body><img src=""></body>"#);
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 0);
}

/// `<img>` без `src` → запрос не включается.
#[test]
fn collect_img_no_src_skipped() {
    let doc = lumen_html_parser::parse(r#"<body><img alt="no src"></body>"#);
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 0);
}

/// `<img srcset="a.png 1x, b.png 2x">` → DPR=1.0 → первый кандидат.
#[test]
fn collect_img_srcset_picks_first_at_dpr1() {
    let doc = lumen_html_parser::parse(
        r#"<body><img srcset="a.png 1x, b.png 2x" src="fallback.png"></body>"#,
    );
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 1);
    // DPR=1.0 → picker выберет "a.png 1x"
    assert_eq!(reqs[0].url, "a.png");
}

/// `<picture><source srcset="hd.webp"><img src="sd.jpg"></picture>` →
/// picker выбирает source-кандидата (нет атрибута type → тип неизвестен, не фильтруется).
#[test]
fn collect_picture_source_wins_over_img_src() {
    let doc = lumen_html_parser::parse(
        r#"<body><picture><source srcset="hd.webp"><img src="sd.jpg"></picture></body>"#,
    );
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "hd.webp");
}

/// `<picture><source type="image/heic" srcset="hero.heic"><img src="hero.jpg"></picture>` →
/// heic нет в `supported_mime_types()` → picker пропускает source → fallback на `<img src>`.
#[test]
fn collect_picture_unsupported_type_falls_back() {
    let doc = lumen_html_parser::parse(concat!(
        r#"<body><picture>"#,
        r#"<source type="image/heic" srcset="hero.heic">"#,
        r#"<img src="hero.jpg">"#,
        r#"</picture></body>"#,
    ));
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 1, "должен быть один запрос — fallback PNG/JPEG");
    assert_eq!(reqs[0].url, "hero.jpg", "heic source скипается, выбирается img src");
}

/// `<picture>` с первым поддерживаемым `<source type="image/webp">` →
/// picker выбирает этот source (webp теперь декодируется), а не img src.
#[test]
fn collect_picture_supported_type_picked() {
    let doc = lumen_html_parser::parse(concat!(
        r#"<body><picture>"#,
        r#"<source type="image/webp" srcset="hero.webp">"#,
        r#"<source type="image/jpeg" srcset="hero.jpg">"#,
        r#"<img src="fallback.png">"#,
        r#"</picture></body>"#,
    ));
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].url, "hero.webp", "первый поддерживаемый source — WebP");
}

/// Несколько `<img>` → несколько запросов.
#[test]
fn collect_multiple_images() {
    let doc = lumen_html_parser::parse(
        r#"<body><img src="a.png"><img src="b.jpg"></body>"#,
    );
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 2);
    let urls: Vec<&str> = reqs.iter().map(|r| r.url.as_str()).collect();
    assert!(urls.contains(&"a.png"));
    assert!(urls.contains(&"b.jpg"));
}

// ── collect_background_image_requests ────────────────────────────────────

fn layout_with(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    layout(&doc, &sheet, vp())
}

/// `background-image: url(...)` на блоке → один URL в результате.
#[test]
fn collect_bg_image_single_block() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; background-image: url(bg.png); }",
    );
    let urls = collect_background_image_requests(&root, 1.0);
    assert_eq!(urls, vec!["bg.png".to_string()]);
}

/// `background-image: none` (initial) → пустой результат.
#[test]
fn collect_bg_image_none_skipped() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; background-image: none; }",
    );
    assert!(collect_background_image_requests(&root, 1.0).is_empty());
}

/// Gradient-вариант не учитывается (Phase 0 не растрит).
#[test]
fn collect_bg_image_gradient_skipped() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; \
         background-image: linear-gradient(red, blue); }",
    );
    assert!(collect_background_image_requests(&root, 1.0).is_empty());
}

/// Дубликаты URL фильтруются.
#[test]
fn collect_bg_image_dedupes() {
    let root = layout_with(
        "<body><div></div><div></div><div></div></body>",
        "div { width: 10px; height: 10px; background-image: url(same.png); }",
    );
    let urls = collect_background_image_requests(&root, 1.0);
    assert_eq!(urls.len(), 1, "three divs same URL → один запрос, got {urls:?}");
    assert_eq!(urls[0], "same.png");
}

/// Разные URL → собираются в порядке обхода.
#[test]
fn collect_bg_image_multiple_distinct() {
    let root = layout_with(
        r#"<body><div class="a"></div><div class="b"></div></body>"#,
        ".a { width: 10px; height: 10px; background-image: url(a.png); } \
         .b { width: 10px; height: 10px; background-image: url(b.png); }",
    );
    let urls = collect_background_image_requests(&root, 1.0);
    assert_eq!(urls.len(), 2);
    assert!(urls.contains(&"a.png".to_string()));
    assert!(urls.contains(&"b.png".to_string()));
}

// ── BUG-101: image-set() / cross-fade() как источники запросов ────────────

/// BUG-101: `image-set()` хранится в слое дословно, а эмиттер кладёт в
/// `DrawBackgroundImage.src` уже выбранного кандидата. Коллектор обязан
/// вернуть тот же URL, иначе shell качает текст функции как имя файла
/// (os error 123) и картинка не рисуется.
#[test]
fn collect_bg_image_resolves_image_set_candidate() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; background-image: \
         image-set(url(one.png) 1x, url(two.png) 2x); }",
    );
    let urls = collect_background_image_requests(&root, 1.0);
    assert_eq!(urls, vec!["one.png".to_string()], "DPR 1 → 1x-кандидат, got {urls:?}");
}

/// Тот же слой при DPR 2 даёт 2x-кандидата — коллектор и эмиттер должны
/// разрешать `image-set()` по одному и тому же DPR.
#[test]
fn collect_bg_image_image_set_follows_dpr() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; background-image: \
         image-set(url(one.png) 1x, url(two.png) 2x); }",
    );
    let urls = collect_background_image_requests(&root, 2.0);
    assert_eq!(urls, vec!["two.png".to_string()], "DPR 2 → 2x-кандидат, got {urls:?}");
}

/// `-webkit-image-set()` разворачивается так же, как беспрефиксная форма.
#[test]
fn collect_bg_image_resolves_webkit_image_set() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; background-image: \
         -webkit-image-set(url(low.png) 1x, url(high.png) 2x); }",
    );
    let urls = collect_background_image_requests(&root, 1.0);
    assert_eq!(urls, vec!["low.png".to_string()]);
}

/// BUG-101: `cross-fade()` рисуется одной командой из двух источников —
/// раньше не собиралась ни одна сторона, и ячейка оставалась пустой.
#[test]
fn collect_bg_image_cross_fade_both_sides() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; background-image: \
         -webkit-cross-fade(url(from.png), url(to.png), 50%); }",
    );
    let urls = collect_background_image_requests(&root, 1.0);
    assert_eq!(urls.len(), 2, "обе стороны cross-fade, got {urls:?}");
    assert!(urls.contains(&"from.png".to_string()));
    assert!(urls.contains(&"to.png".to_string()));
}

/// Сторона `cross-fade()` сама может быть `image-set()` — разворачивается
/// рекурсивно, тем же DPR.
#[test]
fn collect_bg_image_cross_fade_side_image_set() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; background-image: \
         -webkit-cross-fade(image-set(url(a1.png) 1x, url(a2.png) 2x), url(b.png), 50%); }",
    );
    let urls = collect_background_image_requests(&root, 1.0);
    assert_eq!(urls.len(), 2, "got {urls:?}");
    assert!(urls.contains(&"a1.png".to_string()), "1x-сторона image-set, got {urls:?}");
    assert!(urls.contains(&"b.png".to_string()));
}

/// Беспрефиксная 3-аргументная `cross-fade()` невалидна (CSS Images L4 §4),
/// декларация отбрасывается — собирать нечего.
#[test]
fn collect_bg_image_unprefixed_legacy_cross_fade_collects_nothing() {
    let root = layout_with(
        "<body><div></div></body>",
        "div { width: 50px; height: 50px; background-image: \
         cross-fade(url(from.png), url(to.png), 30%); }",
    );
    assert!(
        collect_background_image_requests(&root, 1.0).is_empty(),
        "невалидная декларация не должна порождать запросов"
    );
}

// ── CSS Generated Content L3 §2.1 — content: url() ────────────────────────

/// Собирает пары `(text, img_src)` из всех `InlineRun`-сегментов дерева.
fn inline_segments_of(b: &LayoutBox) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    fn walk(b: &LayoutBox, out: &mut Vec<(String, Option<String>)>) {
        if let crate::box_tree::BoxKind::InlineRun { segments, .. } = &b.kind {
            for s in segments {
                out.push((s.text.clone(), s.img_src.clone()));
            }
        }
        for c in &b.children {
            walk(c, out);
        }
    }
    walk(b, &mut out);
    out
}

/// `content: url(...)` на `::before` → генерирует inline-replaced image-сегмент.
#[test]
fn content_url_before_emits_image_segment() {
    let root = layout_with(
        "<body><p>x</p></body>",
        "p::before { content: url(icon.png); }",
    );
    let segs = inline_segments_of(&root);
    assert!(
        segs.iter().any(|(t, img)| img.as_deref() == Some("icon.png") && t.is_empty()),
        "expected a generated image segment for icon.png, got {segs:?}"
    );
}

/// Сгенерированная `content: url(...)` картинка попадает в фетч-запросы: у неё
/// нет DOM-элемента, поэтому обычный `collect_image_requests` её не видит —
/// её подбирает post-layout background-проход.
#[test]
fn collect_bg_image_generated_content_url() {
    let root = layout_with(
        "<body><p>x</p></body>",
        "p::before { content: url(icon.png); }",
    );
    let urls = collect_background_image_requests(&root, 1.0);
    assert_eq!(urls, vec!["icon.png".to_string()]);
}

/// Реальный inline `<img>` НЕ попадает в background-проход: у него есть DOM-узел,
/// его грузит `collect_image_requests`. Двойной фетч (и поломка `loading=lazy`)
/// недопустимы — сегменты `<img>` несут собственный `NodeId`, а не sentinel-0.
#[test]
fn collect_bg_image_ignores_real_img() {
    let root = layout_with(
        r#"<body><p><img src="real.png"></p></body>"#,
        "",
    );
    assert!(
        collect_background_image_requests(&root, 1.0).is_empty(),
        "real <img> must not be collected by the background pass"
    );
}

/// Смешанный `content: "A" url(i.png) "B"` → текст «A», картинка i.png, текст «B»
/// как отдельные сегменты (url() разрывает текстовый run).
#[test]
fn content_url_mixed_with_text_splits_segments() {
    let root = layout_with(
        "<body><p>x</p></body>",
        r#"p::before { content: "A" url(i.png) "B"; }"#,
    );
    let segs = inline_segments_of(&root);
    assert!(segs.iter().any(|(t, img)| t == "A" && img.is_none()), "text A missing: {segs:?}");
    assert!(
        segs.iter().any(|(t, img)| img.as_deref() == Some("i.png") && t.is_empty()),
        "image i.png missing: {segs:?}"
    );
    assert!(segs.iter().any(|(t, img)| t == "B" && img.is_none()), "text B missing: {segs:?}");
}

// ── CSS Positioned Layout L3 — position: relative / absolute / fixed ──

/// `position: relative; top: 20px; left: 30px` — визуальный сдвиг относительно
/// нормального потока; высота родителя не меняется.
#[test]
fn position_relative_offset() {
    let root = lay(
        "<div class='outer'><div class='inner'>x</div></div>",
        ".outer { width: 200px; height: 100px; }
         .inner { position: relative; top: 20px; left: 30px; }",
    );
    let outer = first_element_child(&root);
    let inner = first_element_child(outer);
    // Нормальная позиция inner без offset: x=0, y=0 (нет margin/padding).
    // С relative offset: y += 20, x += 30.
    assert_eq!(inner.rect.x, 30.0, "relative left");
    assert_eq!(inner.rect.y, 20.0, "relative top");
    // Родительская высота не изменяется (relative не влияет на flow).
    assert_eq!(outer.rect.height, 100.0, "outer height unchanged");
}

/// `position: relative; bottom: 10px; right: 15px` — отрицательный сдвиг.
#[test]
fn position_relative_bottom_right() {
    let root = lay(
        "<div class='inner'>x</div>",
        ".inner { position: relative; bottom: 10px; right: 15px; }",
    );
    let inner = first_element_child(&root);
    // bottom: 10px → y -= 10 (сдвиг вверх)
    assert_eq!(inner.rect.y, -10.0, "relative bottom moves up");
    // right: 15px → x -= 15 (сдвиг влево)
    assert_eq!(inner.rect.x, -15.0, "relative right moves left");
}

/// `position: absolute; top: 10px; left: 20px` внутри positioned parent.
/// Абсолютный элемент не участвует в normal flow (высота родителя = 0).
#[test]
fn position_absolute_top_left() {
    let root = lay(
        "<div class='parent'><div class='abs'>x</div></div>",
        ".parent { position: relative; width: 400px; height: 300px; }
         .abs    { position: absolute; top: 10px; left: 20px; width: 50px; }",
    );
    let parent = first_element_child(&root);
    let abs_child = first_element_child(parent);
    // Positioned relative to parent's border-edge box.
    assert_eq!(abs_child.rect.x, 20.0, "abs left");
    assert_eq!(abs_child.rect.y, 10.0, "abs top");
    // Ширина задана явно.
    assert_eq!(abs_child.rect.width, 50.0, "abs explicit width");
}

/// `position: absolute; bottom: 0; right: 0` — правый нижний угол контейнера.
#[test]
fn position_absolute_bottom_right() {
    let root = lay(
        "<div class='parent'><div class='abs'>x</div></div>",
        ".parent { position: relative; width: 400px; height: 300px; }
         .abs    { position: absolute; bottom: 0px; right: 0px; width: 60px; height: 40px; }",
    );
    let parent = first_element_child(&root);
    let abs_child = first_element_child(parent);
    // right: 0 → right edge of abs = right edge of parent (400)
    // abs.rect.x = 400 - 0 - 60 = 340
    assert_eq!(abs_child.rect.x, 340.0, "abs right=0 positions at right edge");
    // bottom: 0 → bottom edge of abs = bottom edge of parent (300)
    // abs.rect.y = 300 - 0 - 40 = 260
    assert_eq!(abs_child.rect.y, 260.0, "abs bottom=0 positions at bottom edge");
}

/// `position: absolute` без explicit containing block — используется viewport.
#[test]
fn position_absolute_uses_viewport_without_positioned_ancestor() {
    let root = lay(
        "<div><div class='abs'>x</div></div>",
        ".abs { position: absolute; top: 50px; left: 100px; width: 80px; }",
    );
    // Родитель static — CB = viewport (800×600)
    let parent = first_element_child(&root);
    let abs_child = first_element_child(parent);
    assert_eq!(abs_child.rect.y, 50.0, "abs top from viewport");
    assert_eq!(abs_child.rect.x, 100.0, "abs left from viewport");
}

/// Абсолютный элемент не влияет на высоту normal-flow родителя.
#[test]
fn position_absolute_excluded_from_normal_flow() {
    let root = lay(
        "<div class='parent'>
           <div class='normal' style='height: 40px;'></div>
           <div class='abs' style='height: 200px;'></div>
         </div>",
        ".parent { position: relative; }
         .abs    { position: absolute; top: 0; left: 0; }",
    );
    let parent = first_element_child(&root);
    // Только normal-flow div (height=40) считается в высоту родителя.
    assert_eq!(parent.rect.height, 40.0, "abs child excluded from parent height");
}

/// `position: fixed; top: 0; right: 0` — position relative to viewport.
#[test]
fn position_fixed_relative_to_viewport() {
    let root = lay(
        "<div class='parent'><div class='fix'>x</div></div>",
        ".parent { position: relative; width: 400px; height: 300px; margin: 50px; }
         .fix    { position: fixed; top: 5px; right: 10px; width: 80px; }",
    );
    let parent = first_element_child(&root);
    let fix_child = first_element_child(parent);
    // Fixed: CB = viewport (800×600), not parent
    assert_eq!(fix_child.rect.y, 5.0, "fixed top from viewport");
    // right: 10 → x = viewport.width - 10 - 80 = 710
    assert_eq!(fix_child.rect.x, 710.0, "fixed right from viewport");
}

/// `inset` shorthand: `inset: 10px 20px 30px 40px` → top/right/bottom/left.
#[test]
fn inset_shorthand_four_values() {
    let root = lay(
        "<div class='parent'><div class='abs'></div></div>",
        ".parent { position: relative; width: 400px; height: 300px; }
         .abs    { position: absolute; inset: 10px 20px 30px 40px; }",
    );
    let parent = first_element_child(&root);
    let abs_child = first_element_child(parent);
    // top: 10, left: 40
    assert_eq!(abs_child.rect.y, 10.0, "inset top");
    assert_eq!(abs_child.rect.x, 40.0, "inset left");
}

/// `position: relative; top: auto; left: auto` — никакого сдвига.
#[test]
fn position_relative_all_auto_no_offset() {
    let root = lay(
        "<div class='outer'><div class='inner'>x</div></div>",
        ".outer { width: 200px; }
         .inner { position: relative; top: auto; left: auto; }",
    );
    let outer = first_element_child(&root);
    let inner = first_element_child(outer);
    assert_eq!(inner.rect.x, 0.0, "no x offset");
    assert_eq!(inner.rect.y, 0.0, "no y offset");
}

// ── UA stylesheet ──────────────────────────────────────────────────────

fn first_seg_style(p: &LayoutBox) -> ComputedStyle {
    let run = first_inline_run(p);
    if let BoxKind::InlineRun { segments, .. } = &run.kind {
        segments[0].style.clone()
    } else {
        panic!("expected InlineRun with segments");
    }
}

#[test]
fn ua_del_text_decoration_line_through() {
    let root = lay("<p><del>x</del></p>", "");
    let p = first_element_child(&root);
    let style = first_seg_style(p);
    assert!(style.text_decoration_line.line_through, "del → line-through");
    assert!(!style.text_decoration_line.underline, "del → no underline");
}

#[test]
fn ua_s_text_decoration_line_through() {
    let root = lay("<p><s>x</s></p>", "");
    let p = first_element_child(&root);
    let style = first_seg_style(p);
    assert!(style.text_decoration_line.line_through, "s → line-through");
}

#[test]
fn ua_ins_text_decoration_underline() {
    let root = lay("<p><ins>x</ins></p>", "");
    let p = first_element_child(&root);
    let style = first_seg_style(p);
    assert!(style.text_decoration_line.underline, "ins → underline");
    assert!(!style.text_decoration_line.line_through, "ins → no line-through");
}

#[test]
fn ua_a_href_link_color_and_underline() {
    let root = lay(r#"<p><a href="http://example.com">link</a></p>"#, "");
    let p = first_element_child(&root);
    let style = first_seg_style(p);
    assert_eq!(
        style.color,
        Color { r: 0, g: 0, b: 238, a: 255 },
        "a[href] → #0000ee"
    );
    assert!(style.text_decoration_line.underline, "a[href] → underline");
}

#[test]
fn ua_sub_vertical_align_and_font_size() {
    let root = lay("<p><sub>x</sub></p>", "");
    let p = first_element_child(&root);
    let style = first_seg_style(p);
    assert_eq!(style.vertical_align, VerticalAlign::Sub, "sub → VerticalAlign::Sub");
    assert!(
        (style.font_size - 16.0 * 0.83).abs() < 0.01,
        "sub → 83% font-size, got {}",
        style.font_size
    );
}

#[test]
fn ua_sup_vertical_align_and_font_size() {
    let root = lay("<p><sup>x</sup></p>", "");
    let p = first_element_child(&root);
    let style = first_seg_style(p);
    assert_eq!(style.vertical_align, VerticalAlign::Super, "sup → VerticalAlign::Super");
    assert!(
        (style.font_size - 16.0 * 0.83).abs() < 0.01,
        "sup → 83% font-size, got {}",
        style.font_size
    );
}

#[test]
fn ua_small_font_size() {
    let root = lay("<p><small>x</small></p>", "");
    let p = first_element_child(&root);
    let style = first_seg_style(p);
    assert!(
        (style.font_size - 16.0 * 0.83).abs() < 0.01,
        "small → 83% font-size, got {}",
        style.font_size
    );
}

// ──────── ::before / ::after pseudo-element generation ──────────────────

fn first_seg_text(b: &LayoutBox) -> String {
    match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            segments.first().map(|s| s.text.clone()).unwrap_or_default()
        }
        _ => String::new(),
    }
}

#[test]
fn before_pseudo_string_content() {
    // ::before content вставляется как первый сегмент InlineRun.
    let root = lay("<p>Hello</p>", r#"p::before { content: ">> "; }"#);
    let p = first_element_child(&root);
    assert!(!p.children.is_empty(), "p must have children");
    let first = &p.children[0];
    assert!(
        matches!(first.kind, BoxKind::InlineRun { .. }),
        "first child must be InlineRun, got {:?}",
        std::mem::discriminant(&first.kind)
    );
    let text = first_seg_text(first);
    assert!(
        text.starts_with(">> "),
        "::before text should start with '>> ', got {:?}",
        text
    );
}

#[test]
fn after_pseudo_string_content() {
    // ::after content вставляется как последний сегмент InlineRun.
    let root = lay("<p>Hello</p>", r#"p::after { content: " <<"; }"#);
    let p = first_element_child(&root);
    assert!(!p.children.is_empty(), "p must have children");
    let last = p.children.last().unwrap();
    assert!(
        matches!(last.kind, BoxKind::InlineRun { .. }),
        "last child must be InlineRun"
    );
    if let BoxKind::InlineRun { segments, .. } = &last.kind {
        let last_seg = segments.last().unwrap();
        assert!(
            last_seg.text.ends_with(" <<"),
            "::after text should end with ' <<', got {:?}",
            last_seg.text
        );
    }
}

#[test]
fn before_and_after_together() {
    // ::before и ::after оба применяются.
    let root = lay(
        "<p>X</p>",
        r#"p::before { content: "["; } p::after { content: "]"; }"#,
    );
    let p = first_element_child(&root);
    // The p should have at least one InlineRun with all text.
    let all_text: String = p
        .children
        .iter()
        .flat_map(|c| {
            if let BoxKind::InlineRun { segments, .. } = &c.kind {
                segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>()
            } else {
                vec![]
            }
        })
        .collect();
    assert!(
        all_text.contains('[') && all_text.contains(']'),
        "expected '[' and ']' in inline text, got {:?}",
        all_text
    );
}

#[test]
fn before_content_none_generates_nothing() {
    // content: none → псевдоэлемент не генерируется.
    let root = lay("<p>X</p>", "p::before { content: none; }");
    let p = first_element_child(&root);
    // Только один InlineRun с текстом "X", без ::before.
    let inline_texts: Vec<String> = p
        .children
        .iter()
        .flat_map(|c| {
            if let BoxKind::InlineRun { segments, .. } = &c.kind {
                segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>()
            } else {
                vec![]
            }
        })
        .collect();
    assert!(
        inline_texts.iter().all(|t| !t.is_empty()),
        "no empty texts expected"
    );
    // Нет текста кроме "X".
    let all = inline_texts.join("");
    assert_eq!(all.trim(), "X", "got {:?}", all);
}

#[test]
fn before_pseudo_inherits_parent_color() {
    // ::before наследует color от родителя.
    let root = lay(
        "<p>X</p>",
        r#"p { color: red; } p::before { content: "•"; }"#,
    );
    let p = first_element_child(&root);
    // Первый InlineRun содержит сегмент от ::before.
    let first_run = p.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. }));
    let Some(run) = first_run else {
        panic!("no InlineRun found");
    };
    if let BoxKind::InlineRun { segments, .. } = &run.kind {
        let before_seg = segments.iter().find(|s| s.text == "•");
        let Some(seg) = before_seg else {
            panic!("no segment with '•' found");
        };
        // red = Color { r: 255, g: 0, b: 0, a: 255 }. Проверяем r > 0, g == 0.
        assert!(
            seg.style.color.r > 0 && seg.style.color.g == 0,
            "::before should inherit red color, got {:?}",
            seg.style.color
        );
    }
}

#[test]
fn before_pseudo_no_rules_no_box() {
    // Если нет правил для ::before — ничего не генерируется.
    let root = lay("<p>Hello</p>", "p { color: blue; }");
    let p = first_element_child(&root);
    // Только один InlineRun с "Hello".
    assert_eq!(p.children.len(), 1, "expected 1 child (InlineRun)");
    assert!(matches!(p.children[0].kind, BoxKind::InlineRun { .. }));
}

// ──────── inline ::before / ::after (collect_inline_segments path) ───────

#[test]
fn inline_before_pseudo_injects_segment_before_children() {
    // span::before { content: ">>"; } — сегмент ">>" перед текстом span.
    let root = lay(
        "<p><span>Hello</span></p>",
        r#"span::before { content: ">>"; }"#,
    );
    let p = first_element_child(&root);
    let run = p
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
        .expect("InlineRun expected");
    if let BoxKind::InlineRun { segments, .. } = &run.kind {
        let first = segments.first().expect("at least one segment");
        assert!(
            first.text.contains(">>"),
            "::before segment should be first, got {:?}",
            first.text
        );
    }
}

#[test]
fn inline_after_pseudo_injects_segment_after_children() {
    // span::after { content: "<<"; } — сегмент "<<" после текста span.
    let root = lay(
        "<p><span>Hello</span></p>",
        r#"span::after { content: "<<"; }"#,
    );
    let p = first_element_child(&root);
    let run = p
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
        .expect("InlineRun expected");
    if let BoxKind::InlineRun { segments, .. } = &run.kind {
        let last = segments.last().expect("at least one segment");
        assert!(
            last.text.contains("<<"),
            "::after segment should be last, got {:?}",
            last.text
        );
    }
}

#[test]
fn inline_before_after_order() {
    // span::before + ::after — порядок: before / span-text / after.
    let root = lay(
        "<p><span>X</span></p>",
        r#"span::before { content: "A"; } span::after { content: "B"; }"#,
    );
    let p = first_element_child(&root);
    let all_text: String = p
        .children
        .iter()
        .flat_map(|c| {
            if let BoxKind::InlineRun { segments, .. } = &c.kind {
                segments.iter().map(|s| s.text.clone()).collect::<Vec<_>>()
            } else {
                vec![]
            }
        })
        .collect();
    let a_pos = all_text.find('A').expect("A not found");
    let x_pos = all_text.find('X').expect("X not found");
    let b_pos = all_text.find('B').expect("B not found");
    assert!(a_pos < x_pos, "::before must precede span text");
    assert!(x_pos < b_pos, "::after must follow span text");
}

#[test]
fn inline_before_inherits_span_style() {
    // span::before наследует color от span.
    let root = lay(
        "<p><span>X</span></p>",
        r#"span { color: #ff0000; } span::before { content: "●"; }"#,
    );
    let p = first_element_child(&root);
    let run = p
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
        .expect("InlineRun");
    if let BoxKind::InlineRun { segments, .. } = &run.kind {
        let before = segments.iter().find(|s| s.text.contains('●')).expect("● not found");
        assert!(
            before.style.color.r > 0 && before.style.color.g == 0,
            "::before should inherit red color, got {:?}",
            before.style.color
        );
    }
}

#[test]
fn inline_before_display_block_skipped_in_inline_context() {
    // span::before { display: block } внутри inline-контекста — пропускается.
    let root = lay(
        "<p><span>Only</span></p>",
        r#"span::before { content: "X"; display: block; }"#,
    );
    let p = first_element_child(&root);
    let run = p
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
        .expect("InlineRun");
    if let BoxKind::InlineRun { segments, .. } = &run.kind {
        // Текст "X" не должен появиться — псевдо-элемент block в inline-контексте пропускается.
        let has_x = segments.iter().any(|s| s.text == "X");
        assert!(!has_x, "block ::before must be skipped in inline context");
    }
}

fn first_inline_run_frag(b: &LayoutBox) -> &InlineFrag {
    let run = b
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }))
        .expect("expected InlineRun child");
    match &run.kind {
        BoxKind::InlineRun { lines, .. } => &lines[0][0],
        _ => unreachable!(),
    }
}

#[test]
fn vertical_align_baseline_y_offset_half_leading() {
    // baseline — y_offset == half_leading = (line_h - font_size) / 2.
    // CSS 2.1 §10.8.1: content area is centred in line-box via half-leading.
    let root = lay_measured("<p>Hello</p>", "", 800.0);
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    let fs = frag.style.font_size;
    let line_h = fs * frag.style.line_height;
    let expected = ((line_h - fs) / 2.0).max(0.0);
    assert!(
        (frag.y_offset - expected).abs() < 0.01,
        "baseline y_offset must be half_leading={}, got {}",
        expected,
        frag.y_offset
    );
}

#[test]
fn vertical_align_middle_y_offset() {
    // middle → (line_h - font_size) / 2.
    let root = lay_measured(
        "<p><span>Hi</span></p>",
        "span { vertical-align: middle; }",
        800.0,
    );
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    let font_size = frag.style.font_size;
    let line_h = font_size * frag.style.line_height;
    let expected = ((line_h - font_size) / 2.0).max(0.0);
    assert!(
        (frag.y_offset - expected).abs() < 0.01,
        "middle y_offset: expected {}, got {}",
        expected,
        frag.y_offset
    );
}

#[test]
fn vertical_align_bottom_y_offset() {
    // bottom → line_h - font_size.
    let root = lay_measured(
        "<p><span>Hi</span></p>",
        "span { vertical-align: bottom; }",
        800.0,
    );
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    let font_size = frag.style.font_size;
    let line_h = font_size * frag.style.line_height;
    let expected = (line_h - font_size).max(0.0);
    assert!(
        (frag.y_offset - expected).abs() < 0.01,
        "bottom y_offset: expected {}, got {}",
        expected,
        frag.y_offset
    );
}

#[test]
fn vertical_align_length_shifts_up() {
    // vertical-align: 8px → y_offset = half_leading - 8px
    // (позитивная длина CSS = вверх от baseline = half_leading - 8).
    let root = lay_measured(
        "<p><span>Hi</span></p>",
        "span { vertical-align: 8px; }",
        800.0,
    );
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    let fs = frag.style.font_size;
    let line_h = fs * frag.style.line_height;
    let half_leading = ((line_h - fs) / 2.0).max(0.0);
    let expected = half_leading - 8.0;
    assert!(
        (frag.y_offset - expected).abs() < 0.01,
        "length 8px y_offset: expected {}, got {}",
        expected,
        frag.y_offset
    );
}

#[test]
fn vertical_align_super_negative_y_offset() {
    // super → y_offset < 0 (сдвиг вверх).
    let root = lay_measured("<p><sup>note</sup></p>", "", 800.0);
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    assert!(
        frag.y_offset < 0.0,
        "super y_offset must be negative, got {}",
        frag.y_offset
    );
}

#[test]
fn vertical_align_sub_positive_y_offset() {
    // sub → y_offset > 0 (сдвиг вниз).
    let root = lay_measured("<p><sub>note</sub></p>", "", 800.0);
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    assert!(
        frag.y_offset > 0.0,
        "sub y_offset must be positive, got {}",
        frag.y_offset
    );
}

// ── Half-leading (CSS 2.1 §10.8.1) ──────────────────────────────────────

#[test]
fn half_leading_baseline_centred_in_line_box() {
    // line-height: 2.0 → half_leading = (32 - 16) / 2 = 8px for 16px font.
    // Baseline фрагмента должен быть смещён на 8px вниз от верха строки.
    let root = lay_measured(
        "<p>Hello</p>",
        "p { line-height: 2.0; font-size: 16px; }",
        800.0,
    );
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    let expected_half_leading = 8.0_f32; // (32 - 16) / 2
    assert!(
        (frag.y_offset - expected_half_leading).abs() < 0.1,
        "half_leading with line-height:2: expected y_offset={}, got {}",
        expected_half_leading,
        frag.y_offset
    );
}

#[test]
fn half_leading_zero_when_line_height_equals_font_size() {
    // line-height: 1.0 → нет leading, y_offset = 0.
    let root = lay_measured(
        "<p>Hello</p>",
        "p { line-height: 1.0; font-size: 16px; }",
        800.0,
    );
    let p = first_element_child(&root);
    let frag = first_inline_run_frag(p);
    assert!(
        frag.y_offset.abs() < 0.001,
        "line-height:1.0 → no half-leading, expected y_offset=0, got {}",
        frag.y_offset
    );
}

#[test]
fn half_leading_line_box_height_correct() {
    // line-height: 1.5, font-size: 20px → line_h = 30px.
    // Высота InlineRun должна быть 30px.
    let root = lay_measured(
        "<p>Hello</p>",
        "p { line-height: 1.5; font-size: 20px; }",
        800.0,
    );
    let p = first_element_child(&root);
    let run = p.children.iter().find(|c| matches!(c.kind, crate::box_tree::BoxKind::InlineRun { .. })).expect("InlineRun not found");
    assert!(
        (run.rect.height - 30.0).abs() < 0.5,
        "line-height:1.5 font-size:20px → height=30px, got {}",
        run.rect.height
    );
}

// ── Multi-column layout ──────────────────────────────────────────────────

#[test]
fn multicol_column_count_divides_width() {
    // column-count: 3 + column-gap: 10px → each column = (300 - 20) / 3 = 93.33px.
    // Three equal 30px boxes (total 90px) balance into 3 columns of 30px each,
    // so each box maps cleanly to one column fragment.
    let root = lay_measured(
        "<div id='c'><div></div><div></div><div></div></div>",
        "#c { width: 300px; column-count: 3; column-gap: 10px; } #c div { height: 30px; }",
        800.0,
    );
    let container = first_element_child(&root);
    assert_eq!(container.children.len(), 3);
    let col_w = container.children[0].rect.width;
    assert!((col_w - 93.33).abs() < 0.1, "col_w={col_w}");
    // All three children should be in different columns (x differs).
    let x0 = container.children[0].rect.x;
    let x1 = container.children[1].rect.x;
    let x2 = container.children[2].rect.x;
    assert!(x1 > x0, "child1.x={x1} should be right of child0.x={x0}");
    assert!(x2 > x1, "child2.x={x2} should be right of child1.x={x1}");
}

#[test]
fn multicol_no_repeat_width_when_no_column_props() {
    // Without column-count / column-width, block flow is unchanged.
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='b'></div></div>",
        "#c { width: 300px; } #a { height: 20px; } #b { height: 20px; }",
        800.0,
    );
    let container = first_element_child(&root);
    let ch0 = &container.children[0];
    let ch1 = &container.children[1];
    assert_eq!(ch0.rect.x, ch1.rect.x, "children should share same x in normal flow");
    assert!(ch1.rect.y > ch0.rect.y, "b should be below a");
}

#[test]
fn multicol_column_span_all_spans_full_width() {
    // A child with column-span:all should be laid out at the full container width,
    // not squeezed into a single column.
    // Layout: 2 column children → span-all → 2 more column children.
    let root = lay_measured(
        r#"<div id='c'>
          <div id='a'></div>
          <div id='s'></div>
          <div id='b'></div>
        </div>"#,
        r#"#c { width: 300px; column-count: 2; column-gap: 10px; }
           #a { height: 20px; }
           #b { height: 20px; }
           #s { column-span: all; height: 10px; }"#,
        800.0,
    );
    let container = first_element_child(&root);
    // Find the span-all child by its full container width (300px) — column
    // fragments of #a/#b are col_w wide, only the spanner spans the full width.
    let span_child = container.children.iter()
        .find(|c| (c.rect.width - 300.0).abs() < 1.0)
        .expect("span-all child not found");
    // Span-all element must cover the full container width (300px).
    assert!(
        (span_child.rect.width - 300.0).abs() < 1.0,
        "span-all child width={} should be 300px",
        span_child.rect.width
    );
    // Span-all element must start at container's content_x.
    assert!(
        span_child.rect.x < 10.0,
        "span-all child x={} should be near container left edge",
        span_child.rect.x
    );
}

#[test]
fn multicol_column_span_all_children_below_span() {
    // Children after a column-span:all element must be positioned below it.
    let root = lay_measured(
        r#"<div id='c'>
          <div id='s'></div>
          <div id='b'></div>
        </div>"#,
        r#"#c { width: 300px; column-count: 2; column-gap: 10px; }
           #s { column-span: all; height: 15px; }
           #b { height: 20px; }"#,
        800.0,
    );
    let container = first_element_child(&root);
    // Spanner is the only full-width (300px) child; #b becomes column fragments.
    let span_child = container.children.iter()
        .find(|c| (c.rect.width - 300.0).abs() < 1.0)
        .expect("span-all child not found");
    let span_bottom = span_child.rect.y + span_child.rect.height;
    // Every column fragment of #b (the non-span children) must be below the spanner.
    let after_children: Vec<&LayoutBox> = container.children.iter()
        .filter(|c| (c.rect.width - 300.0).abs() >= 1.0 && c.rect.height > 0.0)
        .collect();
    assert!(!after_children.is_empty(), "expected #b column fragments below span");
    for after_child in after_children {
        assert!(
            after_child.rect.y >= span_bottom,
            "after_child.y={} must be >= span bottom={}",
            after_child.rect.y,
            span_bottom
        );
    }
}

#[test]
fn multicol_column_fill_auto_sequential() {
    // column-fill: auto — each column is filled up to the container height before
    // spilling to the next column, rather than distributing content evenly.
    // 3 children of 15px each (total 45px) in a 40px-tall container: col0 fills to
    // 40px (the first two boxes + the top 10px of the third), and the third box's
    // remaining 5px spills into col1 (CSS Multicol §3.4 fragmentation).
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='b'></div><div id='d'></div></div>",
        "#c { width: 300px; column-count: 2; column-gap: 0px; height: 40px; column-fill: auto; } \
         #a { height: 15px; } #b { height: 15px; } #d { height: 15px; }",
        800.0,
    );
    let container = first_element_child(&root);
    let frags: Vec<&LayoutBox> = container.children.iter()
        .filter(|c| c.rect.height > 0.0)
        .collect();
    // col_w = 300 / 2 = 150. col0 at content_x, col1 at content_x + 150.
    let col0_x = frags.iter().map(|c| c.rect.x).fold(f32::INFINITY, f32::min);
    // col0 must be filled all the way to the container height before col1 is used.
    let col0_bottom = frags.iter()
        .filter(|c| (c.rect.x - col0_x).abs() < 1.0)
        .map(|c| c.rect.y + c.rect.height)
        .fold(0.0f32, f32::max);
    assert!(
        (col0_bottom - 40.0).abs() < 1.0,
        "col0 must fill to container height 40 before spilling (col0_bottom={col0_bottom})"
    );
    // The spillover fragment must exist in col1 (x = content_x + 150).
    assert!(
        frags.iter().any(|c| c.rect.x > col0_x + 100.0),
        "expected a spillover fragment in col1 (col0_x={col0_x})"
    );
}

#[test]
fn multicol_balance_fragments_boxes_across_columns() {
    // Regression (BUG-186, TEST-33 case 5): two 36px background boxes in a
    // 3-column balance container fragment into three 24px column slices
    // (total 72 / 3 = 24), matching Edge — not one atomic box per column with
    // an empty third column. The container height collapses to 24px.
    let root = lay_measured(
        "<div id='c'><div></div><div></div></div>",
        "#c { width: 660px; column-count: 3; column-gap: 12px; } #c div { height: 36px; }",
        800.0,
    );
    let container = first_element_child(&root);
    // col_w = (660 - 24) / 3 = 212.
    let frags: Vec<&LayoutBox> = container.children.iter()
        .filter(|c| c.rect.height > 0.0)
        .collect();
    // Every fragment is at most one column tall (24px), never a whole 36px box.
    for f in &frags {
        assert!(f.rect.height <= 24.0 + 0.5, "fragment too tall: {}", f.rect.height);
        assert!((f.rect.width - 212.0).abs() < 0.5, "fragment width={}", f.rect.width);
    }
    // All three columns receive content (distinct x positions).
    let mut xs: Vec<f32> = frags.iter().map(|f| f.rect.x).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    assert_eq!(xs.len(), 3, "all 3 columns should hold a fragment, got xs={xs:?}");
    // Container content height = balanced column height = 24px.
    assert!(
        (container.rect.height - 24.0).abs() < 1.0,
        "container height={} should be 24px",
        container.rect.height
    );
}

#[test]
fn multicol_column_fill_balance_vs_auto_target() {
    // Verify that column-fill:balance uses total/n_cols as target, not container height.
    // With height:20px and 2 children of 15px each and 2 columns:
    //   balance: target = ceil(30/2) = 15 → ch0 fills col0 (15px), ch1 overflows to col1
    //   auto:    target = 20 → ch0(15)+ch1(15)=30>20 with count_cap=1, so still col0+col1
    // Both end up with same layout here; test that column_fill_balance is parsed.
    let root = lay("<p>x</p>", "p { column-fill: balance; }");
    assert!(first_p_style(&root).column_fill_balance, "balance should set column_fill_balance=true");
    let root2 = lay("<p>x</p>", "p { column-fill: auto; }");
    assert!(!first_p_style(&root2).column_fill_balance, "auto should set column_fill_balance=false");
}

#[test]
fn multicol_balance_does_not_skip_first_column() {
    // Regression (BUG-117): with column-count:3 and items each taller than the
    // balanced target height, the greedy assigner advanced past the EMPTY first
    // column (height_overflow fires on column 0 because item height > target),
    // placing items in columns 1 and 2 and leaving column 0 blank. Items must
    // fill column 0 first (CSS Multicol §3.4 — columns filled in order).
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='b'></div></div>",
        "#c { width: 300px; column-count: 3; column-gap: 0px; } \
         #a { height: 40px; } #b { height: 40px; }",
        800.0,
    );
    let container = first_element_child(&root);
    let a = &container.children[0];
    let b = &container.children[1];
    // col_w = 300/3 = 100. col0 at content_x, col1 at content_x + 100.
    assert!(
        (a.rect.x - container.rect.x).abs() < 1.0,
        "first item must be in column 0 (a.x={}, container.x={})",
        a.rect.x, container.rect.x
    );
    assert!(
        (b.rect.x - a.rect.x - 100.0).abs() < 1.0,
        "second item must be in column 1, not column 2 (b.x={}, a.x={})",
        b.rect.x, a.rect.x
    );
}

#[test]
fn multicol_fill_auto_ignores_count_cap() {
    // Regression (BUG-117): column-fill:auto must fill a column purely by height.
    // The per-column count cap (a balance-mode anti-starvation guard) wrongly forced
    // one item per column even in auto mode. With 3 short items and a tall container,
    // all three must stack in column 0.
    let root = lay_measured(
        "<div id='c'><div id='a'></div><div id='b'></div><div id='d'></div></div>",
        "#c { width: 300px; column-count: 3; column-gap: 0px; height: 100px; column-fill: auto; } \
         #a { height: 10px; } #b { height: 10px; } #d { height: 10px; }",
        800.0,
    );
    let container = first_element_child(&root);
    let a = &container.children[0];
    let b = &container.children[1];
    let d = &container.children[2];
    // All three fit in column 0 (30px < 100px) → identical x.
    assert!(
        (a.rect.x - b.rect.x).abs() < 1.0 && (a.rect.x - d.rect.x).abs() < 1.0,
        "auto must stack all items in col0 (xs: {} {} {})",
        a.rect.x, b.rect.x, d.rect.x
    );
    // And they stack vertically within the column.
    assert!(
        b.rect.y > a.rect.y && d.rect.y > b.rect.y,
        "items must stack vertically in col0 (ys: {} {} {})",
        a.rect.y, b.rect.y, d.rect.y
    );
}

// ── ::marker box (BUG-011) ───────────────────────────────────────────

#[test]
fn list_item_generates_marker_box() {
    let root = lay("<ul><li>item</li></ul>", "");
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
    assert!(marker.is_some(), "list-item must have a ::marker child");
    if let BoxKind::Marker { text, position, list_style_type, .. } = &marker.unwrap().kind {
        // Disc renders geometrically — marker_text returns "" for bullet types.
        assert!(text.is_empty(), "disc marker text must be empty (geometric rendering)");
        assert_eq!(*list_style_type, ListStyleType::Disc, "default list-style-type is disc");
        assert_eq!(*position, ListStylePosition::Outside);
    }
}

#[test]
fn list_style_image_marker_carries_url() {
    // CSS Lists L3 §2.3 — `list-style-image` populates the Marker box's
    // `image` field and the URL is collected for fetching.
    let root = lay(
        "<ul><li>item</li></ul>",
        "li { list-style-image: url(\"bullet.png\"); }",
    );
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    if let BoxKind::Marker { image, .. } = &marker.kind {
        assert_eq!(image.as_deref(), Some("bullet.png"));
    } else {
        panic!("expected Marker box");
    }
    let urls = collect_background_image_requests(&root, 1.0);
    assert!(urls.iter().any(|u| u == "bullet.png"), "marker image must be fetched");
}

#[test]
fn list_style_image_marker_shown_with_type_none() {
    // CSS Lists L3 §2.3 — an explicit image still produces a marker even when
    // `list-style-type: none`.
    let root = lay(
        "<ul><li>item</li></ul>",
        "li { list-style-type: none; list-style-image: url(\"b.png\"); }",
    );
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
    assert!(marker.is_some(), "list-style-image must generate a marker despite type:none");
}

#[test]
fn list_item_none_no_marker() {
    let root = lay("<ul><li>item</li></ul>", "li { list-style-type: none; }");
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. }));
    assert!(marker.is_none(), "list-style-type:none must not generate marker");
}

#[test]
fn ordered_list_decimal_marker() {
    let root = lay(
        "<ol><li>a</li><li>b</li></ol>",
        "ol { list-style-type: decimal; }",
    );
    let ol = first_element_child(&root);
    let lis: Vec<_> = ol.children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
    assert_eq!(lis.len(), 2);
    let m0 = lis[0].children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let m1 = lis[1].children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    if let (BoxKind::Marker { text: t0, .. }, BoxKind::Marker { text: t1, .. }) = (&m0.kind, &m1.kind) {
        assert_eq!(t0, "1. ", "first item");
        assert_eq!(t1, "2. ", "second item");
    }
}

#[test]
fn marker_outside_not_in_flow() {
    // For outside markers: child_y must not advance past the marker.
    let root = lay(
        "<ul><li>item</li></ul>",
        "ul { margin: 0; padding: 0; } li { font-size: 16px; line-height: 1; }",
    );
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let content = li.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineRun { .. })).unwrap();
    // Marker y should equal content y (both at top of list item).
    assert_eq!(marker.rect.y, content.rect.y, "marker and content must share the same top");
    // Marker x must be to the left of content x.
    assert!(marker.rect.x < content.rect.x, "marker must be left of content");
}

/// BUG-038: list-style-position: inside — marker must share the first line with content,
/// not occupy a separate block line. li height must equal one line-height.
#[test]
fn marker_inside_shares_line_with_content() {
    let root = lay(
        "<ul><li>item</li></ul>",
        "ul { padding-left: 0; } \
         li { list-style-position: inside; font-size: 16px; line-height: 1; }",
    );
    let ul = first_element_child(&root);
    let li = ul.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let marker = li.children.iter().find(|c| matches!(&c.kind, BoxKind::Marker { .. })).unwrap();
    let content = li.children.iter().find(|c| matches!(&c.kind, BoxKind::InlineRun { .. })).unwrap();
    // Marker and content must be on the same line.
    assert_eq!(marker.rect.y, content.rect.y, "inside marker and content must share the same y");
    // Content must start to the right of the marker.
    assert!(content.rect.x > marker.rect.x, "inside marker must be left of content");
    // li height must be one line-height (16 * 1.0 = 16px), not two.
    assert!((li.rect.height - 16.0).abs() < 1.0,
        "li height should be one line (16px), got {}", li.rect.height);
}

// ─── CSS 2.1 §9.5 — float + clear ────────────────────────────────────────

/// `float: left` с явной шириной — элемент помещается у левого края контейнера.
#[test]
fn float_left_positioned_at_left_edge() {
    let root = lay(
        "<div class='c'><div class='f'>x</div></div>",
        ".c { width: 400px; }
         .f { float: left; width: 100px; height: 50px; }",
    );
    let c = first_element_child(&root);
    let f = first_element_child(c);
    assert_eq!(f.rect.x, 0.0, "float left: x at container left");
    assert_eq!(f.rect.y, 0.0, "float left: y at top");
    assert_eq!(f.rect.width,  100.0, "float left: explicit width");
    assert_eq!(f.rect.height,  50.0, "float left: explicit height");
}

/// `float: right` с явной шириной — элемент у правого края контейнера.
#[test]
fn float_right_positioned_at_right_edge() {
    let root = lay(
        "<div class='c'><div class='f'>x</div></div>",
        ".c { width: 400px; }
         .f { float: right; width: 100px; height: 50px; }",
    );
    let c = first_element_child(&root);
    let f = first_element_child(c);
    // right edge of container = 400px; float width = 100px → x = 300
    assert_eq!(f.rect.x, 300.0, "float right: x at container_right - width");
    assert_eq!(f.rect.y,   0.0, "float right: y at top");
}

/// Float left: последующий in-flow block-брат сохраняет полную ширину
/// containing-block, но его line-box сдвигается за float (CSS 2.1 §9.5).
/// RP-4: раньше клипали сам бокс (x=100, width=300) — это была аппроксимация.
#[test]
fn float_left_narrows_sibling_width() {
    let root = lay(
        "<div class='c'><div class='f'>x</div><div class='s'>y</div></div>",
        ".c { width: 400px; }
         .f { float: left; width: 100px; height: 50px; }
         .s { height: 30px; }",
    );
    let c = first_element_child(&root);
    let sibling = c.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.float_side == FloatSide::None)
        .expect("sibling block");
    // CSS 2.1 §9.5: the block keeps the full containing-block width, not narrowed.
    assert_eq!(sibling.rect.x,     0.0,   "sibling keeps full-width origin");
    assert_eq!(sibling.rect.width, 400.0, "sibling keeps full containing-block width");
    // Only its line box recedes past the float (the inner inline run starts at 100).
    let run = sibling.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::InlineRun { .. }))
        .expect("inline run");
    assert_eq!(run.rect.x, 100.0, "line box starts after the left float");
}

/// Float right: последующий in-flow block-брат сохраняет полную ширину;
/// его line-box укорачивается справа на ширину float (CSS 2.1 §9.5).
#[test]
fn float_right_narrows_sibling_width() {
    let root = lay(
        "<div class='c'><div class='f'>x</div><div class='s'>y</div></div>",
        ".c { width: 400px; }
         .f { float: right; width: 100px; height: 50px; }
         .s { height: 30px; }",
    );
    let c = first_element_child(&root);
    let sibling = c.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.float_side == FloatSide::None)
        .expect("sibling block");
    // CSS 2.1 §9.5: the block keeps the full containing-block width.
    assert_eq!(sibling.rect.x,     0.0,   "sibling starts at left edge");
    assert_eq!(sibling.rect.width, 400.0, "sibling keeps full containing-block width");
    // Its line box starts at the left edge but is shortened by the right float.
    let run = sibling.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::InlineRun { .. }))
        .expect("inline run");
    assert_eq!(run.rect.x, 0.0, "line box starts at left edge");
    assert!(run.rect.width <= 300.0 + 0.01, "line box shortened by right float, got {}", run.rect.width);
}

/// Два `float: left` выстраиваются горизонтально.
#[test]
fn two_left_floats_stack_horizontally() {
    let root = lay(
        "<div class='c'><div class='f1'>a</div><div class='f2'>b</div></div>",
        ".c  { width: 400px; }
         .f1 { float: left; width: 100px; height: 50px; }
         .f2 { float: left; width: 80px;  height: 40px; }",
    );
    let c = first_element_child(&root);
    let floats: Vec<_> = c.children.iter()
        .filter(|ch| ch.style.float_side == FloatSide::Left)
        .collect();
    assert_eq!(floats.len(), 2, "expected two left floats");
    assert_eq!(floats[0].rect.x, 0.0,   "first float at left edge");
    assert_eq!(floats[1].rect.x, 100.0, "second float after first");
}

/// `clear: both` сдвигает элемент ниже обоих float-ов.
#[test]
fn clear_both_advances_past_floats() {
    let root = lay(
        "<div class='c'><div class='fl'>a</div><div class='fr'>b</div><div class='clr'>c</div></div>",
        ".c   { width: 400px; }
         .fl  { float: left;  width: 80px; height: 60px; }
         .fr  { float: right; width: 80px; height: 40px; }
         .clr { clear: both; height: 20px; }",
    );
    let c = first_element_child(&root);
    let clr = c.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.clear == ClearSide::Both)
        .expect("clear:both block");
    // clear:both → must start at y >= max(60, 40) = 60
    assert!(clr.rect.y >= 60.0 - 0.01,
        "clear:both block must start below tallest float (got {})", clr.rect.y);
}

/// Контейнер height охватывает float (float clearing родителя).
/// CSS 2.1 §9.5: контейнер должен расти, чтобы содержать свои float-ы.
#[test]
fn container_height_encloses_float() {
    let root = lay(
        "<div class='c'><div class='f'>x</div></div>",
        ".c { width: 400px; }
         .f { float: left; width: 100px; height: 80px; }",
    );
    let c = first_element_child(&root);
    // Container has no non-float children, so height = float height = 80.
    assert!(c.rect.height >= 80.0 - 0.01,
        "container must enclose float (height={}, expected >=80)", c.rect.height);
}

/// `clear: left` сдвигает элемент мимо левого float.
#[test]
fn clear_left_only_clears_left_floats() {
    let root = lay(
        "<div class='c'><div class='fl'>a</div><div class='clr'>c</div></div>",
        ".c   { width: 400px; }
         .fl  { float: left; width: 80px; height: 50px; }
         .clr { clear: left; height: 20px; }",
    );
    let c = first_element_child(&root);
    let clr = c.children.iter()
        .find(|ch| matches!(ch.kind, BoxKind::Block) && ch.style.clear == ClearSide::Left)
        .expect("clear:left block");
    assert!(clr.rect.y >= 50.0 - 0.01,
        "clear:left must start below left float (got {})", clr.rect.y);
}

/// CSS `float` парсится в FloatSide.
#[test]
fn float_side_parsed_correctly() {
    let root = lay("<div class='l'>x</div><div class='r'>x</div><div class='n'>x</div>",
        ".l { float: left } .r { float: right } .n { float: none }");
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let l = iter.next().unwrap();
    let r = iter.next().unwrap();
    let n = iter.next().unwrap();
    assert_eq!(l.style.float_side, FloatSide::Left,  "float: left");
    assert_eq!(r.style.float_side, FloatSide::Right, "float: right");
    assert_eq!(n.style.float_side, FloatSide::None,  "float: none");
}

/// CSS `clear` парсится в ClearSide.
#[test]
fn clear_parsed_correctly() {
    let root = lay("<div class='b'>x</div><div class='l'>x</div><div class='r'>x</div>",
        ".b { clear: both } .l { clear: left } .r { clear: right }");
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let b = iter.next().unwrap();
    let l = iter.next().unwrap();
    let r = iter.next().unwrap();
    assert_eq!(b.style.clear, ClearSide::Both,  "clear: both");
    assert_eq!(l.style.clear, ClearSide::Left,  "clear: left");
    assert_eq!(r.style.clear, ClearSide::Right, "clear: right");
}

// ── Margin collapsing CSS 2.1 §8.3.1 ─────────────────────────────────────

/// Соседние блоки: побеждает бо́льший margin-top (top wins).
#[test]
fn sibling_blocks_margin_collapse_top_wins() {
    // mb=10, mt=30 → gap = max(10,30) = 30, а не 40
    let root = lay(
        "<div class='a'>x</div><div class='b'>y</div>",
        ".a { height: 10px; margin-bottom: 10px; } .b { height: 10px; margin-top: 30px; }",
    );
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    assert!((a.rect.y - 0.0).abs() < 0.1, "a.y={}", a.rect.y);
    // bottom of .a = 10. gap = max(10,30)=30. .b top = 40.
    assert!((b.rect.y - 40.0).abs() < 0.1, "b.y={}", b.rect.y);
}

/// Соседние блоки: побеждает бо́льший margin-bottom (bottom wins).
#[test]
fn sibling_blocks_margin_collapse_bottom_wins() {
    // mb=30, mt=10 → gap = max(30,10) = 30, а не 40
    let root = lay(
        "<div class='a'>x</div><div class='b'>y</div>",
        ".a { height: 10px; margin-bottom: 30px; } .b { height: 10px; margin-top: 10px; }",
    );
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    assert!((a.rect.y - 0.0).abs() < 0.1, "a.y={}", a.rect.y);
    // bottom of .a = 10. gap = max(30,10)=30. .b top = 40.
    assert!((b.rect.y - 40.0).abs() < 0.1, "b.y={}", b.rect.y);
}

/// Цепочка из трёх блоков: два соседних схлопывания независимы.
#[test]
fn three_sibling_blocks_margin_collapse_chain() {
    // .a mb=20, .b mt=15 mb=25, .c mt=10
    // gap(a–b) = max(20,15)=20,  gap(b–c) = max(25,10)=25
    let root = lay(
        "<div class='a'>x</div><div class='b'>y</div><div class='c'>z</div>",
        ".a { height: 5px; margin-bottom: 20px; }
         .b { height: 5px; margin-top: 15px; margin-bottom: 25px; }
         .c { height: 5px; margin-top: 10px; }",
    );
    let mut iter = root.children.iter().filter(|c| matches!(c.kind, BoxKind::Block));
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    let c = iter.next().unwrap();
    assert!((a.rect.y -  0.0).abs() < 0.1, "a.y={}", a.rect.y);
    assert!((b.rect.y - 25.0).abs() < 0.1, "b.y={}", b.rect.y);
    assert!((c.rect.y - 55.0).abs() < 0.1, "c.y={}", c.rect.y);
}

/// BUG-193: a `display: table` wrapper box is block-level, so its margins
/// collapse with adjacent sibling margins (CSS 2.1 §8.3.1) — even though the
/// table establishes a BFC for its own rows/cells. The gap between the table
/// and the following block must be `max(30, 10) = 30`, not the summed `40`.
#[test]
fn table_bottom_margin_collapses_with_next_sibling() {
    let root = lay(
        "<table class='t'><tr><td>x</td></tr></table><div class='b'>y</div>",
        ".t { margin-bottom: 30px; } .b { height: 10px; margin-top: 10px; }",
    );
    let table = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Table))
        .expect("table box");
    let b = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Block))
        .expect("following block");
    let gap = b.rect.y - (table.rect.y + table.rect.height);
    assert!(
        (gap - 30.0).abs() < 0.1,
        "table↔block gap={gap} (expected collapsed 30, not summed 40)",
    );
}

// ── CSS Intrinsic Sizing L3 — min-content / max-content / fit-content ────

/// `width: fit-content` на block-элементе с явной шириной потомка: бокс
/// сжимается до ширины потомка, не растягиваясь на весь контейнер.
#[test]
fn fit_content_shrinks_to_child_explicit_width() {
    let root = lay(
        "<div class='outer'><div class='inner'>x</div></div>",
        ".outer { width: fit-content; }
         .inner { width: 120px; height: 10px; }",
    );
    let outer = first_element_child(&root);
    // outer's border-box should equal inner's 120px (no padding/border on outer).
    assert!(
        (outer.rect.width - 120.0).abs() < 1.0,
        "outer.width={} expected≈120",
        outer.rect.width
    );
}

/// `width: fit-content` не выходит за пределы доступного пространства.
#[test]
fn fit_content_capped_at_available_width() {
    // Container 200px wide; inner has explicit width 300px (wider than container).
    let root = lay_viewport(
        "<div class='outer'><div class='inner'>x</div></div>",
        ".outer { width: fit-content; }
         .inner { width: 300px; height: 10px; }",
        Size { width: 200.0, height: 600.0 },
    );
    let outer = first_element_child(&root);
    // fit-content = min(available=200, max-content=300) → 200.
    assert!(
        outer.rect.width <= 200.0 + 0.5,
        "outer.width={} should be ≤ 200",
        outer.rect.width
    );
}

/// `width: max-content` expands past the container to fit content.
#[test]
fn max_content_expands_to_child_explicit_width() {
    let root = lay_viewport(
        "<div class='outer'><div class='inner'>x</div></div>",
        ".outer { width: max-content; }
         .inner { width: 500px; height: 10px; }",
        Size { width: 200.0, height: 600.0 },
    );
    let outer = first_element_child(&root);
    // max-content ignores available width — should be 500px.
    assert!(
        (outer.rect.width - 500.0).abs() < 1.0,
        "outer.width={} expected≈500",
        outer.rect.width
    );
}

/// `width: min-content` with single-word text: box shrinks to word width.
#[test]
fn min_content_shrinks_to_word_width() {
    // Fixed8 measurer: each char = 8px. "Hello" = 5 chars = 40px.
    // Container is 800px wide. min-content should give 40px.
    let root = lay_measured(
        "<p class='p'>Hello</p>",
        ".p { width: min-content; }",
        800.0,
    );
    let p = first_element_child(&root);
    // With Fixed8 measurer: "Hello" = 5 × 8 = 40px.
    assert!(
        (p.rect.width - 40.0).abs() < 1.0,
        "p.width={} expected≈40 (5 chars × 8px)",
        p.rect.width
    );
}

/// `width: fit-content` on block with text: shrinks to text width.
#[test]
fn fit_content_text_shrinks_within_container() {
    // "Hi" = 2 chars × 8px = 16px; container = 800px.
    let root = lay(
        "<p class='p'>Hi</p>",
        ".p { width: fit-content; }",
    );
    let p = first_element_child(&root);
    assert!(
        p.rect.width <= 800.0,
        "p.width={} should be ≤ container",
        p.rect.width
    );
    // Text content width = 16px. Box should shrink to ~16px (+ any padding).
    assert!(
        p.rect.width < 100.0,
        "p.width={} should be much less than 800px (container)",
        p.rect.width
    );
}

/// `width: fit-content` with text: element shrinks to text content width.
#[test]
fn fit_content_text_node_shrinks_to_content() {
    // "Hi" = 2 chars × 8px = 16px with Fixed8 measurer.
    let root = lay_measured(
        "<div class='d'>Hi</div>",
        ".d { width: fit-content; }",
        800.0,
    );
    let div = first_element_child(&root);
    // Should shrink to text content width ≈ 16px, not fill the 800px container.
    assert!(
        div.rect.width < 100.0,
        "div.width={} should shrink to ~16px",
        div.rect.width
    );
    assert!(
        div.rect.width >= 16.0,
        "div.width={} should be at least text width 16px",
        div.rect.width
    );
}

/// `width: max-content` parsing: keyword stored correctly.
#[test]
fn max_content_keyword_parsed() {
    let sheet = lumen_css_parser::parse(".x { width: max-content; }");
    let doc = lumen_html_parser::parse("<div class='x'>a</div>");
    let vp = Size { width: 800.0, height: 600.0 };
    use crate::style::Length;
    let children = doc.get(doc.body().unwrap()).children.clone();
    let div_id = children.into_iter().find(|&id| {
        matches!(&doc.get(id).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
    }).unwrap();
    let div_style = compute_style(&doc, div_id, &sheet, &ComputedStyle::root(), vp, false);
    assert!(
        matches!(div_style.width, Some(Length::MaxContent)),
        "expected MaxContent, got {:?}", div_style.width
    );
}

/// `width: min-content` and `width: fit-content` parsing round-trip.
#[test]
fn min_fit_content_keywords_parsed() {
    let sheet = lumen_css_parser::parse(".a { width: min-content; } .b { width: fit-content; }");
    let doc = lumen_html_parser::parse("<div class='a'></div><div class='b'></div>");
    let root_style = ComputedStyle::root();
    let vp = Size { width: 800.0, height: 600.0 };
    use crate::style::Length;
    let children = doc.get(doc.body().unwrap()).children.clone();
    let mut it = children.into_iter().filter(|&id| matches!(&doc.get(id).data, lumen_dom::NodeData::Element { .. }));
    let a_id = it.next().unwrap();
    let b_id = it.next().unwrap();
    let a_style = compute_style(&doc, a_id, &sheet, &root_style, vp, false);
    let b_style = compute_style(&doc, b_id, &sheet, &root_style, vp, false);
    assert!(matches!(a_style.width, Some(Length::MinContent)), "got {:?}", a_style.width);
    assert!(matches!(b_style.width, Some(Length::FitContent(None))), "got {:?}", b_style.width);
}

/// `fit-content(<length>)` functional form: parsed with inner length.
#[test]
fn fit_content_functional_form_parsed() {
    let sheet = lumen_css_parser::parse(".x { width: fit-content(200px); }");
    let doc = lumen_html_parser::parse("<div class='x'>a</div>");
    let vp = Size { width: 800.0, height: 600.0 };
    use crate::style::Length;
    let children = doc.get(doc.body().unwrap()).children.clone();
    let div_id = children.into_iter().find(|&id| {
        matches!(&doc.get(id).data, lumen_dom::NodeData::Element { name, .. } if name.local == "div")
    }).unwrap();
    let style = compute_style(&doc, div_id, &sheet, &ComputedStyle::root(), vp, false);
    assert!(
        matches!(style.width, Some(Length::FitContent(Some(_)))),
        "expected FitContent(Some(200px)), got {:?}", style.width
    );
}
