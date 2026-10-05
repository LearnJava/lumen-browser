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

/// Пустой subgrid всё равно несёт дорожки родителя (`LayoutBox::subgrid_tracks`): по ним paint
/// находит щели `column-rule`/`row-rule`, когда рёбер элементов нет.
#[test]
fn subgrid_box_keeps_inherited_tracks() {
    let root = lay(
        "<body><div id='g'><div id='sg'></div></div></body>",
        r#"
        body { margin: 0; }
        #g { display: grid; grid-template-columns: 20px 30px 40px 50px; grid-template-rows: 10px 20px 30px;
             column-gap: 5px; row-gap: 7px; width: 200px; }
        #sg { display: grid; grid-template-columns: subgrid; grid-template-rows: subgrid;
              grid-column: 2 / 4; grid-row: 2 / 4; }
        "#,
    );
    let sg = first_element_child(first_element_child(&root));
    let t = sg.subgrid_tracks.as_deref().expect("subgrid keeps the inherited tracks");
    assert_eq!(t.cols.as_deref(), Some(&[(0.0, 30.0), (35.0, 75.0)][..]));
    assert_eq!(t.rows.as_deref(), Some(&[(0.0, 20.0), (27.0, 57.0)][..]));
    // Обычный grid с шаблоном хранит собственные дорожки (срез 59), а не унаследованные: у него
    // нет `subgrid`-оси, но они те же `(start, end)` от начала content box.
    let g = first_element_child(&root).subgrid_tracks.as_deref().expect("a templated grid keeps its own tracks");
    assert_eq!(g.cols.as_deref().map(<[_]>::len), Some(4));
}

/// Grid L2 §9: an explicit `column-gap`/`row-gap` of a subgrid replaces the parent's gutter between
/// its tracks (each inner edge moves by half the difference); `normal` keeps the parent's. The
/// tracks the painter reads and the items' rects agree (`subgrid-gap-decorations-014`).
#[test]
fn subgrid_own_gap_replaces_the_parent_gutter() {
    let page = |sub_gaps: &str| {
        lay(
            "<body><div id='g'><div id='sg'><i></i><i></i><i></i></div></div></body>",
            &format!(
                "body {{ margin: 0; }}
                 #g {{ display: grid; grid-template-columns: repeat(3, 20px); gap: 0; width: 60px; }}
                 #sg {{ display: grid; grid-template-columns: subgrid; grid-column: 1 / 4; {sub_gaps} }}
                 i {{ display: block; }}"
            ),
        )
    };
    let sg_of = |root| first_element_child(first_element_child(root)).clone();
    let root = page("column-gap: 10px;");
    let sg = sg_of(&root);
    let t = sg.subgrid_tracks.as_deref().expect("tracks");
    // 60px over three tracks, two 10px gutters: 5 + 15 + 5 px of track around them.
    assert_eq!(t.cols.as_deref(), Some(&[(0.0, 15.0), (25.0, 35.0), (45.0, 60.0)][..]));
    let xs: Vec<(f32, f32)> = sg
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .map(|c| (c.rect.x - sg.rect.x, c.rect.x - sg.rect.x + c.rect.width))
        .collect();
    assert_eq!(xs.len(), 3);
    for (got, want) in xs.iter().zip([(0.0, 15.0), (25.0, 35.0), (45.0, 60.0)]) {
        assert!((got.0 - want.0).abs() < 0.01 && (got.1 - want.1).abs() < 0.01, "{xs:?}");
    }
    // `normal` keeps the parent's (here zero) gutter.
    let root = page("");
    let t = sg_of(&root).subgrid_tracks.clone().expect("tracks");
    assert_eq!(t.cols.as_deref(), Some(&[(0.0, 20.0), (20.0, 40.0), (40.0, 60.0)][..]));
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

/// GAP-REFERRER срез 7: `referrerpolicy` на `<img>` собирается наряду с
/// `crossorigin`; отсутствие атрибута — `None`, не пустая строка.
#[test]
fn collect_img_reads_referrerpolicy_attribute() {
    let doc = lumen_html_parser::parse(
        r#"<body><img src="a.png" referrerpolicy="no-referrer"><img src="b.png"></body>"#,
    );
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].referrer_policy_attr.as_deref(), Some("no-referrer"));
    assert_eq!(reqs[1].referrer_policy_attr, None, "absent attribute must be None");
}

/// `<video poster>` не несёт `referrerpolicy` (HTML LS §6.6 не связывает
/// атрибут с `<video>`) — BUG-848-путь всегда даёт `None`, даже если сам
/// автор его написал по ошибке.
#[test]
fn collect_video_poster_ignores_referrerpolicy_attribute() {
    let doc = lumen_html_parser::parse(
        r#"<body><video poster="p.jpg" referrerpolicy="no-referrer"></video></body>"#,
    );
    let reqs = collect_image_requests(&doc, vp());
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].referrer_policy_attr, None);
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

// ── BUG-1117: collect_cascade_background_image_requests ──────────────────

fn cascade_bg_urls(html: &str, css: &str, dpr: f32) -> Vec<String> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    collect_cascade_background_image_requests(&doc, &sheet, vp(), false, dpr)
}

/// Ранний (по каскаду) и авторитетный (по дереву боксов) сборщики обязаны
/// давать одни и те же URL-ы: всё, что ранний пропустит, пойдёт в сеть
/// последней волной, а лишнее — лишний запрос.
#[test]
fn cascade_bg_urls_match_layout_collector() {
    let cases: &[(&str, &str, f32)] = &[
        ("<body><div></div></body>", "div { width: 50px; height: 50px; background-image: url(bg.png); }", 1.0),
        (
            r#"<body><div class="a"></div><div class="b"></div><div class="a"></div></body>"#,
            ".a { background-image: url(a.png); } .b { background: url(b.png) no-repeat; }",
            1.0,
        ),
        ("<body><div></div></body>", "div { background-image: image-set(url(one.png) 1x, url(two.png) 2x); }", 2.0),
        ("<body><div></div></body>", "div { background-image: -webkit-cross-fade(url(f.png), url(t.png), 50%); }", 1.0),
        ("<body><div></div></body>", "div { background-image: url(x.png), linear-gradient(red, blue), url(y.png); }", 1.0),
        ("<body><ul><li>a</li></ul></body>", "li { list-style-image: url(\"bullet.png\"); }", 1.0),
        ("<body><p>x</p></body>", "p::before { content: url(icon.png); }", 1.0),
        ("<body><p>x</p></body>", "p::after { content: \"\"; display: block; width: 5px; height: 5px; background: url(after.png); }", 1.0),
        (r#"<body><p><img src="real.png"></p></body>"#, "", 1.0),
    ];
    for (html, css, dpr) in cases {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout(&doc, &sheet, vp());
        let from_layout = collect_background_image_requests(&root, *dpr);
        let from_cascade = collect_cascade_background_image_requests(&doc, &sheet, vp(), false, *dpr);
        assert_eq!(from_cascade, from_layout, "css: {css}");
    }
}

/// `display: none` у предка — бокса нет, фон не рисуется и не запрашивается
/// (как и в Chrome). `display: contents` — у самого элемента бокса нет, а у
/// потомков есть.
#[test]
fn cascade_bg_urls_skip_boxless_elements() {
    let urls = cascade_bg_urls(
        r#"<body><div class="h"><div class="in"></div></div><div class="c"><div class="kid"></div></div></body>"#,
        ".h { display: none; background: url(h.png); } .in { background: url(in.png); } \
         .c { display: contents; background: url(c.png); } .kid { background: url(kid.png); }",
        1.0,
    );
    assert_eq!(urls, vec!["kid.png".to_string()]);
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
        (*segments[0].style).clone()
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

pub(super) fn first_inline_run_frag(b: &LayoutBox) -> &InlineFrag {
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
