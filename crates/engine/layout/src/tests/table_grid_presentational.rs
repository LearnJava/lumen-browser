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

#[test]
fn body_bg_propagates_to_html_when_html_has_none() {
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "body { background-color: red; }",
    );
    let (html, body) = html_and_body(&root);
    assert_eq!(
        html.style.background_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
        "html должен получить фон body"
    );
    assert_eq!(
        body.style.background_color, None,
        "у body фон обнуляется после propagation"
    );
}

#[test]
fn html_with_own_bg_blocks_propagation() {
    let root = lay_full(
        "<html><body><p>x</p></body></html>",
        "html { background-color: blue; } body { background-color: red; }",
    );
    let (html, body) = html_and_body(&root);
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
    assert!(
        html.style.background_layers.first().is_some_and(|l| {
            matches!(&l.image, BackgroundImage::Url(s) if s == "bg.png")
        }),
        "html получает background-image"
    );
    assert!(body.style.background_layers.is_empty(), "у body background_layers обнуляется");
}

#[test]
fn html_image_blocks_propagation_even_if_color_empty() {
    // У html есть background-image (color=None) — propagation НЕ должна
    // сработать, у body свой фон остаётся.
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
/// После canvas-propagation фон переходит на html-box.
#[test]
fn body_bgcolor_attr_sets_background() {
    let root = lay_full("<html><body bgcolor=\"red\"><p>x</p></body></html>", "");
    let (html, body) = html_and_body(&root);
    assert_eq!(
        html.style.background_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
        "html должен получить фон из bgcolor после propagation"
    );
    assert_eq!(body.style.background_color, None, "body фон обнуляется после propagation");
}

/// `<body bgcolor="ff0000">` — hashless hex принимается по HTML5 §2.4.6
/// legacy color algorithm.
#[test]
fn body_bgcolor_hashless_hex_accepted() {
    let root = lay_full("<html><body bgcolor=\"ff0000\"><p>x</p></body></html>", "");
    let (html, _body) = html_and_body(&root);
    assert_eq!(
        html.style.background_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })),
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
    let (html, _body) = html_and_body(&root);
    assert_eq!(
        html.style.background_color,
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
    let (html, _body) = html_and_body(&root);
    assert_eq!(
        html.style.background_color,
        Some(CssColor::Rgba(Color { r: 128, g: 128, b: 0, a: 255 })),
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
    let root = lay("<body><font color=\"red\">x</font></body>", "");
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
    let root = lay("<body><font color=\"#0000ff\">x</font></body>", "");
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
        "font { color: blue; }",
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
    let root = lay("<body><font size=\"3\">x</font></body>", "");
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
    let root = lay("<body><font size=\"1\">x</font></body>", "");
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
    let root = lay("<body><font size=\"7\">x</font></body>", "");
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
    let root = lay("<body><font size=\"+2\">x</font></body>", "");
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
    let root = lay("<body><font size=\"-1\">x</font></body>", "");
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
    let root = lay("<body><font size=\"99\">x</font></body>", "");
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
        "font { font-size: 20px; }",
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
    let root = lay("<body><font face=\"Arial, sans-serif\">x</font></body>", "");
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

// --- CSS Grid Layout tests ---

/// Parse `grid-template-columns: 100px 200px 300px`.
#[test]
fn grid_parse_fixed_columns() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: 100px 200px 300px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_template_columns.len(), 3);
    assert_eq!(div.style.grid_template_columns[0], GridTrackSize::Length(Length::Px(100.0)));
    assert_eq!(div.style.grid_template_columns[1], GridTrackSize::Length(Length::Px(200.0)));
    assert_eq!(div.style.grid_template_columns[2], GridTrackSize::Length(Length::Px(300.0)));
}

/// Parse fr units.
#[test]
fn grid_parse_fr_columns() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: 1fr 2fr; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_template_columns.len(), 2);
    assert_eq!(div.style.grid_template_columns[0], GridTrackSize::Fr(1.0));
    assert_eq!(div.style.grid_template_columns[1], GridTrackSize::Fr(2.0));
}

/// Parse `repeat(3, 100px)` — expands to 3 tracks.
#[test]
fn grid_parse_repeat() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: repeat(3, 100px); }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_template_columns.len(), 3);
    for ts in &div.style.grid_template_columns {
        assert_eq!(*ts, GridTrackSize::Length(Length::Px(100.0)));
    }
}

/// Parse `grid-column: 2 / 4`.
#[test]
fn grid_parse_column_shorthand() {
    let root = lay(
        "<body><div></div></body>",
        "div { grid-column: 2 / 4; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_column_start, GridLine::Line(2));
    assert_eq!(div.style.grid_column_end, GridLine::Line(4));
}

/// Parse `grid-row: 1 / span 2`.
#[test]
fn grid_parse_row_span() {
    let root = lay(
        "<body><div></div></body>",
        "div { grid-row: 1 / span 2; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_row_start, GridLine::Line(1));
    assert_eq!(div.style.grid_row_end, GridLine::Span(2));
}

/// Two equal fr columns should each get half the container width.
#[test]
fn grid_two_fr_columns_equal_width() {
    let root = lay(
        "<body><div><span></span><span></span></div></body>",
        "div { display: grid; grid-template-columns: 1fr 1fr; width: 400px; } \
         span { height: 50px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 2, "должно быть 2 grid-item");
    assert!((items[0].rect.width - 200.0).abs() < 1.0, "первый item = 200px, получили {}", items[0].rect.width);
    assert!((items[1].rect.width - 200.0).abs() < 1.0, "второй item = 200px, получили {}", items[1].rect.width);
    // Second item starts at x=200.
    assert!((items[1].rect.x - items[0].rect.x - 200.0).abs() < 1.0);
}

/// Fixed 3-column grid: items placed in row order.
#[test]
fn grid_three_column_auto_placement() {
    let root = lay(
        "<body><div><a></a><a></a><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px 100px; width: 300px; } \
         a { height: 30px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 4);
    // First 3 items on row 1, 4th on row 2.
    assert!((items[0].rect.y - items[1].rect.y).abs() < 1.0, "items 0,1 одна строка");
    assert!((items[1].rect.y - items[2].rect.y).abs() < 1.0, "items 1,2 одна строка");
    assert!(items[3].rect.y > items[0].rect.y + 1.0, "item 4 на второй строке");
    // Column positions.
    assert!(items[0].rect.x < items[1].rect.x, "col 0 < col 1");
    assert!(items[1].rect.x < items[2].rect.x, "col 1 < col 2");
}

/// Explicit grid-column / grid-row placement.
#[test]
fn grid_explicit_placement() {
    let root = lay(
        "<body><div><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px 100px; \
               grid-template-rows: 50px 50px; width: 300px; } \
         a { grid-column: 3; grid-row: 2; height: 40px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let item = div.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip)).unwrap();
    // item at column 3, row 2 → x ≈ 200, y ≈ 50.
    assert!((item.rect.x - 200.0).abs() < 1.0, "x≈200, got {}", item.rect.x);
    assert!((item.rect.y - 50.0).abs() < 1.0, "y≈50, got {}", item.rect.y);
}

/// Grid with `gap` between cells.
#[test]
fn grid_gap_applied() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; \
               column-gap: 20px; width: 220px; } \
         a { height: 30px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 2);
    // Second item starts at x ≈ 120 (100px col + 20px gap).
    assert!((items[1].rect.x - items[0].rect.x - 120.0).abs() < 1.0,
        "gap: x diff should be 120, got {}", items[1].rect.x - items[0].rect.x);
}

// ──── grid content distribution: align-content / justify-content / place-content
//      (CSS Box Alignment L3 §5, CSS Grid L1 §12.3) ────

/// Grid items of the container, in source order, excluding `Skip` boxes.
fn grid_items(div: &LayoutBox) -> Vec<&LayoutBox> {
    div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect()
}

/// `justify-content: center` centres the fixed column tracks in the inline axis.
#[test]
fn grid_justify_content_center_offsets_columns() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; width: 400px; \
               justify-content: center; } \
         a { height: 30px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert_eq!(items.len(), 2);
    // Free space 400 - 200 = 200 → first track pushed by 100.
    assert!((items[0].rect.x - div.rect.x - 100.0).abs() < 1.0,
        "first column offset by half the free space, got {}", items[0].rect.x - div.rect.x);
    assert!((items[1].rect.x - items[0].rect.x - 100.0).abs() < 1.0,
        "tracks stay adjacent, got {}", items[1].rect.x - items[0].rect.x);
}

/// `justify-content: end` flushes the column tracks to the inline end edge.
#[test]
fn grid_justify_content_end_flushes_columns() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; width: 400px; \
               justify-content: end; } \
         a { height: 30px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert!((items[0].rect.x - div.rect.x - 200.0).abs() < 1.0,
        "first column offset by the whole free space, got {}", items[0].rect.x - div.rect.x);
}

/// `justify-content: space-between` widens the gap between the column tracks.
#[test]
fn grid_justify_content_space_between_widens_gap() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; width: 400px; \
               justify-content: space-between; } \
         a { height: 30px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert!((items[0].rect.x - div.rect.x).abs() < 1.0,
        "first column at the start edge, got {}", items[0].rect.x - div.rect.x);
    // 200px of free space becomes the single in-between gap.
    assert!((items[1].rect.x - items[0].rect.x - 300.0).abs() < 1.0,
        "second column pushed by track + free space, got {}", items[1].rect.x - items[0].rect.x);
}

/// A column-spanning item's cell absorbs the spacing `justify-content` injected.
#[test]
fn grid_justify_content_space_between_widens_spanning_cell() {
    let root = lay(
        "<body><div><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; width: 400px; \
               justify-content: space-between; } \
         a { grid-column: 1 / 3; height: 30px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    // Cell spans track 1 (100) + injected gap (200) + track 2 (100) = 400.
    assert!((items[0].rect.width - 400.0).abs() < 1.0,
        "spanning cell covers the widened gap, got {}", items[0].rect.width);
}

/// `align-content: center` centres the row tracks inside a definite height.
#[test]
fn grid_align_content_center_offsets_rows() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 50px 50px; height: 300px; \
               align-content: center; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert_eq!(items.len(), 2);
    // Free space 300 - 100 = 200 → first row pushed by 100.
    assert!((items[0].rect.y - div.rect.y - 100.0).abs() < 1.0,
        "first row offset by half the free space, got {}", items[0].rect.y - div.rect.y);
    assert!((items[1].rect.y - items[0].rect.y - 50.0).abs() < 1.0,
        "rows stay adjacent, got {}", items[1].rect.y - items[0].rect.y);
}

/// `align-content: space-evenly` spaces the row tracks including both edges.
#[test]
fn grid_align_content_space_evenly_spaces_rows() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 50px 50px; height: 350px; \
               align-content: space-evenly; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    // Free 350 - 100 = 250, three equal gaps of 250/3 ≈ 83.33.
    let per = 250.0 / 3.0;
    assert!((items[0].rect.y - div.rect.y - per).abs() < 1.0,
        "first row offset by one share, got {}", items[0].rect.y - div.rect.y);
    assert!((items[1].rect.y - items[0].rect.y - (50.0 + per)).abs() < 1.0,
        "rows separated by track + one share, got {}", items[1].rect.y - items[0].rect.y);
}

/// `align-content: center` on an overflowing grid resolves *unsafely* (CSS Box
/// Alignment L3 §5.3): the tracks shift back past the start edge, like Edge.
#[test]
fn grid_align_content_center_overflow_shifts_past_start_edge() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 100px 100px; height: 50px; \
               align-content: center; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    // Free space 50 - 200 = -150 → the first row starts 75px above the edge.
    assert!((items[0].rect.y - div.rect.y + 75.0).abs() < 1.0,
        "overflowing tracks centre unsafely, got {}", items[0].rect.y - div.rect.y);
}

/// `align-content: space-between` on an overflowing grid falls back to `start`
/// (CSS Box Alignment L3 §5.3) instead of producing negative spacing.
#[test]
fn grid_align_content_space_between_overflow_falls_back_to_start() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 100px 100px; height: 50px; \
               align-content: space-between; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert!((items[0].rect.y - div.rect.y).abs() < 1.0,
        "first row at the start edge, got {}", items[0].rect.y - div.rect.y);
    assert!((items[1].rect.y - items[0].rect.y - 100.0).abs() < 1.0,
        "rows stay adjacent, no negative spacing, got {}", items[1].rect.y - items[0].rect.y);
}

/// `place-content: <align-content> <justify-content>` reaches both grid axes.
#[test]
fn grid_place_content_shorthand_applies_to_both_axes() {
    let root = lay(
        "<body><div><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: 100px; width: 400px; height: 300px; \
               place-content: end center; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    // align-content: end → y offset by the whole 200px of block free space.
    assert!((items[0].rect.y - div.rect.y - 200.0).abs() < 1.0,
        "align-content: end flushes the row, got {}", items[0].rect.y - div.rect.y);
    // justify-content: center → x offset by half the 300px of inline free space.
    assert!((items[0].rect.x - div.rect.x - 150.0).abs() < 1.0,
        "justify-content: center centres the column, got {}", items[0].rect.x - div.rect.x);
}

/// Default `align-content: normal` behaves as `stretch`: auto rows share the
/// leftover block space of a definitely-sized container.
#[test]
fn grid_align_content_normal_stretches_auto_rows() {
    let root = lay(
        "<body><div><a></a><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; \
               grid-template-rows: auto auto; height: 200px; }",
    );
    let div = first_element_child(&root);
    let items = grid_items(div);
    assert_eq!(items.len(), 2);
    // Both auto rows grow to 100px each → the second row starts halfway down.
    assert!((items[1].rect.y - items[0].rect.y - 100.0).abs() < 1.0,
        "auto rows share the free space, got {}", items[1].rect.y - items[0].rect.y);
}

/// `grid-auto-flow: column` places items vertically first.
#[test]
fn grid_auto_flow_column() {
    let root = lay(
        "<body><div><a></a><a></a><a></a></div></body>",
        "div { display: grid; grid-template-rows: 50px 50px; \
               grid-auto-flow: column; width: 300px; } \
         a { width: 80px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div.children.iter().filter(|c| !matches!(c.kind, BoxKind::Skip)).collect();
    assert_eq!(items.len(), 3);
    // items 0,1 same column (different y); item 2 in next column.
    assert!((items[0].rect.x - items[1].rect.x).abs() < 1.0, "items 0,1 same column");
    assert!(items[2].rect.x > items[0].rect.x + 1.0, "item 2 next column");
}

/// `minmax(50px, 1fr)` — explicit minmax() track.
#[test]
fn grid_parse_minmax() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: minmax(50px, 1fr); }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_template_columns.len(), 1);
    assert!(matches!(div.style.grid_template_columns[0], GridTrackSize::Minmax(_, _)));
}

/// `grid-area` shorthand parses `row-start / col-start / row-end / col-end`.
#[test]
fn grid_parse_area_shorthand() {
    let root = lay(
        "<body><div></div></body>",
        "div { grid-area: 2 / 1 / 4 / 3; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_row_start, GridLine::Line(2));
    assert_eq!(div.style.grid_column_start, GridLine::Line(1));
    assert_eq!(div.style.grid_row_end, GridLine::Line(4));
    assert_eq!(div.style.grid_column_end, GridLine::Line(3));
}

/// `display: grid` container has no height when empty.
#[test]
fn grid_empty_container_zero_height() {
    let root = lay(
        "<body><div></div></body>",
        "div { display: grid; grid-template-columns: 100px 100px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.rect.height, 0.0, "empty grid should have 0 height");
}

/// Auto rows sized by content.
#[test]
fn grid_auto_row_height_from_content() {
    let root = lay(
        "<body><div><a></a></div></body>",
        "div { display: grid; grid-template-columns: 100px; width: 100px; } \
         a { height: 80px; }",
    );
    let body = &root;
    let div = first_element_child(body);
    // Container height should accommodate the 80px item.
    assert!(div.rect.height >= 80.0, "grid height should be ≥80px, got {}", div.rect.height);
}

// ── CSS Grid named areas ──────────────────────────────────────────────────

/// `parse_grid_template_areas` — 2×2 grid with named areas.
#[test]
fn grid_template_areas_parse_2x2() {
    use crate::parse_grid_template_areas;
    let areas = parse_grid_template_areas(r#""header header" "sidebar main""#);
    assert_eq!(areas.len(), 2, "should have 2 rows");
    assert_eq!(areas[0], vec!["header", "header"]);
    assert_eq!(areas[1], vec!["sidebar", "main"]);
}

/// `parse_grid_template_areas` — single row.
#[test]
fn grid_template_areas_parse_single_row() {
    use crate::parse_grid_template_areas;
    let areas = parse_grid_template_areas(r#""a b c""#);
    assert_eq!(areas, vec![vec!["a", "b", "c"]]);
}

/// `parse_grid_template_areas` — `none` returns empty.
#[test]
fn grid_template_areas_none() {
    use crate::parse_grid_template_areas;
    let areas = parse_grid_template_areas("none");
    assert!(areas.is_empty());
}

/// `parse_grid_template_areas` — dot (.) cells are stored as-is.
#[test]
fn grid_template_areas_dot_cells() {
    use crate::parse_grid_template_areas;
    let areas = parse_grid_template_areas(r#""a . b""#);
    assert_eq!(areas[0], vec!["a", ".", "b"]);
}

/// `GridLine::parse` recognises named area idents.
#[test]
fn grid_line_parse_named_ident() {
    use crate::GridLine;
    assert_eq!(GridLine::parse("main"), Some(GridLine::Named("main".into())));
    assert_eq!(GridLine::parse("header-area"), Some(GridLine::Named("header-area".into())));
    assert_eq!(GridLine::parse("auto"), Some(GridLine::Auto));
    assert_eq!(GridLine::parse("2"), Some(GridLine::Line(2)));
    // digit-only or empty → not an ident
    assert_eq!(GridLine::parse("3abc"), None);
}

/// `grid-area: <name>` shorthand sets all four placement properties to Named.
#[test]
fn grid_area_named_sets_all_four() {
    let root = lay(
        "<body><div></div></body>",
        "div { grid-area: main; }",
    );
    let body = &root;
    let div = first_element_child(body);
    assert_eq!(div.style.grid_row_start,    GridLine::Named("main".into()));
    assert_eq!(div.style.grid_row_end,      GridLine::Named("main".into()));
    assert_eq!(div.style.grid_column_start, GridLine::Named("main".into()));
    assert_eq!(div.style.grid_column_end,   GridLine::Named("main".into()));
}

/// `grid-template-areas` stored on container after cascade.
#[test]
fn grid_template_areas_stored_on_container() {
    let root = lay(
        "<body><div></div></body>",
        r#"div { display: grid; grid-template-areas: "header header" "sidebar main"; }"#,
    );
    let body = &root;
    let div = first_element_child(body);
    let areas = &div.style.grid_template_areas;
    assert_eq!(areas.len(), 2, "should have 2 rows");
    assert_eq!(areas[0], vec!["header", "header"]);
    assert_eq!(areas[1], vec!["sidebar", "main"]);
}

/// Named area layout: a 2×2 grid where items reference areas by name.
///
/// ```css
/// .grid {
///   display: grid;
///   grid-template-columns: 100px 100px;
///   grid-template-rows: 50px 50px;
///   grid-template-areas: "a b" "a c";
///   width: 200px;
/// }
/// .item-a { grid-area: a; }  /* row 1–3, col 1–2 */
/// .item-b { grid-area: b; }  /* row 1–2, col 2–3 */
/// .item-c { grid-area: c; }  /* row 2–3, col 2–3 */
/// ```
#[test]
fn grid_named_areas_layout_placement() {
    let root = lay(
        "<body><div><span id='a'></span><span id='b'></span><span id='c'></span></div></body>",
        r#"
        div {
            display: grid;
            grid-template-columns: 100px 100px;
            grid-template-rows: 50px 50px;
            grid-template-areas: "a b" "a c";
            width: 200px;
        }
        #a { grid-area: a; }
        #b { grid-area: b; }
        #c { grid-area: c; }
        "#,
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 3, "3 named-area items");
    let item_a = &items[0];
    let item_b = &items[1];
    let item_c = &items[2];
    // item-a occupies rows 1-2 (height=100) at column 1 (x=0, width=100)
    assert!((item_a.rect.x - 0.0).abs() < 1.0,  "a.x should be 0, got {}", item_a.rect.x);
    assert!((item_a.rect.width - 100.0).abs() < 1.0, "a.w should be 100, got {}", item_a.rect.width);
    assert!((item_a.rect.height - 100.0).abs() < 1.0, "a.h should be 100 (2 rows), got {}", item_a.rect.height);
    // item-b occupies row 1 at column 2 (x=100, width=100, height=50)
    assert!((item_b.rect.x - 100.0).abs() < 1.0, "b.x should be 100, got {}", item_b.rect.x);
    assert!((item_b.rect.y - 0.0).abs() < 1.0,   "b.y should be 0, got {}", item_b.rect.y);
    assert!((item_b.rect.width - 100.0).abs() < 1.0, "b.w should be 100, got {}", item_b.rect.width);
    assert!((item_b.rect.height - 50.0).abs() < 1.0, "b.h should be 50, got {}", item_b.rect.height);
    // item-c occupies row 2 at column 2 (y=50, width=100, height=50)
    assert!((item_c.rect.x - 100.0).abs() < 1.0, "c.x should be 100, got {}", item_c.rect.x);
    assert!((item_c.rect.y - 50.0).abs() < 1.0,  "c.y should be 50, got {}", item_c.rect.y);
    assert!((item_c.rect.width - 100.0).abs() < 1.0, "c.w should be 100, got {}", item_c.rect.width);
    assert!((item_c.rect.height - 50.0).abs() < 1.0, "c.h should be 50, got {}", item_c.rect.height);
}

/// Named area with a span > 1 row: area "sidebar" spans both rows.
#[test]
fn grid_named_area_spanning_rows() {
    let root = lay(
        "<body><div><span id='h'></span><span id='s'></span></div></body>",
        r#"
        div {
            display: grid;
            grid-template-columns: 200px 600px;
            grid-template-rows: 80px 80px;
            grid-template-areas: "header header" "sidebar content";
            width: 800px;
        }
        #h { grid-area: header; }
        #s { grid-area: sidebar; }
        "#,
    );
    let body = &root;
    let div = first_element_child(body);
    let items: Vec<_> = div
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    // header spans both columns: x=0, w=800, y=0, h=80
    let header = &items[0];
    assert!((header.rect.x - 0.0).abs() < 1.0,    "h.x={}", header.rect.x);
    assert!((header.rect.width - 800.0).abs() < 1.0, "h.w={}", header.rect.width);
    assert!((header.rect.y - 0.0).abs() < 1.0,    "h.y={}", header.rect.y);
    assert!((header.rect.height - 80.0).abs() < 1.0, "h.h={}", header.rect.height);
    // sidebar: x=0, w=200, y=80, h=80
    let sidebar = &items[1];
    assert!((sidebar.rect.x - 0.0).abs() < 1.0,   "s.x={}", sidebar.rect.x);
    assert!((sidebar.rect.width - 200.0).abs() < 1.0, "s.w={}", sidebar.rect.width);
    assert!((sidebar.rect.y - 80.0).abs() < 1.0,  "s.y={}", sidebar.rect.y);
}

// ── grid-auto-flow: dense ────────────────────────────────────────────────

/// Dense row packing fills the gap left by a wide item.
///
///  3 cols, A and B each span 2 cols; C and D are 1×1.
///
///  Sparse (row):             Dense (row dense):
///  +---+---+---+             +---+---+---+
///  | A   A |   |             | A   A | C |  ← C fills gap in row 1
///  +---+---+---+             +---+---+---+
///  | B   B | C |             | B   B | D |  ← D fills gap in row 2
///  +---+---+---+             +---+---+---+
///  | D |       |
///  +---+---+---+
#[test]
fn grid_dense_row_fills_gap() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span>\
           <span id='b'></span>\
           <span id='c'></span>\
           <span id='d'></span>\
         </div></body>",
        r#"
        #g {
            display: grid;
            grid-template-columns: 100px 100px 100px;
            grid-auto-rows: 50px;
            grid-auto-flow: row dense;
            width: 300px;
        }
        #a { grid-column: span 2; }
        #b { grid-column: span 2; }
        /* c, d: auto 1×1 */
        "#,
    );
    let body = &root;
    let grid = first_element_child(body);
    let items: Vec<_> = grid.children.iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 4, "expected 4 items");

    let a = &items[0];
    let b = &items[1];
    let c = &items[2];
    let d = &items[3];

    // A: cols 1-2, row 1 → x=0, w=200, y=0
    assert!((a.rect.x - 0.0).abs() < 1.0,     "a.x={}", a.rect.x);
    assert!((a.rect.width - 200.0).abs() < 1.0, "a.w={}", a.rect.width);
    assert!((a.rect.y - 0.0).abs() < 1.0,     "a.y={}", a.rect.y);

    // B: cols 1-2, row 2 → x=0, w=200, y=50
    assert!((b.rect.x - 0.0).abs() < 1.0,     "b.x={}", b.rect.x);
    assert!((b.rect.width - 200.0).abs() < 1.0, "b.w={}", b.rect.width);
    assert!((b.rect.y - 50.0).abs() < 1.0,    "b.y={}", b.rect.y);

    // Dense: C fills the gap at col 3, row 1 → x=200, y=0
    assert!((c.rect.x - 200.0).abs() < 1.0, "c.x={}: dense must fill row-1 gap", c.rect.x);
    assert!((c.rect.y - 0.0).abs() < 1.0,   "c.y={}: dense must fill row-1 gap", c.rect.y);

    // Dense: D fills the gap at col 3, row 2 → x=200, y=50
    assert!((d.rect.x - 200.0).abs() < 1.0, "d.x={}: dense must fill row-2 gap", d.rect.x);
    assert!((d.rect.y - 50.0).abs() < 1.0,  "d.y={}: dense must fill row-2 gap", d.rect.y);
}

/// Sparse layout must NOT back-fill: C stays in row 2 (after B), D in row 3.
///
///  Same grid: A(span2), B(span2), C(1×1), D(1×1) with `grid-auto-flow: row`.
///  Col-3 gap in row 1 is skipped by the forward-only cursor.
#[test]
fn grid_sparse_row_no_backfill() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span>\
           <span id='b'></span>\
           <span id='c'></span>\
           <span id='d'></span>\
         </div></body>",
        r#"
        #g {
            display: grid;
            grid-template-columns: 100px 100px 100px;
            grid-auto-rows: 50px;
            grid-auto-flow: row;
            width: 300px;
        }
        #a { grid-column: span 2; }
        #b { grid-column: span 2; }
        "#,
    );
    let body = &root;
    let grid = first_element_child(body);
    let items: Vec<_> = grid.children.iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 4, "expected 4 items");

    let c = &items[2];
    let d = &items[3];

    // Sparse: C ends up at col 3, row 2 (not row 1 — cursor didn't go back).
    assert!((c.rect.x - 200.0).abs() < 1.0, "c.x={}: sparse must not back-fill col3 row1", c.rect.x);
    assert!((c.rect.y - 50.0).abs() < 1.0,  "c.y={}: sparse must not back-fill col3 row1", c.rect.y);

    // D ends up at col 1, row 3 (cursor advanced past row 2).
    assert!((d.rect.y - 100.0).abs() < 1.0, "d.y={}: sparse must not back-fill", d.rect.y);
}

// ── BUG-801: auto-placement must terminate when an item needs a column
//    past the last explicit line ─────────────────────────────────────────
//
// Three shapes that used to hang `lay_out_grid`'s Pass-2 scan forever: an
// explicit column start beyond the explicit grid, a span wider than the
// explicit grid, and both combined. Each must finish and grow the implicit
// grid to fit the item (CSS Grid L1 §7.1) rather than refusing placement.
// A regression here reintroduces an infinite loop, not a wrong pixel — if
// this test never returns, that is the failure, not a panic.

/// `grid-column: 3` on a 2-column grid, row auto: the fixed column start
/// is beyond the explicit grid, so the scan must not search columns at
/// all for this item — only occupancy decides the row.
#[test]
fn grid_column_start_beyond_explicit_grid_terminates() {
    let root = lay(
        "<body><div id='g'><span id='a'></span><span id='b'></span></div></body>",
        "#g { display: grid; grid-template-columns: 100px 100px; column-gap: 4px; width: 400px; } \
         #a { grid-column: 3; height: 20px; } \
         #b { height: 20px; }",
    );
    let grid = first_element_child(&root);
    let items = grid_items(grid);
    assert_eq!(items.len(), 2);
    let a = &items[0];
    let b = &items[1];
    // A lands in the implicit 3rd column (past the 2 explicit + gap), row 1.
    assert!(a.rect.x > 200.0, "a.x={}: must be in the implicit 3rd column", a.rect.x);
    assert!((a.rect.y - 0.0).abs() < 1.0, "a.y={}", a.rect.y);
    // B auto-places on row 2, column 1 — placing A's fixed column also
    // advances the sparse cursor forward past row 1, same as it would for
    // any other placed item.
    assert!((b.rect.x - 0.0).abs() < 1.0, "b.x={}", b.rect.x);
    assert!(b.rect.y > a.rect.y, "b.y={} must be below a.y={}", b.rect.y, a.rect.y);
}

/// `grid-column: span 3` on a 2-column grid, both axes otherwise auto: the
/// item's own span exceeds the explicit column count, so the column bound
/// used by the scan must grow to the span or no starting column ever fits.
#[test]
fn grid_span_wider_than_explicit_grid_terminates() {
    let root = lay(
        "<body><div id='g'><span id='a'></span><span id='b'></span></div></body>",
        "#g { display: grid; grid-template-columns: 100px 100px; width: 400px; } \
         #a { grid-column: span 3; height: 20px; } \
         #b { height: 20px; }",
    );
    let grid = first_element_child(&root);
    let items = grid_items(grid);
    assert_eq!(items.len(), 2);
    let a = &items[0];
    let b = &items[1];
    // A must start at column 1 (the only column a span-3 item can ever fit)
    // and span the grown implicit grid.
    assert!((a.rect.x - 0.0).abs() < 1.0, "a.x={}", a.rect.x);
    assert!((a.rect.width - 400.0).abs() < 1.0, "a.w={}: spans the whole grown grid", a.rect.width);
    // B auto-places on the next row — row 1 is fully occupied by A's span.
    assert!(b.rect.y > a.rect.y, "b.y={} must be below a.y={}", b.rect.y, a.rect.y);
}

/// `grid-column: 9 / span 2` on a 2-column grid: both defects from the bug
/// report at once — explicit start past the grid, spanning further still.
#[test]
fn grid_explicit_start_and_span_beyond_grid_terminates() {
    let root = lay(
        "<body><div id='g'><span id='a'></span></div></body>",
        "#g { display: grid; grid-template-columns: 100px 100px; width: 400px; } \
         #a { grid-column: 9 / span 2; height: 20px; }",
    );
    let grid = first_element_child(&root);
    let items = grid_items(grid);
    assert_eq!(items.len(), 1);
    // Must land past the 2 explicit columns (200px), in implicit columns
    // 9-10 — those, and the 6 unused implicit columns before them, share
    // the leftover width, so the item does not reach the container edge.
    assert!(items[0].rect.x > 200.0, "x={}: must be past the explicit grid", items[0].rect.x);
}

/// Dense column-flow mirror of the row-flow fix: `grid-row: span 3` on a
/// 2-row grid must grow the implicit row count rather than hang.
#[test]
fn grid_row_span_wider_than_explicit_grid_terminates() {
    let root = lay(
        "<body><div id='g'><span id='a'></span></div></body>",
        "#g { display: grid; grid-template-rows: 50px 50px; grid-auto-flow: column; \
              grid-auto-columns: 100px; height: 100px; } \
         #a { grid-row: span 3; }",
    );
    let grid = first_element_child(&root);
    let items = grid_items(grid);
    assert_eq!(items.len(), 1);
    // Reaching this assertion at all is the point — the scan terminated.
    assert!((items[0].rect.y - 0.0).abs() < 1.0, "y={}", items[0].rect.y);
}

/// Dense column flow: small items back-fill gaps left by tall items in earlier columns.
///
///  2 cols, 3 explicit rows (50px).
///  A spans 2 rows (col 1, rows 1-2); B spans 3 rows (col 2, rows 1-3).
///  Dense: C fills the remaining slot in col 1, row 3.
///  Sparse: C would continue forward to col 3 (outside the explicit grid).
#[test]
fn grid_dense_column_fills_gap() {
    let root = lay(
        "<body><div id='g'>\
           <span id='a'></span>\
           <span id='b'></span>\
           <span id='c'></span>\
         </div></body>",
        r#"
        #g {
            display: grid;
            grid-template-columns: 100px 100px;
            grid-template-rows: 50px 50px 50px;
            grid-auto-flow: column dense;
            width: 200px;
        }
        #a { grid-row: span 2; }
        #b { grid-row: span 3; }
        /* c: auto 1×1 */
        "#,
    );
    let body = &root;
    let grid = first_element_child(body);
    let items: Vec<_> = grid.children.iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();
    assert_eq!(items.len(), 3, "expected 3 items");

    let a = &items[0];
    let b = &items[1];
    let c = &items[2];

    // A: col 1, rows 1-2 → x=0, y=0, h=100
    assert!((a.rect.x - 0.0).abs() < 1.0,      "a.x={}", a.rect.x);
    assert!((a.rect.y - 0.0).abs() < 1.0,      "a.y={}", a.rect.y);
    assert!((a.rect.height - 100.0).abs() < 1.0, "a.h={}", a.rect.height);

    // B: col 2, rows 1-3 → x=100, y=0, h=150
    assert!((b.rect.x - 100.0).abs() < 1.0,    "b.x={}", b.rect.x);
    assert!((b.rect.y - 0.0).abs() < 1.0,      "b.y={}", b.rect.y);
    assert!((b.rect.height - 150.0).abs() < 1.0, "b.h={}", b.rect.height);

    // Dense: C fills col 1 row 3 → x=0, y=100
    assert!((c.rect.x - 0.0).abs() < 1.0,   "c.x={}: dense col must back-fill col1 row3", c.rect.x);
    assert!((c.rect.y - 100.0).abs() < 1.0, "c.y={}: dense col must back-fill col1 row3", c.rect.y);
}

