use lumen_core::geom::Size;

// ── BUG-734: intrinsic aspect ratio of a replaced element ─────────────────

/// Border box of the first `BoxKind::Image` produced by laying out
/// `html` + `css` in an 800×600 viewport.
///
/// The `<img width|height>` attributes stand in for the decoded intrinsic
/// size: layout has no decoder, and the shell delivers the real size the
/// same way (`apply_intrinsic_size` fills the empty attribute slots).
fn img_border_box(html: &str, css: &str) -> (f32, f32) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::Image { .. }) {
            return Some(b);
        }
        b.children.iter().find_map(find)
    }
    let img = find(&root).expect("Image box not found");
    (img.rect.width, img.rect.height)
}

/// `<img>` 852×725, CSS задаёт только ширину — высота обязана прийти из
/// intrinsic-соотношения, а не из сырого intrinsic-значения (CSS 2.1
/// §10.6.2). До BUG-734 давало 100×725.
#[test]
fn bug734_css_width_with_height_auto_uses_intrinsic_ratio() {
    let (w, h) = img_border_box(
        r#"<img width="852" height="725" src="x.png">"#,
        "img { width: 100px; height: auto; }",
    );
    assert_eq!(w, 100.0);
    assert!((h - 85.09).abs() < 0.1, "height={h}");
}

/// Самый частый в вебе responsive-идиом: `max-width: 100%` + `height:
/// auto`. Высота считается уже ПОСЛЕ клампа ширины, иначе картинка
/// растягивается по вертикали во столько же раз, во сколько сжата ширина.
#[test]
fn bug734_max_width_clamp_rescales_auto_height() {
    let (w, h) = img_border_box(
        r#"<div><img width="852" height="725" src="x.png"></div>"#,
        "div { width: 100px; } img { max-width: 100%; height: auto; }",
    );
    assert_eq!(w, 100.0);
    assert!((h - 85.09).abs() < 0.1, "height={h}");
}

/// Симметричный случай: задана высота, ширина `auto`. До BUG-734 ширина
/// оставалась сырым intrinsic-значением (852 вместо 117.5).
#[test]
fn bug734_css_height_with_width_auto_uses_intrinsic_ratio() {
    let (w, h) = img_border_box(
        r#"<img width="852" height="725" src="x.png">"#,
        "img { height: 100px; width: auto; }",
    );
    assert!((w - 117.52).abs() < 0.1, "width={w}");
    assert_eq!(h, 100.0);
}

/// Обе стороны `auto` — картинка рисуется натуральным размером
/// (поведение до BUG-734 сохранено).
#[test]
fn bug734_both_axes_auto_keep_natural_size() {
    let (w, h) = img_border_box(r#"<img width="852" height="725" src="x.png">"#, "");
    assert_eq!((w, h), (852.0, 725.0));
}

/// Обе стороны заданы автором — соотношение не вмешивается.
#[test]
fn bug734_both_axes_specified_ignore_ratio() {
    let (w, h) = img_border_box(
        r#"<img width="852" height="725" src="x.png">"#,
        "img { width: 300px; height: 40px; }",
    );
    assert_eq!((w, h), (300.0, 40.0));
}

/// Author-ский `aspect-ratio` перекрывает intrinsic-соотношение
/// (CSS Sizing L4 §4.1: intrinsic ratio подставляется только под
/// `aspect-ratio: auto`, то есть под initial-значение).
#[test]
fn bug734_author_aspect_ratio_beats_intrinsic() {
    let (w, h) = img_border_box(
        r#"<img width="852" height="725" src="x.png">"#,
        "img { width: 100px; height: auto; aspect-ratio: 1 / 1; }",
    );
    assert_eq!((w, h), (100.0, 100.0));
}

/// Картинка без известного intrinsic-размера (ни одного атрибута — shell
/// ещё не декодировал): соотношения нет, коробка нулевая, как и раньше.
#[test]
fn bug734_unknown_intrinsic_size_stays_collapsed() {
    let (w, h) = img_border_box(r#"<img src="x.png">"#, "img { height: auto; }");
    assert_eq!((w, h), (0.0, 0.0));
}

// ── BUG-737: intrinsic width of a row flex container ──────────────────────

/// Border-box widths of the direct children of the element with `id="outer"`.
///
/// Leaf sizes are declared in CSS on purpose: `super::super::layout` runs without a
/// `TextMeasurer`, so any text-derived width would measure as zero and the
/// assertions would pass for the wrong reason.
fn child_widths(html: &str, css: &str) -> Vec<f32> {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let outer = super::find_by_id_all(&root, &doc, "outer").expect("#outer box not found");
    outer
        .children
        .iter()
        .filter(|c| !matches!(c.kind, super::super::BoxKind::Skip))
        .map(|c| c.rect.width)
        .collect()
}

const FLEX_CSS: &str =
    "#outer { display: flex; width: 600px; } \
     .inner { display: flex; } \
     .leaf { display: block; width: 40px; height: 10px; } \
     .tail { width: 30px; height: 10px; }";
const FLEX_HTML: &str = r#"<div id="outer">
    <div class="inner"><div class="leaf"></div><div class="leaf"></div></div>
    <div class="tail"></div></div>"#;

/// Вложенный row-flex как flex-элемент: его max-content — СУММА элементов,
/// а не максимум (CSS Flexbox §9.9). До BUG-737 давало 40 вместо 80.
#[test]
fn bug737_nested_row_flex_max_content_is_sum() {
    assert_eq!(child_widths(FLEX_HTML, FLEX_CSS), vec![80.0, 30.0]);
}

/// `column-gap` — тоже часть intrinsic-ширины: два элемента по 40 с
/// зазором 10 дают 90.
#[test]
fn bug737_row_flex_max_content_includes_gap() {
    let css = FLEX_CSS.replace(".inner { display: flex; }", ".inner { display: flex; gap: 10px; }");
    assert_eq!(child_widths(FLEX_HTML, &css), vec![90.0, 30.0]);
}

/// `flex-wrap: wrap` не меняет max-content: перенос строк подавлен по
/// определению max-content, все элементы остаются на одной линии.
#[test]
fn bug737_wrapping_row_flex_max_content_is_still_sum() {
    let css = FLEX_CSS.replace(
        ".inner { display: flex; }",
        ".inner { display: flex; flex-wrap: wrap; }",
    );
    assert_eq!(child_widths(FLEX_HTML, &css), vec![80.0, 30.0]);
}

/// Колоночный контейнер элементы складывает вертикально — как блок,
/// поэтому правило «самый широкий ребёнок» для него верно и не тронуто.
#[test]
fn bug737_column_flex_max_content_stays_max() {
    let css = FLEX_CSS.replace(
        ".inner { display: flex; }",
        ".inner { display: flex; flex-direction: column; }",
    );
    assert_eq!(child_widths(FLEX_HTML, &css), vec![40.0, 30.0]);
}

/// Абсолютно позиционированный ребёнок flex-элементом не является
/// (§4.1) и в intrinsic-ширину контейнера не входит.
#[test]
fn bug737_absolutely_positioned_child_does_not_contribute() {
    let css = format!("{FLEX_CSS} .abs {{ position: absolute; width: 500px; height: 10px; }}");
    let html = r#"<div id="outer">
        <div class="inner"><div class="leaf"></div><div class="abs"></div></div>
        <div class="tail"></div></div>"#;
    assert_eq!(child_widths(html, &css), vec![40.0, 30.0]);
}

/// min-content row-flex-контейнера с `nowrap` — тоже сумма: элементы
/// нечем развести по строкам, поэтому контейнер не может стать уже 80 и
/// переполняет тесный родитель (сверено с Edge).
#[test]
fn bug737_nowrap_row_flex_min_content_is_sum() {
    let css = FLEX_CSS.replace("width: 600px;", "width: 60px;");
    assert_eq!(child_widths(FLEX_HTML, &css)[0], 80.0);
}

/// shrink-to-fit `inline-block`, внутри которого row-flex: обтягивает
/// сумму, а не самый широкий элемент.
#[test]
fn bug737_inline_block_wrapping_row_flex_shrinks_to_sum() {
    let css = format!("{FLEX_CSS} #ib {{ display: inline-block; }}");
    let html = r#"<div id="outer"><div id="ib">
        <div class="inner"><div class="leaf"></div><div class="leaf"></div></div>
        </div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let ib = super::find_by_id_all(&root, &doc, "ib").expect("#ib box not found");
    assert_eq!(ib.rect.width, 80.0);
}

// ── BUG-738: out-of-flow дети не участвуют в intrinsic-ширине ─────────────

const ABS_CSS: &str =
    "#outer { display: flex; width: 600px; } \
     .item { position: relative; } \
     .leaf { display: block; width: 40px; height: 10px; } \
     .drop { position: absolute; width: 300px; height: 10px; } \
     .tail { width: 30px; height: 10px; }";

/// Меню-выпадайка `position: absolute` шириной 300px внутри пункта
/// навигации не должна раздувать сам пункт до 300px (CSS 2.1 §10.3.7 —
/// out-of-flow бокс меряется от своего containing block, а не от родителя).
/// Ровно эта форма у верхней навигации `tbank.ru`.
#[test]
fn bug738_absolute_child_does_not_inflate_flex_item() {
    let html = r#"<div id="outer">
        <div class="item"><div class="leaf"></div><div class="drop"></div></div>
        <div class="tail"></div></div>"#;
    assert_eq!(child_widths(html, ABS_CSS), vec![40.0, 30.0]);
}

/// То же для shrink-to-fit флоата.
#[test]
fn bug738_absolute_child_does_not_inflate_float() {
    let css = format!("{ABS_CSS} #fl {{ float: left; position: relative; }}");
    let html = r#"<div id="outer"><div id="fl">
        <div class="leaf"></div><div class="drop"></div></div></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let fl = super::find_by_id_all(&root, &doc, "fl").expect("#fl box not found");
    assert_eq!(fl.rect.width, 40.0);
}

/// `display: none` бокса не существует вовсе — даже его padding не имеет
/// права попасть в intrinsic-ширину родителя.
#[test]
fn bug738_display_none_child_does_not_inflate() {
    let css = format!("{ABS_CSS} .gone {{ display: none; width: 300px; padding: 0 50px; }}");
    let html = r#"<div id="outer">
        <div class="item"><div class="leaf"></div><div class="gone"></div></div>
        <div class="tail"></div></div>"#;
    assert_eq!(child_widths(html, &css), vec![40.0, 30.0]);
}

/// `position: fixed` — тот же out-of-flow случай, что и `absolute`.
#[test]
fn bug738_fixed_child_does_not_inflate() {
    let css = ABS_CSS.replace("position: absolute;", "position: fixed;");
    let html = r#"<div id="outer">
        <div class="item"><div class="leaf"></div><div class="drop"></div></div>
        <div class="tail"></div></div>"#;
    assert_eq!(child_widths(html, &css), vec![40.0, 30.0]);
}

/// Обычный in-flow потомок по-прежнему задаёт ширину пункта.
#[test]
fn bug738_in_flow_child_still_contributes() {
    let css = ABS_CSS.replace(".drop { position: absolute;", ".drop { position: static;");
    let html = r#"<div id="outer">
        <div class="item"><div class="leaf"></div><div class="drop"></div></div>
        <div class="tail"></div></div>"#;
    assert_eq!(child_widths(html, &css), vec![300.0, 30.0]);
}

// ── BUG-739: inline-flex / inline-grid — atomic inline-level боксы ────────

/// `display: inline-flex`/`inline-grid` — atomic inline-level (CSS Display
/// L3 §2.1): снаружи inline, внутри собственный formatting context. До
/// BUG-739 они не создавали бокса вовсе — содержимое уплощалось в
/// `InlineRun` родителя, как при `display: inline`.
///
/// Размеры листьев задаются в CSS: `super::super::layout` работает без
/// `TextMeasurer`, поэтому любая ширина из текста измерилась бы нулём и
/// проверки прошли бы по неверной причине.
const IL_CSS: &str = "#outer { display: block; width: 600px; } \
     .leaf { display: block; width: 40px; height: 10px; }";
const IL_HTML: &str = r#"<div id="outer"><span id="t">
    <i class="leaf"></i><i class="leaf"></i></span></div>"#;

/// Бокс есть, лежит в `InlineBlockRow` (как `inline-block`), а его
/// auto-ширина — shrink-to-fit по row-flex сумме, а не 600 родителя.
#[test]
fn bug739_inline_flex_gets_its_own_shrink_to_fit_box() {
    let css = format!("{IL_CSS} #t {{ display: inline-flex; }}");
    let doc = lumen_html_parser::parse(IL_HTML);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let outer = super::find_by_id_all(&root, &doc, "outer").expect("#outer box not found");
    assert!(
        outer.children.iter().any(|c| matches!(c.kind, super::super::BoxKind::InlineBlockRow)),
        "atomic inline-level бокс должен собираться в InlineBlockRow"
    );
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t box not found");
    assert_eq!(t.style.display, super::super::Display::InlineFlex);
    assert_eq!(t.rect.width, 80.0);
}

/// Внутри действительно работает flex-алгоритм, а не блочный поток:
/// элементы стоят бок о бок, второй — за первым по X на одном Y.
#[test]
fn bug739_inline_flex_lays_items_side_by_side() {
    let css = format!("{IL_CSS} #t {{ display: inline-flex; }}");
    let doc = lumen_html_parser::parse(IL_HTML);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t box not found");
    let items: Vec<_> = t.children.iter().filter(|c| !matches!(c.kind, super::super::BoxKind::Skip)).collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].rect.x - t.rect.x, 0.0);
    assert_eq!(items[1].rect.x - t.rect.x, 40.0);
    assert_eq!(items[0].rect.y, items[1].rect.y);
}

/// `column-gap` внутри inline-flex доезжает и до раскладки, и до
/// shrink-to-fit ширины (после BUG-737 сумма учитывает зазор).
#[test]
fn bug739_inline_flex_honours_gap() {
    let css = format!("{IL_CSS} #t {{ display: inline-flex; gap: 10px; }}");
    let doc = lumen_html_parser::parse(IL_HTML);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t box not found");
    assert_eq!(t.rect.width, 90.0);
    let items: Vec<_> = t.children.iter().filter(|c| !matches!(c.kind, super::super::BoxKind::Skip)).collect();
    assert_eq!(items[1].rect.x - t.rect.x, 50.0);
}

/// Колоночный inline-flex складывает элементы вертикально; ширина —
/// самый широкий элемент, высота — сумма (сверено с headless Edge).
#[test]
fn bug739_inline_flex_column_stacks_items() {
    let css = format!("{IL_CSS} #t {{ display: inline-flex; flex-direction: column; }}");
    let doc = lumen_html_parser::parse(IL_HTML);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t box not found");
    assert_eq!(t.rect.width, 40.0);
    assert_eq!(t.rect.height, 20.0);
}

/// Явные `width` + padding + border дают border-box 214 — тот же расчёт,
/// что у `inline-block` (Edge: 214).
#[test]
fn bug739_inline_flex_explicit_width_adds_padding_and_border() {
    let css = format!(
        "{IL_CSS} #t {{ display: inline-flex; width: 200px; padding: 5px; \
         border: 2px solid #000; }}"
    );
    let doc = lumen_html_parser::parse(IL_HTML);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t box not found");
    assert_eq!(t.rect.width, 214.0);
}

/// `inline-grid` запускает grid-алгоритм: два трека 40px ставят второй
/// элемент на x = 40. (Shrink-to-fit ширина самого контейнера остаётся
/// блочной — это [BUG-740], отдельная заявка.)
#[test]
fn bug739_inline_grid_runs_track_sizing() {
    let css = format!(
        "{IL_CSS} #t {{ display: inline-grid; grid-template-columns: 40px 40px; }}"
    );
    let doc = lumen_html_parser::parse(IL_HTML);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t box not found");
    assert_eq!(t.style.display, super::super::Display::InlineGrid);
    let items: Vec<_> = t.children.iter().filter(|c| !matches!(c.kind, super::super::BoxKind::Skip)).collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].rect.x - t.rect.x, 0.0);
    assert_eq!(items[1].rect.x - t.rect.x, 40.0);
    assert_eq!(items[0].rect.y, items[1].rect.y);
}

/// Внутри inline-элемента бокс приходит через `InlineEscape` (BUG-728) и
/// наследует стиль inline-родителя, а не блока: `<a>` красит содержимое.
#[test]
fn bug739_inline_flex_inside_inline_element_gets_box() {
    let css = format!("{IL_CSS} #t {{ display: inline-flex; }} a {{ color: #008800; }}");
    let html = r##"<div id="outer"><a href="#"><span id="t">
        <i class="leaf"></i><i class="leaf"></i></span></a></div>"##;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t box not found");
    assert_eq!(t.rect.width, 80.0);
    assert_eq!(t.style.color, crate::Color { r: 0, g: 136, b: 0, a: 255 });
}

/// Два соседних inline-flex стоят на одной строке — это и значит
/// «снаружи ведёт себя как inline».
#[test]
fn bug739_two_inline_flex_siblings_share_a_line() {
    let css = format!("{IL_CSS} .t {{ display: inline-flex; }}");
    let html = r#"<div id="outer"><span class="t" id="a"><i class="leaf"></i></span><span
        class="t" id="b"><i class="leaf"></i></span></div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let a = super::find_by_id_all(&root, &doc, "a").expect("#a box not found");
    let b = super::find_by_id_all(&root, &doc, "b").expect("#b box not found");
    assert_eq!(a.rect.y, b.rect.y);
    assert!(b.rect.x >= a.rect.x + a.rect.width, "второй бокс должен стоять справа от первого");
}

// ── BUG-742: процентная width в intrinsic-контексте ведёт себя как auto ───

/// CSS Sizing L3 §5.2.1: в intrinsic-расчёте процент неразрешим, поэтому
/// `width: <%>` трактуется как `auto` и вклад берётся из содержимого. До
/// BUG-742 процент резолвился против нуля, и от поддерева оставались одни
/// padding + border самого бокса (кнопка CTA `tbank.ru` — 32 px вместо 154).
///
/// Листья опять размечены в CSS: `super::super::layout` идёт без `TextMeasurer`.
const PCT_CSS: &str = "#outer { display: block; width: 600px; } \
     .leaf { display: block; width: 120px; height: 10px; } \
     #t { display: inline-block; }";
const PCT_HTML: &str = r#"<div id="outer"><span id="t">
    <span id="p"><i class="leaf"></i></span></span></div>"#;

/// Ширины боксов `#t` (shrink-to-fit) и `#p` (процентный) по одному прогону.
fn pct_widths(css: &str) -> (f32, f32) {
    let doc = lumen_html_parser::parse(PCT_HTML);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let t = super::find_by_id_all(&root, &doc, "t").expect("#t box not found");
    let p = super::find_by_id_all(&root, &doc, "p").expect("#p box not found");
    (t.rect.width, p.rect.width)
}

/// `width: 100%` внутри shrink-to-fit: контейнер обтягивает лист (120), а не
/// схлопывается в 0.
#[test]
fn bug742_percent_width_does_not_erase_content_contribution() {
    let css = format!("{PCT_CSS} #p {{ display: block; width: 100%; }}");
    assert_eq!(pct_widths(&css), (120.0, 120.0));
}

/// Padding процентного бокса складывается с вкладом содержимого, а не
/// заменяет его: 120 + 16 + 16 (`box-sizing: border-box` — ровно форма
/// кнопки CTA `tbank.ru`).
#[test]
fn bug742_percent_width_box_keeps_its_padding_on_top_of_content() {
    let css = format!(
        "{PCT_CSS} #p {{ display: block; width: 100%; padding: 0 16px; \
         box-sizing: border-box; }}"
    );
    assert_eq!(pct_widths(&css), (152.0, 152.0));
}

/// Процент по-прежнему разрешается на самой раскладке — как auto он ведёт
/// себя только в intrinsic-расчёте: 50% от обтянутых 120 дают 60.
#[test]
fn bug742_percent_still_resolves_against_the_used_width() {
    let css = format!("{PCT_CSS} #p {{ display: block; width: 50%; }}");
    assert_eq!(pct_widths(&css), (120.0, 60.0));
}

/// `calc()` с процентом внутри неразрешим целиком — тоже auto.
#[test]
fn bug742_calc_with_percent_is_treated_as_auto_too() {
    let css = format!("{PCT_CSS} #p {{ display: block; width: calc(100% - 20px); }}");
    assert_eq!(pct_widths(&css), (120.0, 100.0));
}

/// Абсолютная `width` — по-прежнему явная: она задаёт и intrinsic-вклад,
/// и использованную ширину (страховка от «починили процент, сломали px»).
#[test]
fn bug742_absolute_width_still_wins_over_content() {
    let css = format!("{PCT_CSS} #p {{ display: block; width: 300px; }}");
    assert_eq!(pct_widths(&css), (300.0, 300.0));
}

/// Тесный родитель: shrink-to-fit ограничен доступной шириной, процентный
/// бокс тянется по ней, а не по своему min-content (Edge: 60 / 60).
#[test]
fn bug742_percent_width_follows_a_narrow_containing_block() {
    let css = format!("{PCT_CSS} #p {{ display: block; width: 100%; }}")
        .replace("#outer { display: block; width: 600px; }", "#outer { display: block; width: 60px; }");
    assert_eq!(pct_widths(&css), (60.0, 60.0));
}

// ── Hyphenation helpers ───────────────────────────────────────────────────

#[test]
fn strip_soft_hyphens_removes_shy_and_collects_positions() {
    let (disp, pos) = super::super::strip_soft_hyphens("hy\u{00AD}phen");
    assert_eq!(disp, "hyphen");
    assert_eq!(pos, vec![2]); // break point between 'y' and 'p'
}

#[test]
fn strip_soft_hyphens_multiple_breaks() {
    // "su\u{AD}per\u{AD}man"
    let (disp, pos) = super::super::strip_soft_hyphens("su\u{00AD}per\u{00AD}man");
    assert_eq!(disp, "superman");
    assert_eq!(pos, vec![2, 5]);
}

#[test]
fn strip_soft_hyphens_no_shy_returns_empty_positions() {
    let (disp, pos) = super::super::strip_soft_hyphens("hello");
    assert_eq!(disp, "hello");
    assert!(pos.is_empty());
}

#[test]
fn measure_text_w_empty_is_zero() {
    struct ZeroMeasurer;
    impl super::super::super::TextMeasurer for ZeroMeasurer {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    let m = ZeroMeasurer;
    assert_eq!(super::super::measure_text_w("", 16.0, 0.0, 0.0, &m), 0.0);
}

#[test]
fn measure_text_w_three_chars_no_spacing() {
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    // 3 chars × 8px − 0 letter-spacing = 24px
    let w = super::super::measure_text_w("abc", 16.0, 0.0, 0.0, &Fixed8);
    assert_eq!(w, 24.0);
}

#[test]
fn try_hyp_break_finds_rightmost_fitting_split() {
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    // "superman" → break positions [2, 5] (su|per|man)
    // Each char = 8px; hyphen = 8px.
    // If available_w = 32px: "su-" = 3×8 = 24 ≤ 32 ✓, "super-" = 6×8 = 48 > 32
    // So rightmost fitting = pos 2 ("su-" / "perman")
    let m = Fixed8;
    let result = super::super::try_hyp_break("superman", 32.0, 16.0, 0.0, &m, &[2, 5]);
    assert_eq!(result, Some(("su-".to_string(), "perman".to_string())));
}

#[test]
fn try_hyp_break_prefers_rightmost_break() {
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    // "superman" → break positions [2, 5]; available = 56px
    // "super-" = 6×8 = 48 ≤ 56 ✓ → prefer pos 5 over pos 2
    let m = Fixed8;
    let result = super::super::try_hyp_break("superman", 56.0, 16.0, 0.0, &m, &[2, 5]);
    assert_eq!(result, Some(("super-".to_string(), "man".to_string())));
}

#[test]
fn try_hyp_break_returns_none_when_nothing_fits() {
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    // Only 10px available; minimum "su-" = 24px
    let m = Fixed8;
    let result = super::super::try_hyp_break("superman", 10.0, 16.0, 0.0, &m, &[2, 5]);
    assert!(result.is_none());
}

#[test]
fn wrap_inline_run_soft_hyphen_breaks_word_on_manual() {
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let style = ComputedStyle::root();
    // Segment: "hi hy\u{AD}phen" — two words; 'hi' fills line, 'hy\u{AD}phen' needs break.
    // char=10, max_width=60:
    //   "hi"=20px fits; then gap(10)+60=90>60 → wrap attempted.
    //   avail = 60-20-10 = 30; "hy-"=30 ≤ 30 → break at pos 2.
    let seg = InlineSegment {
        text: "hi hy\u{00AD}phen".to_string(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };

    let m = Fixed10;
    let hp = NullHyphenationProvider;
    let lines = wrap_inline_run(&[seg], 60.0, 16.0, 0.0, Size::new(800.0, 600.0), &m, Hyphens::Manual, &hp, crate::style::WhiteSpace::Normal, crate::style::WordBreak::Normal, crate::style::OverflowWrap::Normal, crate::style::LineBreak::Auto);
    assert_eq!(lines.len(), 2, "expected 2 lines, got {}", lines.len());
    // Line 1 has both "hi" and "hy-" merged or as separate frags.
    let line1_text: String = lines[0].iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join(" ");
    assert!(line1_text.contains("hi"), "line1={line1_text}");
    assert!(line1_text.contains("hy-"), "line1={line1_text}");
    assert_eq!(lines[1].len(), 1);
    assert_eq!(lines[1][0].text, "phen");
}

#[test]
fn wrap_inline_run_hyphens_none_no_break_on_shy() {
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let style = ComputedStyle::root();
    // Same segment, Hyphens::None → soft hyphen ignored, full word wraps to new line unbroken.
    let seg = InlineSegment {
        text: "hi hy\u{00AD}phen".to_string(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };
    let m = Fixed10;
    let hp = NullHyphenationProvider;
    let lines = wrap_inline_run(&[seg], 60.0, 16.0, 0.0, Size::new(800.0, 600.0), &m, Hyphens::None, &hp, crate::style::WhiteSpace::Normal, crate::style::WordBreak::Normal, crate::style::OverflowWrap::Normal, crate::style::LineBreak::Auto);
    assert_eq!(lines.len(), 2, "expected 2 lines, got {}", lines.len());
    // Line 1 has only "hi"; line 2 has "hyphen" (whole, no hyphen char).
    assert_eq!(lines[0].len(), 1);
    assert_eq!(lines[0][0].text, "hi");
    let line2_text = &lines[1][0].text;
    assert_eq!(line2_text, "hyphen", "soft-hyphen should be stripped: {line2_text}");
}

// ── F-2: CSS hyphens — soft hyphen (U+00AD) rendering ───────────────────

#[test]
fn shy_invisible_when_word_fits_on_line() {
    // hyphens: manual, wide container — word with SHY fits; SHY must be stripped,
    // no visible hyphen in the rendered fragment (CSS Text L3 §6).
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let style = ComputedStyle::root();
    // "hy\u{AD}phen" → strip → "hyphen" = 6 chars × 10px = 60px; max_width=200 → fits.
    let seg = InlineSegment {
        text: "hy\u{00AD}phen".to_string(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };

    let m = Fixed10;
    let hp = NullHyphenationProvider;
    let lines = wrap_inline_run(
        &[seg], 200.0, 16.0, 0.0, Size::new(800.0, 600.0),
        &m, Hyphens::Manual, &hp,
        crate::style::WhiteSpace::Normal,
        crate::style::WordBreak::Normal,
        crate::style::OverflowWrap::Normal,
        crate::style::LineBreak::Auto,
    );
    assert_eq!(lines.len(), 1, "single word must stay on one line");
    let text = &lines[0][0].text;
    assert_eq!(text, "hyphen", "SHY must be stripped when word fits: got {text}");
    assert!(!text.contains('\u{00AD}'), "U+00AD must not appear in output");
    assert!(!text.contains('-'), "no hyphen added when no line break occurs: {text}");
}

#[test]
fn shy_rightmost_fitting_break_selected() {
    // hyphens: manual, word with two SHY positions — the rightmost that fits is used.
    // CSS Text L3 §6 requires the typographically preferred (rightmost) break.
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let style = ComputedStyle::root();
    // Segment "xx su\u{AD}per\u{AD}man", max_width=90:
    //   "xx"=20px occupies line;
    //   "superman"=80px needs wrap (20+10+80=110 > 90);
    //   avail = 90−20−10(gap) = 60;
    //   SHY positions in "superman": [2]="su", [5]="super";
    //   rightmost: "super"=50, 50+10(hyphen)=60 ≤ 60 → break → "super-" / "man".
    let seg = InlineSegment {
        text: "xx su\u{00AD}per\u{00AD}man".to_string(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };

    let m = Fixed10;
    let hp = NullHyphenationProvider;
    let lines = wrap_inline_run(
        &[seg], 90.0, 16.0, 0.0, Size::new(800.0, 600.0),
        &m, Hyphens::Manual, &hp,
        crate::style::WhiteSpace::Normal,
        crate::style::WordBreak::Normal,
        crate::style::OverflowWrap::Normal,
        crate::style::LineBreak::Auto,
    );
    assert_eq!(lines.len(), 2, "expected 2 lines, got {}", lines.len());
    let line1_text: String = lines[0].iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join(" ");
    assert!(line1_text.contains("super-"), "rightmost SHY break → 'super-', got: {line1_text}");
    assert!(!line1_text.contains("su-"), "must NOT use leftmost SHY: {line1_text}");
    assert_eq!(lines[1].len(), 1);
    assert_eq!(lines[1][0].text, "man");
}

#[test]
fn shy_auto_mode_respects_shy_positions() {
    // hyphens: auto with NullHyphenationProvider (no dict) falls back to SHY positions,
    // identical to manual mode behaviour for words with explicit U+00AD.
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let style = ComputedStyle::root();
    // Same geometry as shy_rightmost_fitting_break_selected but with Hyphens::Auto.
    let seg = InlineSegment {
        text: "xx su\u{00AD}per\u{00AD}man".to_string(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };

    let m = Fixed10;
    let hp = NullHyphenationProvider;
    let lines = wrap_inline_run(
        &[seg], 90.0, 16.0, 0.0, Size::new(800.0, 600.0),
        &m, Hyphens::Auto, &hp,
        crate::style::WhiteSpace::Normal,
        crate::style::WordBreak::Normal,
        crate::style::OverflowWrap::Normal,
        crate::style::LineBreak::Auto,
    );
    assert_eq!(lines.len(), 2, "auto mode: expected 2 lines, got {}", lines.len());
    let line1_text: String = lines[0].iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join(" ");
    assert!(
        line1_text.contains("super-"),
        "auto mode must honour SHY positions: {line1_text}",
    );
    assert_eq!(lines[1][0].text, "man");
}

#[test]
fn shy_manual_no_hyphen_when_no_shy_in_word() {
    // hyphens: manual without U+00AD — word must wrap to the next line as-is,
    // without any hyphen appended (no auto-hyphenation in manual mode).
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let style = ComputedStyle::root();
    // "aa longword": "aa"=20px, "longword"=80px; max_width=50;
    //   20+10+80=110 > 50 → needs_wrap; no SHY → try_hyp_break returns None
    //   → normal wrap: "longword" moves to next line intact, no hyphen.
    let seg = InlineSegment {
        text: "aa longword".to_string(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };

    let m = Fixed10;
    let hp = NullHyphenationProvider;
    let lines = wrap_inline_run(
        &[seg], 50.0, 16.0, 0.0, Size::new(800.0, 600.0),
        &m, Hyphens::Manual, &hp,
        crate::style::WhiteSpace::Normal,
        crate::style::WordBreak::Normal,
        crate::style::OverflowWrap::Normal,
        crate::style::LineBreak::Auto,
    );
    assert_eq!(lines.len(), 2, "expected 2 lines");
    assert_eq!(lines[0].len(), 1);
    assert_eq!(lines[0][0].text, "aa");
    assert_eq!(lines[1].len(), 1);
    let word = &lines[1][0].text;
    assert_eq!(word, "longword", "no hyphen without SHY: {word}");
    assert!(!word.contains('-'), "manual mode must not add hyphens without SHY: {word}");
}

// ── char_break_offset ────────────────────────────────────────────────────

#[test]
fn char_break_offset_all_fit() {
    struct Fixed8;
    impl super::super::super::TextMeasurer for Fixed8 {
        fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
    }
    // "abc" = 3 chars × 8px = 24px; avail = 100 → whole word fits.
    let off = super::super::char_break_offset("abc", 100.0, 16.0, 0.0, &[], &Fixed8);
    assert_eq!(off, 3); // "abc".len() == 3
}

#[test]
fn char_break_offset_splits_after_second_char() {
    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }
    // "abcde", avail = 25px; "ab" = 20px fits, "abc" = 30px > 25 → split at 2.
    let off = super::super::char_break_offset("abcde", 25.0, 16.0, 0.0, &[], &Fixed10);
    assert_eq!(off, 2); // byte offset 2 = between 'b' and 'c'
}

#[test]
fn char_break_offset_emits_at_least_one_char() {
    struct Wide;
    impl super::super::super::TextMeasurer for Wide {
        fn char_width(&self, _: char, _: f32) -> f32 { 100.0 }
    }
    // avail = 5px, char width 100px — even first char doesn't fit.
    // Must return offset past first char to avoid infinite loop.
    let off = super::super::char_break_offset("abc", 5.0, 16.0, 0.0, &[], &Wide);
    assert_eq!(off, 1); // emit 'a' anyway
}

// ── text-wrap-mode: nowrap ────────────────────────────────────────────────

#[test]
fn text_wrap_mode_nowrap_no_line_break() {
    // text-wrap-mode: nowrap should prevent wrapping (like white-space: nowrap).
    // Container 50px wide, word each 8px × 5 chars = 40px ("Hello" + " " + "World").
    let html = "<p>Hello World</p>";
    let css = "p { width: 50px; text-wrap-mode: nowrap; }";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let root = super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
    fn find_inline_run(b: &super::super::LayoutBox) -> Option<&super::super::LayoutBox> {
        if matches!(b.kind, super::super::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(f) = find_inline_run(c) { return Some(f); } }
        None
    }
    let ir = find_inline_run(&root).expect("InlineRun not found");
    if let super::super::BoxKind::InlineRun { lines, .. } = &ir.kind {
        assert_eq!(lines.len(), 1, "text-wrap-mode:nowrap must produce 1 line, got {}", lines.len());
    }
}

// ── overflow-wrap: break-word ─────────────────────────────────────────────

#[test]
fn overflow_wrap_break_word_splits_long_word() {
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens, OverflowWrap, WordBreak};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let style = ComputedStyle::root();
    // "Superlongword" = 13 chars × 10px = 130px; max_width = 80px.
    // overflow-wrap: break-word should split it across lines.
    let seg = InlineSegment {
        text: "Superlongword".to_string(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };

    let m = Fixed10;
    let hp = NullHyphenationProvider;
    let lines = wrap_inline_run(
        &[seg], 80.0, 16.0, 0.0,
        Size::new(800.0, 600.0),
        &m, Hyphens::None, &hp,
        crate::style::WhiteSpace::Normal,
        WordBreak::Normal,
        OverflowWrap::BreakWord,
        crate::style::LineBreak::Auto,
    );
    // 13 chars at 10px = 130px > 80px, so must wrap.
    assert!(lines.len() >= 2, "expected multiple lines, got {}", lines.len());
    // No line should exceed max_width.
    for (i, line) in lines.iter().enumerate() {
        if let Some(last) = line.last() {
            let line_w = last.x + last.width;
            assert!(line_w <= 81.0, "line {} width {line_w} exceeds max_width 80", i);
        }
    }
    // All characters of "Superlongword" must appear in the output.
    let all_text: String = lines.iter().flat_map(|l| l.iter().map(|f| f.text.as_str())).collect();
    assert_eq!(all_text, "Superlongword", "all chars must be emitted: {all_text}");
}

// ── line-break: CJK wrapping (CSS Text L3 §5.5) ───────────────────────────

/// Wraps one CJK segment under the given `line-break` / `word-break` and
/// returns the text of each produced line.
#[cfg(test)]
fn wrap_cjk(
    text: &str,
    max_width: f32,
    line_break: crate::style::LineBreak,
    word_break: crate::style::WordBreak,
) -> Vec<String> {
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens, OverflowWrap};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let seg = InlineSegment {
        text: text.to_string(),
        style: ComputedStyle::root(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };
    let lines = wrap_inline_run(
        &[seg], max_width, 16.0, 0.0,
        Size::new(800.0, 600.0),
        &Fixed10, Hyphens::None, &NullHyphenationProvider,
        crate::style::WhiteSpace::Normal,
        word_break,
        OverflowWrap::Normal,
        line_break,
    );
    lines
        .iter()
        .map(|l| l.iter().map(|f| f.text.as_str()).collect::<String>())
        .collect()
}

#[test]
fn cjk_paragraph_wraps_without_spaces() {
    use crate::style::{LineBreak, WordBreak};
    // 8 ideographs × 10px = 80px into a 30px box → 3 chars per line.
    let lines = wrap_cjk("日本語版日本語版", 30.0, LineBreak::Auto, WordBreak::Normal);
    assert_eq!(lines, vec!["日本語", "版日本", "語版"]);
}

#[test]
fn cjk_wrap_keeps_all_text() {
    use crate::style::{LineBreak, WordBreak};
    let text = "日本語版日本語版";
    for w in [10.0_f32, 25.0, 55.0, 200.0] {
        let joined = wrap_cjk(text, w, LineBreak::Auto, WordBreak::Normal).concat();
        assert_eq!(joined, text, "text lost at max_width={w}");
    }
}

#[test]
fn cjk_wrap_respects_word_break_keep_all() {
    use crate::style::{LineBreak, WordBreak};
    // keep-all forbids breaking between ideographs — one overflowing line.
    let lines = wrap_cjk("日本語版日本語版", 30.0, LineBreak::Auto, WordBreak::KeepAll);
    assert_eq!(lines, vec!["日本語版日本語版"]);
}

#[test]
fn cjk_wrap_line_break_anywhere_overrides_keep_all() {
    use crate::style::{LineBreak, WordBreak};
    let lines = wrap_cjk("日本語版", 20.0, LineBreak::Anywhere, WordBreak::KeepAll);
    assert_eq!(lines, vec!["日本", "語版"]);
}

#[test]
fn cjk_wrap_never_starts_a_line_with_small_kana() {
    use crate::style::{LineBreak, WordBreak};
    // きゃきゃきゃ into 15px (1.5 chars): a break before ゃ is forbidden
    // unless `loose`, so `normal` keeps each きゃ pair together and lets it
    // overflow rather than starting a line with the small kana.
    let normal = wrap_cjk("きゃきゃきゃ", 15.0, LineBreak::Normal, WordBreak::Normal);
    assert_eq!(normal, vec!["きゃ", "きゃ", "きゃ"]);
    for line in &normal {
        assert!(!line.starts_with('ゃ'), "small kana must not start a line: {line}");
    }
    // `loose` takes the extra opportunity and fits one char per line.
    let loose = wrap_cjk("きゃきゃきゃ", 15.0, LineBreak::Loose, WordBreak::Normal);
    assert_eq!(loose, vec!["き", "ゃ", "き", "ゃ", "き", "ゃ"]);
}

#[test]
fn cjk_wrap_line_break_anywhere_splits_latin() {
    use crate::style::{LineBreak, WordBreak};
    let lines = wrap_cjk("Superlongword", 50.0, LineBreak::Anywhere, WordBreak::Normal);
    assert!(lines.len() >= 3, "expected char-level wrapping, got {lines:?}");
    assert_eq!(lines.concat(), "Superlongword");
}

#[test]
fn latin_wrapping_unchanged_by_line_break_auto() {
    use crate::style::{LineBreak, WordBreak};
    // A long Latin word has no opportunities under `auto` — it overflows
    // exactly as it did before CJK support.
    let lines = wrap_cjk("Superlongword", 50.0, LineBreak::Auto, WordBreak::Normal);
    assert_eq!(lines, vec!["Superlongword"]);
}

#[test]
fn cjk_wrap_lines_fit_max_width() {
    use crate::style::{LineBreak, WordBreak};
    let lines = wrap_cjk("日本語版日本語版日本語版", 45.0, LineBreak::Auto, WordBreak::Normal);
    for line in &lines {
        let w = line.chars().count() as f32 * 10.0;
        assert!(w <= 45.0, "line {line} is {w}px wide, max 45");
    }
}

// ── word-break: break-all ─────────────────────────────────────────────────

#[test]
fn word_break_break_all_breaks_at_current_position() {
    use lumen_core::ext::NullHyphenationProvider;
    use super::super::{InlineSegment, PseudoKind, wrap_inline_run};
    use crate::style::{ComputedStyle, Hyphens, OverflowWrap, WordBreak};
    use lumen_core::geom::Size;
    use lumen_dom::NodeId;

    struct Fixed10;
    impl super::super::super::TextMeasurer for Fixed10 {
        fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
    }

    let style = ComputedStyle::root();
    // Two words: "Hi" (20px) then "World" (50px). max_width = 60px.
    // Normal: "Hi" fits, gap(10)+50=80 > 60 → wrap → line2 = "World".
    // break-all: "Hi" fits; gap(10)+"World" → need 80 > 60 → char-break.
    //   avail at current pos = 60 - 20 - 10 = 30px → "Wor" (30px) fits.
    //   Emit "Wor" at end of line1, line2 = "ld".
    let seg = InlineSegment {
        text: "Hi World".to_string(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };

    let m = Fixed10;
    let hp = NullHyphenationProvider;
    let lines = wrap_inline_run(
        &[seg], 60.0, 16.0, 0.0,
        Size::new(800.0, 600.0),
        &m, Hyphens::None, &hp,
        crate::style::WhiteSpace::Normal,
        WordBreak::BreakAll,
        OverflowWrap::Normal,
        crate::style::LineBreak::Auto,
    );
    assert_eq!(lines.len(), 2, "expected 2 lines with break-all, got {}", lines.len());
    // All text must be preserved.
    let all_text: String = lines.iter()
        .flat_map(|l| l.iter().map(|f| f.text.as_str()))
        .collect::<Vec<_>>()
        .join(" "); // words may be merged by frag-merging
    assert!(all_text.contains("Hi"), "line1 must contain 'Hi': {all_text}");
    // Line 2 must have the remainder of "World".
    let line2_text: String = lines[1].iter().map(|f| f.text.as_str()).collect();
    assert!(!line2_text.is_empty(), "line2 must not be empty");
}

