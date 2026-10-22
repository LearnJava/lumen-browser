use super::*;

// ──────────────── :current / :past / :future (CSS Selectors L4 §11.4) ────────────────

/// `:current` (§11.4.1) — timed-text «active cue». Phase 0 без timed-text
/// runtime никакой элемент не считается current, правило не применяется.
#[test]
fn current_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        "<p>x</p>",
        "p:current { color: red; }",
        "p",
    );
    assert_eq!(c.r, 0);
}

/// `:past` (§11.4.2) — Phase 0 timed-text без runtime → always false.
#[test]
fn past_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        "<p>x</p>",
        "p:past { color: red; }",
        "p",
    );
    assert_eq!(c.r, 0);
}

/// `:future` (§11.4.3) — Phase 0 timed-text без runtime → always false.
#[test]
fn future_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        "<p>x</p>",
        "p:future { color: red; }",
        "p",
    );
    assert_eq!(c.r, 0);
}

/// Time-dim pseudo-classes specificity = class-level (0,1,0). Проверяем,
/// что `:not(:current)` матчит все элементы (классическая FOUC/initial-
/// state idiom — когда timed-text runtime появится, правило сбросится).
#[test]
fn not_current_matches_all_elements_in_phase_0() {
    let c = element_color(
        "<p>x</p>",
        "p:not(:current) { color: red; }",
        "p",
    );
    assert_eq!(c.r, 255);
}

/// То же для `:not(:past)`.
#[test]
fn not_past_matches_all_elements_in_phase_0() {
    let c = element_color(
        "<p>x</p>",
        "p:not(:past) { color: red; }",
        "p",
    );
    assert_eq!(c.r, 255);
}

/// То же для `:not(:future)`.
#[test]
fn not_future_matches_all_elements_in_phase_0() {
    let c = element_color(
        "<p>x</p>",
        "p:not(:future) { color: red; }",
        "p",
    );
    assert_eq!(c.r, 255);
}

// ─── Canvas background propagation (CSS Backgrounds L3 §2.11.2) ─────

pub(crate) fn html_and_body(root: &LayoutBox) -> (&LayoutBox, &LayoutBox) {
    let html = root
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Block))
        .expect("html box");
    let body = html
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::Block))
        .expect("body box");
    (html, body)
}

/// BUG-1103: propagation is a **used-value** effect on the canvas clear
/// color only, computed read-only by [`canvas_background_color`] — it must
/// not mutate either element's `ComputedStyle` (that mutation is exactly
/// what corrupted `getComputedStyle(body)`/`getComputedStyle(documentElement)`
/// before the fix). Each of these tests therefore asserts BOTH ends: the
/// canvas color a paint pass would clear to, and that `html`/`body` still
/// report their own authored values unchanged, as CSSOM requires.
#[test]
fn body_bg_propagates_to_html_when_html_has_none() {
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "body { background-color: red; }",
    );
    let (html, body) = html_and_body(&root);
    assert_eq!(
        canvas_background_color(&root),
        Some(Color { r: 255, g: 0, b: 0, a: 255 }),
        "канва должна очищаться в фон body"
    );
    assert_eq!(html.style.background_color, None, "html своего фона не приобретает");
    assert_eq!(
        body.style.background_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
        "body сохраняет собственный computed-style фон"
    );
}

/// BUG-1398: layout/paint/size/style containment on `<html>` or `<body>` cuts the propagation.
#[test]
fn containment_on_root_or_body_blocks_bg_propagation() {
    for who in ["html", "body"] {
        for c in ["layout", "paint", "size", "style", "strict", "content"] {
            let root = lay_full(
                "<html><body><p>x</p></body></html>",
                &format!("body {{ background-color: red; }} {who} {{ contain: {c}; }}"),
            );
            assert_eq!(canvas_background_color(&root), None, "{who} contain:{c}");
        }
    }
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "body { background-color: red; contain: inline-size; }",
    );
    assert_eq!(canvas_background_color(&root), Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "html { background-color: blue; contain: paint; } body { background-color: red; }",
    );
    assert_eq!(canvas_background_color(&root), Some(Color { r: 0, g: 0, b: 255, a: 255 }));
}

#[test]
fn html_with_own_bg_blocks_propagation() {
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "html { background-color: blue; } body { background-color: red; }",
    );
    let (html, body) = html_and_body(&root);
    assert_eq!(
        canvas_background_color(&root),
        Some(Color { r: 0, g: 0, b: 255, a: 255 }),
        "канва берёт фон html — propagation не сработала"
    );
    assert_eq!(
        html.style.background_color,
        Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 })),
        "html сохраняет свой фон"
    );
    assert_eq!(
        body.style.background_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
        "body тоже сохраняет — propagation не сработала"
    );
}

#[test]
fn body_bg_image_propagates_when_html_has_none() {
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "body { background-image: url(\"bg.png\"); }",
    );
    let (html, body) = html_and_body(&root);
    assert!(html.style.background_layers.is_empty(), "html своих слоёв не приобретает");
    assert!(
        body.style.background_layers.first().is_some_and(|l| {
            matches!(&l.image, BackgroundImage::Url(s) if s == "bg.png")
        }),
        "body сохраняет собственный background-image"
    );
}

#[test]
fn html_image_blocks_propagation_even_if_color_empty() {
    // У html есть background-image (color=None) — propagation в
    // `canvas_background_color` НЕ должна сработать, у body свой фон
    // остаётся собственным computed-style значением.
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "html { background-image: url(\"h.png\"); } body { background-color: red; }",
    );
    let (html, body) = html_and_body(&root);
    assert!(html.style.background_layers.first().is_some_and(|l| matches!(&l.image, BackgroundImage::Url(_))));
    assert_eq!(html.style.background_color, None);
    assert_eq!(
        body.style.background_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }))
    );
    // html блокирует опору канвы на body своим (пусть и не цветовым) фоном:
    // непрозрачного цвета у html нет, поэтому `None` (UA-дефолт), а не body.
    assert_eq!(canvas_background_color(&root), None);
}

#[test]
fn no_body_no_propagation() {
    // `<html>` без `<body>` — propagation noop, ничего не падает.
    let root = lay("<html><p>x</p></html>", "p { background-color: red; }");
    // Просто проверка, что layout не паникует и не выставляет фон
    // случайно: у root-Document-box-а нет background style-а.
    assert_eq!(root.style.background_color, None);
}

#[test]
fn fragment_without_html_skips_propagation() {
    // Bare-fragment без `<html>`/`<body>` — наш tree builder не
    // добавляет implicit-ы. propagation должна тихо пропустить.
    let root = lay("<p>x</p>", "p { background-color: red; }");
    assert_eq!(root.style.background_color, None);
    // p сохраняет свой фон (он не body, propagation не трогает).
    let p = first_element_child(&root);
    assert_eq!(
        p.style.background_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }))
    );
}

// ── HTML presentational hints: bgcolor / text (HTML5 §15) ──────────────

/// `<body bgcolor="red">` — presentational hint задаёт background-color.
/// Канва очищается в этот цвет (BUG-1103: без переноса в `html`'s
/// `ComputedStyle` — `body` остаётся собственником своего значения).
#[test]
fn body_bgcolor_attr_sets_background() {
    let root = lay_full("<html><body bgcolor=\"red\"><p>x</p></body></html>", "");
    let (html, body) = html_and_body(&root);
    assert_eq!(
        canvas_background_color(&root),
        Some(Color { r: 255, g: 0, b: 0, a: 255 }),
        "канва должна получить фон из bgcolor"
    );
    assert_eq!(html.style.background_color, None, "html своего фона не приобретает");
    assert_eq!(
        body.style.background_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
        "body сохраняет собственный computed-style фон из bgcolor"
    );
}

/// `<body bgcolor="ff0000">` — hashless hex принимается по HTML5 §2.4.6
/// legacy color algorithm.
#[test]
fn body_bgcolor_hashless_hex_accepted() {
    let root = lay_full("<html><body bgcolor=\"ff0000\"><p>x</p></body></html>", "");
    assert_eq!(
        canvas_background_color(&root),
        Some(Color { r: 255, g: 0, b: 0, a: 255 }),
        "hashless hex bgcolor должен распознаваться"
    );
}

/// `<table bgcolor="navy">` — bgcolor на table-элементе.
#[test]
fn table_bgcolor_attr_sets_background() {
    let root = lay("<body><table bgcolor=\"navy\"><tr><td>x</td></tr></table></body>", "");
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(
        table.style.background_color,
        Some(CssColor::Rgba(Color { r: 0, g: 0, b: 128, a: 255 })),
        "bgcolor на table должен задавать background-color"
    );
}

/// `<tr bgcolor="lime">` — bgcolor на tr-элементе.
#[test]
fn tr_bgcolor_attr_sets_background() {
    let root = lay("<body><table><tr bgcolor=\"lime\"><td>x</td></tr></table></body>", "");
    let body = &root;
    let table = first_element_child(body);
    // HTML5 parser inserts implicit <tbody>; navigate through it.
    let tbody = first_element_child(table);
    let tr = first_element_child(tbody);
    assert_eq!(
        tr.style.background_color,
        Some(CssColor::Rgba(Color { r: 0, g: 255, b: 0, a: 255 })),
        "bgcolor на tr должен задавать background-color"
    );
}

/// `<td bgcolor="#00f">` — bgcolor на td-элементе, short hex form.
#[test]
fn td_bgcolor_attr_sets_background() {
    let root = lay("<body><table><tr><td bgcolor=\"#00f\">x</td></tr></table></body>", "");
    let body = &root;
    let table = first_element_child(body);
    // HTML5 parser inserts implicit <tbody>; navigate through it.
    let tbody = first_element_child(table);
    let tr = first_element_child(tbody);
    let td = first_element_child(tr);
    assert_eq!(
        td.style.background_color,
        Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 })),
        "bgcolor на td должен задавать background-color"
    );
}

// ── table layout (BUG-006) ────────────────────────────────────────────────

/// Ячейки таблицы должны раскладываться горизонтально, не вертикально.
#[test]
fn table_cells_layout_horizontally() {
    let root = lay(
        "<body><table><tr>\
           <td style=\"width:100px;height:50px\"></td>\
           <td style=\"width:200px;height:50px\"></td>\
         </tr></table></body>",
        "body,table,tr,td { margin:0; padding:0; border:0 }",
    );
    let body = &root;
    let table = first_element_child(body);
    // HTML5 parser inserts implicit <tbody>; navigate through it.
    let tbody = first_element_child(table);
    let tr = first_element_child(tbody);
    assert!(
        matches!(tr.kind, BoxKind::TableRow),
        "<tr> должен иметь BoxKind::TableRow"
    );
    let cells: Vec<_> = tr
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::Block))
        .collect();
    assert_eq!(cells.len(), 2, "должно быть 2 ячейки");
    // Первая ячейка: x=0, w=100
    assert!((cells[0].rect.x - 0.0).abs() < 0.01, "первая ячейка x=0, получено {}", cells[0].rect.x);
    assert!((cells[0].rect.width - 100.0).abs() < 0.01, "первая ячейка w=100");
    // Вторая ячейка: x=100, w=200
    assert!((cells[1].rect.x - 100.0).abs() < 0.01, "вторая ячейка x=100, получено {}", cells[1].rect.x);
    assert!((cells[1].rect.width - 200.0).abs() < 0.01, "вторая ячейка w=200");
    // Высота строки = max(50, 50) = 50
    assert!((tr.rect.height - 50.0).abs() < 0.01, "высота строки 50px");
}

/// Строки таблицы стакаются вертикально (block-flow для `<table>`).
#[test]
fn table_rows_stack_vertically() {
    let root = lay(
        "<body><table><tr><td style=\"width:100px;height:40px\"></td></tr>\
                     <tr><td style=\"width:100px;height:60px\"></td></tr></table></body>",
        "body,table,tr,td { margin:0; padding:0; border:0 }",
    );
    let body = &root;
    let table = first_element_child(body);
    // HTML5 parser inserts implicit <tbody>; navigate through it.
    let tbody = first_element_child(table);
    let rows: Vec<_> = tbody
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::TableRow))
        .collect();
    assert_eq!(rows.len(), 2, "должно быть 2 строки");
    assert!((rows[0].rect.y - 0.0).abs() < 0.01, "первая строка y=0");
    assert!((rows[1].rect.y - 40.0).abs() < 0.01, "вторая строка y=40, получено {}", rows[1].rect.y);
}

/// Колонки выравниваются между строками — global column widths.
/// Row 1: col0=100px, col1=200px. Row 2: col0=80px, col1=250px.
/// Global: col0=max(100,80)=100, col1=max(200,250)=250.
/// All rows use the global widths, so both rows → col0=100, col1=250.
#[test]
fn table_global_column_widths_aligned() {
    let root = lay(
        "<body><table><tr>\
           <td style=\"width:100px;height:20px\"></td>\
           <td style=\"width:200px;height:20px\"></td>\
         </tr><tr>\
           <td style=\"width:80px;height:20px\"></td>\
           <td style=\"width:250px;height:20px\"></td>\
         </tr></table></body>",
        "body,table,tr,td { margin:0; padding:0; border:0 }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(matches!(table.kind, BoxKind::Table), "table должен иметь BoxKind::Table");
    // HTML5 parser inserts implicit <tbody>; rows are inside it.
    let tbody = first_element_child(table);
    let rows: Vec<_> = tbody.children.iter().filter(|c| matches!(c.kind, BoxKind::TableRow)).collect();
    assert_eq!(rows.len(), 2);
    let r1_cells: Vec<_> = rows[0].children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
    let r2_cells: Vec<_> = rows[1].children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
    // col0 global = max(100, 80) = 100 — both rows.
    assert!((r1_cells[0].rect.width - 100.0).abs() < 0.01, "r1 col0=100, got {}", r1_cells[0].rect.width);
    assert!((r2_cells[0].rect.width - 100.0).abs() < 0.01, "r2 col0=100 (global), got {}", r2_cells[0].rect.width);
    // col1 global = max(200, 250) = 250 — both rows.
    assert!((r1_cells[1].rect.width - 250.0).abs() < 0.01, "r1 col1=250 (global), got {}", r1_cells[1].rect.width);
    assert!((r2_cells[1].rect.width - 250.0).abs() < 0.01, "r2 col1=250 (global), got {}", r2_cells[1].rect.width);
}

/// `<table>` имеет BoxKind::Table (не Block).
#[test]
fn table_has_boxkind_table() {
    let root = lay("<body><table><tr><td>x</td></tr></table></body>", "");
    let body = &root;
    let table = first_element_child(body);
    assert!(
        matches!(table.kind, BoxKind::Table),
        "table должен быть BoxKind::Table, получено {:?}", table.kind
    );
}

/// `<tbody>` имеет BoxKind::TableRowGroup.
#[test]
fn tbody_has_boxkind_tablerowgroup() {
    let root = lay("<body><table><tbody><tr><td>x</td></tr></tbody></table></body>", "");
    let body = &root;
    let table = first_element_child(body);
    let tbody = first_element_child(table);
    assert!(
        matches!(tbody.kind, BoxKind::TableRowGroup),
        "tbody должен быть BoxKind::TableRowGroup, получено {:?}", tbody.kind
    );
}

/// Строки внутри `<tbody>` выравниваются вертикально через `<table>`.
#[test]
fn table_with_tbody_rows_stack_vertically() {
    let root = lay(
        "<body><table><tbody>\
           <tr><td style=\"width:100px;height:40px\"></td></tr>\
           <tr><td style=\"width:100px;height:60px\"></td></tr>\
         </tbody></table></body>",
        "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
    );
    let body = &root;
    let table = first_element_child(body);
    let tbody = first_element_child(table);
    let rows: Vec<_> = tbody.children.iter().filter(|c| matches!(c.kind, BoxKind::TableRow)).collect();
    assert_eq!(rows.len(), 2, "должно быть 2 строки");
    assert!((rows[0].rect.y - 0.0).abs() < 0.01, "первая строка y=0, got {}", rows[0].rect.y);
    assert!((rows[1].rect.y - 40.0).abs() < 0.01, "вторая строка y=40, got {}", rows[1].rect.y);
}

/// `<thead>` и `<tfoot>` должны иметь BoxKind::TableRowGroup.
#[test]
fn thead_tfoot_have_boxkind_tablerowgroup() {
    let root = lay(
        "<body><table>\
           <thead><tr><th>H</th></tr></thead>\
           <tfoot><tr><td>F</td></tr></tfoot>\
         </table></body>",
        "",
    );
    let body = &root;
    let table = first_element_child(body);
    let groups: Vec<_> = table.children.iter()
        .filter(|c| matches!(c.kind, BoxKind::TableRowGroup))
        .collect();
    assert_eq!(groups.len(), 2, "должно быть 2 row group (thead + tfoot)");
}

/// Колонки внутри tbody выравниваются глобально (через родительский table).
#[test]
fn table_tbody_global_col_widths() {
    let root = lay(
        "<body><table><tbody><tr>\
           <td style=\"width:120px;height:20px\"></td>\
           <td style=\"width:80px;height:20px\"></td>\
         </tr><tr>\
           <td style=\"width:60px;height:20px\"></td>\
           <td style=\"width:150px;height:20px\"></td>\
         </tr></tbody></table></body>",
        "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
    );
    let body = &root;
    let table = first_element_child(body);
    let tbody = first_element_child(table);
    let rows: Vec<_> = tbody.children.iter().filter(|c| matches!(c.kind, BoxKind::TableRow)).collect();
    let r1: Vec<_> = rows[0].children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
    let r2: Vec<_> = rows[1].children.iter().filter(|c| matches!(c.kind, BoxKind::Block)).collect();
    // Col0 global = max(120, 60) = 120 — both rows.
    assert!((r1[0].rect.width - 120.0).abs() < 0.01, "r1 col0=120, got {}", r1[0].rect.width);
    assert!((r2[0].rect.width - 120.0).abs() < 0.01, "r2 col0=120 (global), got {}", r2[0].rect.width);
    // Col1 global = max(80, 150) = 150 — both rows.
    assert!((r1[1].rect.width - 150.0).abs() < 0.01, "r1 col1=150 (global), got {}", r1[1].rect.width);
    assert!((r2[1].rect.width - 150.0).abs() < 0.01, "r2 col1=150 (global), got {}", r2[1].rect.width);
}

// ── colspan / rowspan ────────────────────────────────────────────────────
// All table tests use explicit <tbody> because html-full-tree-builder
// correctly injects implicit <tbody> for bare <table><tr> markup (BUG-040).

/// `col_span` and `row_span` are stored on the LayoutBox from HTML attrs.
#[test]
fn table_cell_col_span_row_span_stored() {
    let root = lay(
        "<body><table><tbody><tr>\
           <td colspan=\"3\" rowspan=\"2\"></td>\
         </tr></tbody></table></body>",
        "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
    );
    let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
    let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
    let row = find_box(tbody, |k| matches!(k, BoxKind::TableRow)).unwrap();
    let cell = row.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    assert_eq!(cell.col_span, 3, "colspan=3 must be stored");
    assert_eq!(cell.row_span, 2, "rowspan=2 must be stored");
}

/// Non-cell boxes have col_span=1, row_span=1 by default.
#[test]
fn non_cell_col_row_span_defaults_to_one() {
    // `lay` returns the body box directly, so the <div> is its first
    // element child (no intermediate <html>/<body> unwrapping needed).
    let root = lay("<body><div></div></body>", "");
    let div = first_element_child(&root);
    assert_eq!(div.col_span, 1);
    assert_eq!(div.row_span, 1);
}

/// `<td colspan="2">` spanning two equal-width columns gets combined width.
#[test]
fn table_colspan2_cell_width() {
    // Row 1 sets col widths: col0=100, col1=100.
    // Row 2 has a single cell with colspan=2 → width should be 200.
    let root = lay(
        "<body><table><tbody>\
           <tr><td style=\"width:100px;height:20px\"></td>\
               <td style=\"width:100px;height:20px\"></td></tr>\
           <tr><td colspan=\"2\" style=\"height:30px\"></td></tr>\
         </tbody></table></body>",
        "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
    );
    let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
    let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
    let rows: Vec<_> = tbody
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::TableRow))
        .collect();
    assert_eq!(rows.len(), 2);
    let r2_cells: Vec<_> = rows[1]
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::Block))
        .collect();
    assert_eq!(r2_cells.len(), 1, "colspan=2 row must have exactly 1 DOM cell");
    assert!(
        (r2_cells[0].rect.width - 200.0).abs() < 0.01,
        "colspan=2 cell width should be 200px, got {}",
        r2_cells[0].rect.width
    );
    assert!(
        (r2_cells[0].rect.x - 0.0).abs() < 0.01,
        "colspan=2 cell x should be 0, got {}",
        r2_cells[0].rect.x
    );
}

/// Cell after a `colspan=2` cell starts at column 2 (x = col0+col1).
#[test]
fn table_cell_after_colspan2_x_position() {
    // Row 1: col0=60, col1=80, col2=50.
    // Row 2: [colspan=2 cell → cols 0-1, width=140], [cell at col2, width=50].
    let root = lay(
        "<body><table><tbody>\
           <tr><td style=\"width:60px;height:20px\"></td>\
               <td style=\"width:80px;height:20px\"></td>\
               <td style=\"width:50px;height:20px\"></td></tr>\
           <tr><td colspan=\"2\" style=\"height:20px\"></td>\
               <td style=\"height:20px\"></td></tr>\
         </tbody></table></body>",
        "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
    );
    let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
    let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
    let rows: Vec<_> = tbody
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::TableRow))
        .collect();
    let r2_cells: Vec<_> = rows[1]
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::Block))
        .collect();
    assert_eq!(r2_cells.len(), 2, "row 2 should have 2 DOM cells");
    assert!(
        (r2_cells[0].rect.x - 0.0).abs() < 0.01,
        "colspan cell x=0, got {}",
        r2_cells[0].rect.x
    );
    assert!(
        (r2_cells[0].rect.width - 140.0).abs() < 0.01,
        "colspan=2 width=140, got {}",
        r2_cells[0].rect.width
    );
    assert!(
        (r2_cells[1].rect.x - 140.0).abs() < 0.01,
        "cell after colspan x=140, got {}",
        r2_cells[1].rect.x
    );
    assert!(
        (r2_cells[1].rect.width - 50.0).abs() < 0.01,
        "cell after colspan width=50, got {}",
        r2_cells[1].rect.width
    );
}

/// `colspan=2 width=200` distributes 100px hint per column;
/// an explicit 120px col0 in another row wins over the 100px hint.
#[test]
fn table_colspan_distributes_width_hint() {
    let root = lay(
        "<body><table><tbody>\
           <tr><td style=\"width:120px;height:20px\"></td>\
               <td style=\"height:20px\"></td></tr>\
           <tr><td colspan=\"2\" style=\"width:200px;height:20px\"></td></tr>\
         </tbody></table></body>",
        "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
    );
    let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
    let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
    let rows: Vec<_> = tbody
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::TableRow))
        .collect();
    let r1_cells: Vec<_> = rows[0]
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::Block))
        .collect();
    // col0 = max(120, 100) = 120; col1 = max(auto→0, 100) = 100
    assert!(
        (r1_cells[0].rect.width - 120.0).abs() < 0.01,
        "col0 should be 120, got {}",
        r1_cells[0].rect.width
    );
    assert!(
        (r1_cells[1].rect.width - 100.0).abs() < 0.01,
        "col1 hint from colspan should be 100, got {}",
        r1_cells[1].rect.width
    );
}

/// `rowspan=2` in row 1 occupies col0 for both rows;
/// row 2's cell must be placed at col1, not col0.
#[test]
fn table_rowspan2_second_row_skips_occupied_column() {
    let root = lay(
        "<body><table><tbody>\
           <tr><td rowspan=\"2\" style=\"width:80px;height:20px\"></td>\
               <td style=\"width:60px;height:20px\"></td></tr>\
           <tr><td style=\"width:60px;height:20px\"></td></tr>\
         </tbody></table></body>",
        "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
    );
    let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
    let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
    let rows: Vec<_> = tbody
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::TableRow))
        .collect();
    let r2_cells: Vec<_> = rows[1]
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::Block))
        .collect();
    assert_eq!(r2_cells.len(), 1, "row 2 has 1 DOM cell");
    assert!(
        (r2_cells[0].rect.x - 80.0).abs() < 0.01,
        "row2 cell must start at x=80 (col1), got {}",
        r2_cells[0].rect.x
    );
    assert!(
        (r2_cells[0].rect.width - 60.0).abs() < 0.01,
        "row2 cell width=60, got {}",
        r2_cells[0].rect.width
    );
}

/// After layout, a `rowspan=2` cell's height is patched to cover both rows.
#[test]
fn table_rowspan2_cell_height_spans_two_rows() {
    // Row1: [A(rowspan=2,h=10), B(h=30)] → row1_h=30.
    // Row2: [C(h=40)] → row2_h=40.
    // A.height post-fix = row1.y+row1.h + row2.h - A.y = 30+40 = 70.
    let root = lay(
        "<body><table><tbody>\
           <tr><td rowspan=\"2\" style=\"width:50px;height:10px\"></td>\
               <td style=\"width:50px;height:30px\"></td></tr>\
           <tr><td style=\"width:50px;height:40px\"></td></tr>\
         </tbody></table></body>",
        "body,table,tbody,tr,td { margin:0; padding:0; border:0 }",
    );
    let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
    let tbody = find_box(table, |k| matches!(k, BoxKind::TableRowGroup)).unwrap();
    let rows: Vec<_> = tbody
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::TableRow))
        .collect();
    let row1_cells: Vec<_> = rows[0]
        .children
        .iter()
        .filter(|c| matches!(c.kind, BoxKind::Block))
        .collect();
    let cell_a = row1_cells[0];
    let row1_h = rows[0].rect.height;
    let row2_h = rows[1].rect.height;
    assert!(
        (row1_h - 30.0).abs() < 0.01,
        "row1 height should be 30 (from B), got {}",
        row1_h
    );
    assert!(
        (row2_h - 40.0).abs() < 0.01,
        "row2 height should be 40 (from C), got {}",
        row2_h
    );
    let expected_a_h = row1_h + row2_h;
    assert!(
        (cell_a.rect.height - expected_a_h).abs() < 0.01,
        "rowspan=2 cell A height should be {}, got {}",
        expected_a_h,
        cell_a.rect.height
    );
}

/// CSS 2.1 §17.5.2 — table without explicit CSS width shrinks to fit its columns.
/// 3×3 grid with border-spacing:12px and cell width:60px should be 228px
/// (3×60 + 4×12), not the full container width.
#[test]
fn table_without_explicit_width_shrinks_to_fit() {
    let root = lay(
        "<body><table><tr>\
           <td style=\"width:60px;height:20px\"></td>\
           <td style=\"width:60px;height:20px\"></td>\
           <td style=\"width:60px;height:20px\"></td>\
         </tr></table></body>",
        "body { width:800px } table { border-spacing:12px } td { margin:0; padding:0 }",
    );
    let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
    // Expected: 3×60 + 4×12 = 180 + 48 = 228px
    assert!(
        (table.rect.width - 228.0).abs() < 0.01,
        "table should shrink to 228px, got {}",
        table.rect.width
    );
}

/// CSS 2.1 §17.5.2 — table with explicit CSS width is NOT shrunk to fit.
#[test]
fn table_with_explicit_width_keeps_that_width() {
    let root = lay(
        "<body><table><tr>\
           <td style=\"width:60px;height:20px\"></td>\
           <td style=\"width:60px;height:20px\"></td>\
         </tr></table></body>",
        "body { width:800px } table { width:400px; border-spacing:8px } td { margin:0; padding:0 }",
    );
    let table = find_box(&root, |k| matches!(k, BoxKind::Table)).unwrap();
    assert!(
        (table.rect.width - 400.0).abs() < 0.01,
        "table with explicit width:400px should stay 400px, got {}",
        table.rect.width
    );
}

/// Author CSS `background-color` выигрывает у presentational hint `bgcolor`.
#[test]
fn author_css_overrides_bgcolor_hint() {
    let root = lay_full(
        "<html><body bgcolor=\"red\"><p>x</p></body></html>",
        "body { background-color: blue; }",
    );
    let (_html, body) = html_and_body(&root);
    assert_eq!(
        body.style.background_color,
        Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 })),
        "author CSS background-color должен побеждать bgcolor атрибут"
    );
}

/// `<body bgcolor="transparent">` — по HTML5 §2.4.6 «transparent» является
/// ошибкой; атрибут игнорируется, фон остаётся None.
#[test]
fn body_bgcolor_transparent_is_ignored() {
    let root = lay_full("<html><body bgcolor=\"transparent\"><p>x</p></body></html>", "");
    let (html, body) = html_and_body(&root);
    assert_eq!(html.style.background_color, None, "transparent bgcolor должен игнорироваться");
    assert_eq!(body.style.background_color, None);
}

/// `<body bgcolor="olive">` — named color через HTML5 legacy-парсер.
#[test]
fn body_bgcolor_named_color() {
    let root = lay_full("<html><body bgcolor=\"olive\"><p>x</p></body></html>", "");
    assert_eq!(
        canvas_background_color(&root),
        Some(Color { r: 128, g: 128, b: 0, a: 255 }),
        "named color 'olive' должен правильно конвертироваться"
    );
}

// ── HTML presentational hints: body text / font color (HTML5 §15.3) ────

/// `<body text="red">` → body.color = red.
#[test]
fn body_text_attr_sets_color() {
    let root = lay_full("<html><body text=\"red\"><p>x</p></body></html>", "");
    let (_html, body) = html_and_body(&root);
    assert_eq!(
        body.style.color,
        Color { r: 255, g: 0, b: 0, a: 255 },
        "body text= должен задавать color"
    );
}

/// `<body text="blue">` — цвет наследуется дочерними элементами.
#[test]
fn body_text_color_inherited_by_child() {
    let root = lay_full("<html><body text=\"blue\"><p>x</p></body></html>", "");
    let (_html, body) = html_and_body(&root);
    let p = first_element_child(body);
    assert_eq!(
        p.style.color,
        Color { r: 0, g: 0, b: 255, a: 255 },
        "<p> должен наследовать color из body text="
    );
}

/// Author CSS `color` выигрывает у presentational hint `text=`.
#[test]
fn author_css_overrides_body_text_hint() {
    let root = lay_full(
        "<html><body text=\"red\"><p>x</p></body></html>",
        "body { color: green; }",
    );
    let (_html, body) = html_and_body(&root);
    assert_eq!(
        body.style.color,
        Color { r: 0, g: 128, b: 0, a: 255 },
        "author CSS color должен побеждать body text= атрибут"
    );
}

/// `<font color="red">` задаёт color на элементе font.
#[test]
fn font_color_attr_sets_color() {
    let root = lay("<body><font color=\"red\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.color,
        Color { r: 255, g: 0, b: 0, a: 255 },
        "<font color=> должен задавать color"
    );
}

/// `<font color="#0000ff">` — hash long hex form.
#[test]
fn font_color_hash_long_hex() {
    let root = lay("<body><font color=\"#0000ff\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.color,
        Color { r: 0, g: 0, b: 255, a: 255 },
        "<font color=#0000ff> должен задавать blue"
    );
}

/// Author CSS `color` выигрывает у `<font color=>`.
#[test]
fn author_css_overrides_font_color_hint() {
    let root = lay(
        "<body><font color=\"red\">x</font></body>",
        "font { display: block; color: blue; }",
    );
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.color,
        Color { r: 0, g: 0, b: 255, a: 255 },
        "author CSS должен побеждать font color= атрибут"
    );
}

// ── HTML presentational hints: <font size/face>, img hspace/vspace/border, align ──

/// `<font size="3">` → font-size 16px (medium).
#[test]
fn font_size_attr_medium() {
    let root = lay("<body><font size=\"3\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.font_size, 16.0,
        "<font size=3> должен задавать font-size 16px"
    );
}

/// `<font size="1">` → font-size 10px (xx-small).
#[test]
fn font_size_attr_xxsmall() {
    let root = lay("<body><font size=\"1\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.font_size, 10.0,
        "<font size=1> должен задавать font-size 10px"
    );
}

/// `<font size="7">` → font-size 48px (xxx-large).
#[test]
fn font_size_attr_xxxlarge() {
    let root = lay("<body><font size=\"7\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.font_size, 48.0,
        "<font size=7> должен задавать font-size 48px"
    );
}

/// `<font size="+2">` → base 3 + 2 = 5 → 24px.
#[test]
fn font_size_attr_relative_plus() {
    let root = lay("<body><font size=\"+2\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.font_size, 24.0,
        "<font size=+2> должен задавать font-size 24px"
    );
}

/// `<font size="-1">` → base 3 - 1 = 2 → 13px.
#[test]
fn font_size_attr_relative_minus() {
    let root = lay("<body><font size=\"-1\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.font_size, 13.0,
        "<font size=-1> должен задавать font-size 13px"
    );
}

/// `<font size="99">` clamps to 7 → 48px.
#[test]
fn font_size_attr_clamp_max() {
    let root = lay("<body><font size=\"99\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.font_size, 48.0,
        "<font size=99> должен клэмпироваться к 48px"
    );
}

/// Author CSS `font-size` побеждает `<font size>` hint.
#[test]
fn author_css_overrides_font_size_hint() {
    let root = lay(
        "<body><font size=\"7\">x</font></body>",
        "font { display: block; font-size: 20px; }",
    );
    let body = &root;
    let font = first_element_child(body);
    assert_eq!(
        font.style.font_size, 20.0,
        "author CSS font-size должен побеждать font size= атрибут"
    );
}

/// `<font face="Arial, sans-serif">` → font-family.
#[test]
fn font_face_attr_sets_font_family() {
    let root = lay("<body><font face=\"Arial, sans-serif\">x</font></body>", "font { display: block; }");
    let body = &root;
    let font = first_element_child(body);
    assert!(
        font.style.font_family.contains(&"Arial".to_string()),
        "<font face=> должен задавать font-family"
    );
}

/// `<img hspace="10">` → margin-left и margin-right по 10px.
#[test]
fn img_hspace_attr_sets_margins() {
    let root = lay(r#"<img src="x.png" hspace="10">"#, "");
    let img = first_image_child(&root);
    assert_eq!(
        img.style.margin_left,
        LengthOrAuto::Length(Length::Px(10.0)),
        "img hspace должен задавать margin-left 10px"
    );
    assert_eq!(
        img.style.margin_right,
        LengthOrAuto::Length(Length::Px(10.0)),
        "img hspace должен задавать margin-right 10px"
    );
}

/// `<img vspace="8">` → margin-top и margin-bottom по 8px.
#[test]
fn img_vspace_attr_sets_margins() {
    let root = lay(r#"<img src="x.png" vspace="8">"#, "");
    let img = first_image_child(&root);
    assert_eq!(
        img.style.margin_top,
        LengthOrAuto::Length(Length::Px(8.0)),
        "img vspace должен задавать margin-top 8px"
    );
    assert_eq!(
        img.style.margin_bottom,
        LengthOrAuto::Length(Length::Px(8.0)),
        "img vspace должен задавать margin-bottom 8px"
    );
}

/// `<img border="2">` → все 4 border-width 2px + style=solid.
#[test]
fn img_border_attr_sets_border() {
    let root = lay(r#"<img src="x.png" border="2">"#, "");
    let img = first_image_child(&root);
    assert_eq!(img.style.border_top_width, 2.0, "img border должен задавать border-top-width 2px");
    assert_eq!(img.style.border_right_width, 2.0);
    assert_eq!(img.style.border_bottom_width, 2.0);
    assert_eq!(img.style.border_left_width, 2.0);
    assert_eq!(
        img.style.border_top_style,
        crate::style::BorderStyle::Solid,
        "img border>0 должен задавать border-style solid"
    );
}

/// `<img border="0">` → нулевые border-width, style=none (no-op).
#[test]
fn img_border_zero_no_style() {
    let root = lay(r#"<img src="x.png" border="0">"#, "");
    let img = first_image_child(&root);
    assert_eq!(img.style.border_top_width, 0.0);
    assert_eq!(
        img.style.border_top_style,
        crate::style::BorderStyle::None,
        "img border=0 не должен задавать border-style"
    );
}

/// `<div align="center">` → text-align: center.
#[test]
fn div_align_center_attr() {
    let root = lay("<body><div align=\"center\">x</div></body>", "");
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(
        div.style.text_align,
        crate::style::TextAlign::Center,
        "div align=center должен задавать text-align: center"
    );
}

/// `<p align="right">` → text-align: right.
#[test]
fn p_align_right_attr() {
    let root = lay("<body><p align=\"right\">x</p></body>", "");
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(
        p.style.text_align,
        crate::style::TextAlign::Right,
        "p align=right должен задавать text-align: right"
    );
}

/// `<h1 align="middle">` → text-align: center (middle = center alias).
#[test]
fn h1_align_middle_is_center() {
    let root = lay("<body><h1 align=\"middle\">x</h1></body>", "");
    let body = &root;
    let h1 = first_element_child(body);
    assert_eq!(
        h1.style.text_align,
        crate::style::TextAlign::Center,
        "align=middle должен давать text-align: center"
    );
}

/// Author CSS `text-align` побеждает `align` атрибут.
#[test]
fn author_css_overrides_align_hint() {
    let root = lay(
        "<body><div align=\"center\">x</div></body>",
        "div { text-align: right; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(
        div.style.text_align,
        crate::style::TextAlign::Right,
        "author CSS text-align должен побеждать align= атрибут"
    );
}
// ─── Principal writing mode (CSS Writing Modes L3 §8, BUG-1274) ─────

#[test]
fn body_vertical_rl_is_principal_mode_and_starts_at_right_edge() {
    let root = lay_full(
        r#"<html><body style="writing-mode:vertical-rl;margin:0"><div style="width:100px;height:100px"></div></body></html>"#,
        "",
    );
    let (html, body) = html_and_body(&root);
    assert_eq!(html.style.writing_mode, crate::style::WritingMode::VerticalRl, "режим body переходит на html");
    assert_eq!(body.rect.x, 700.0, "блочный поток вертикального документа идёт от правого края");
    assert_eq!(html.rect.x + html.rect.width, 800.0);
}

// ── BUG-1363: canvas background from a root with position/display other than block ──

/// CSS Backgrounds 3 §2.11.2: the root element's background paints the whole canvas
/// whatever the root's own `position`/`display` (WPT `abspos-containing-block-initial-004*`).
#[test]
fn canvas_bg_from_abspos_root() {
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "html { position: absolute; left: 100px; top: 100px; width: 100px; height: 100px; \
         background: yellow; border: 10px solid black; }",
    );
    assert_eq!(
        canvas_background_color(&root),
        Some(Color { r: 255, g: 255, b: 0, a: 255 }),
    );
}

#[test]
fn canvas_bg_from_abspos_table_root() {
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "html { position: absolute; display: table; left: 100px; top: 100px; \
         background: yellow; border: 10px solid black; }",
    );
    assert_eq!(
        canvas_background_color(&root),
        Some(Color { r: 255, g: 255, b: 0, a: 255 }),
    );
}

#[test]
fn canvas_bg_from_table_root_with_body_bg() {
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "html { display: table; } body { background: lime; }",
    );
    assert_eq!(
        canvas_background_color(&root),
        Some(Color { r: 0, g: 255, b: 0, a: 255 }),
    );
}
