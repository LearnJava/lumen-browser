//! P1/SPLIT-DL7: table painting — `enum BorderPrecedence` … до конца
//! `emit_table_cell_content`. Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-7).

use super::*;

/// Border precedence value для collapsed border model (CSS Tables L2 §17.6.2).
/// Более высокий precedence побеждает при конфликте.
/// Phase 1: поддержка precedence calculation, full integration в Phase 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub(crate) enum BorderPrecedence {
    /// Table border — самый низкий precedence
    Table,
    /// Row group border (thead/tbody/tfoot)
    RowGroup,
    /// Row border
    Row,
    /// Column group border (colgroup)
    ColumnGroup,
    /// Column border (col)
    Column,
    /// Cell border — самый высокий precedence
    Cell,
}

/// Информация о border для collapsed border model
/// Phase 1: структура и helpers для future collapse mode implementation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CollapsedBorder {
    /// Ширина границы
    pub(crate) width: f32,
    /// Цвет границы
    pub(crate) color: [f32; 4],
    /// Стиль границы (solid, dashed и т.д.)
    pub(crate) style: BorderStyle,
    /// Precedence для разрешения конфликтов
    pub(crate) precedence: BorderPrecedence,
}

impl CollapsedBorder {
    /// Выбирает наиболее приоритетную границу из двух конкурирующих
    /// Согласно CSS Tables L2 §17.6.2, более узкие границы скрываются,
    /// а при равной ширине побеждает hide > none > solid/dashed... > initial
    #[allow(dead_code)]
    pub(crate) fn resolve_conflict(a: &Self, b: &Self) -> Self {
        // По precedence: более высокий precedence побеждает
        if a.precedence != b.precedence {
            return if a.precedence > b.precedence {
                a.clone()
            } else {
                b.clone()
            };
        }

        // При равном precedence: более узкая граница скрывается
        if (a.width - b.width).abs() > 0.001 {
            return if a.width > b.width {
                a.clone()
            } else {
                b.clone()
            };
        }

        // По умолчанию выбираем первую (может быть улучшено по стилю)
        a.clone()
    }
}

/// Контекст таблицы — режим схлопывания границ и spacing, читаются из `ComputedStyle`.
/// Phase 0: layout использует spacing напрямую; Phase 2 будет передавать ctx в emit_table_cell.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct TableContext {
    /// `separate | collapse` — из `ComputedStyle.border_collapse`.
    pub(crate) border_collapse: BorderCollapse,
    /// Горизонтальный и вертикальный gap (px) между ячейками в `separate` режиме.
    pub(crate) border_spacing: (f32, f32),
}

impl TableContext {
    /// Строит контекст из стиля таблицы.
    fn from_box(b: &LayoutBox) -> Self {
        TableContext {
            border_collapse: b.style.border_collapse,
            border_spacing: (b.style.border_spacing_h, b.style.border_spacing_v),
        }
    }
}

/// Рендеринг таблицы с поддержкой border-collapse и фонов ячеек.
///
/// CSS 2.1 §17.5: separate (default) — ячейки рисуют свои границы;
/// collapse — соседние границы схлопываются (Phase 0: suppress double-draw).
pub(crate) fn emit_table_box(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    let _table_ctx = TableContext::from_box(b);

    // Эмитим фон таблицы
    if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        let clip = background_clip_rect(b, background_color_clip(b));
        if clip.width > 0.0 && clip.height > 0.0 {
            out.push(DisplayCommand::FillRect { rect: clip, color: bg });
        }
    }
    emit_background_image(out, b, dpr);

    // Обрабатываем граници таблицы
    let s = &b.style;
    let has_border = s.border_top_style.is_visible()
        || s.border_right_style.is_visible()
        || s.border_bottom_style.is_visible()
        || s.border_left_style.is_visible();
    if has_border {
        let cur = s.color;
        out.push(DisplayCommand::DrawBorder {
            rect: b.rect,
            widths: [
                s.border_top_width, s.border_right_width,
                s.border_bottom_width, s.border_left_width,
            ],
            colors: [
                s.border_top_color.resolve(cur),
                s.border_right_color.resolve(cur),
                s.border_bottom_color.resolve(cur),
                s.border_left_color.resolve(cur),
            ],
            styles: [
                s.border_top_style, s.border_right_style,
                s.border_bottom_style, s.border_left_style,
            ],
            radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
        });
    }

    // BUG-200: under `border-collapse: collapse` adjacent cells overlap by the shared
    // grid-line width (layout pulls them together). If each cell emits its own
    // background+border interleaved in DOM order, a later cell's background erases the
    // earlier neighbour's collapsed border on the shared edge — when the later cell's
    // own border is thinner (e.g. a 1px `thin` cell after a 3px `thick` one), the shared
    // edge collapses to 1px instead of the spec's max width (CSS 2.1 §17.6.2). Emit the
    // whole table in three passes (all backgrounds, then all borders, then all contents)
    // so no cell's fill can overwrite another cell's collapsed border; meeting borders of
    // the same colour then composite to the wider one.
    if matches!(b.style.border_collapse, BorderCollapse::Collapse) {
        let mut cells: Vec<&LayoutBox> = Vec::new();
        collect_table_cells(b, &mut cells);
        for cell in &cells {
            emit_table_cell_background(cell, out, dpr);
        }
        for cell in &cells {
            emit_table_cell_border(cell, out);
        }
        for cell in &cells {
            emit_table_cell_content(cell, out, dpr);
        }
        return;
    }

    // Обрабатываем строки и ячейки (separate model)
    for row_group in &b.children {
        match &row_group.kind {
            BoxKind::TableRowGroup => {
                emit_table_row_group(row_group, out, dpr);
            }
            BoxKind::TableRow => {
                emit_table_row(row_group, out, dpr);
            }
            _ => {
                walk(row_group, out, dpr, None);
            }
        }
    }
}

/// Collect every table cell box (`display: table-cell`) under a table, flattening
/// row groups and rows into DOM order. Used by the collapse-mode three-pass emitter.
pub(crate) fn collect_table_cells<'a>(b: &'a LayoutBox, out: &mut Vec<&'a LayoutBox>) {
    for child in &b.children {
        match &child.kind {
            BoxKind::TableRowGroup => collect_table_cells(child, out),
            BoxKind::TableRow => {
                for cell in &child.children {
                    if !matches!(cell.kind, BoxKind::Skip) {
                        out.push(cell);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Эмитируем группу строк таблицы (thead, tbody, tfoot)
fn emit_table_row_group(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    // Группа не рендерится сама по себе (прозрачный контейнер)
    // но может иметь фон и граници

    // TODO: для Phase 1 можно добавить фон group-уровня

    // Обрабатываем строки
    for row in &b.children {
        if matches!(&row.kind, BoxKind::TableRow) {
            emit_table_row(row, out, dpr);
        }
    }
}

/// Эмитируем строку таблицы
fn emit_table_row(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    // Обрабатываем ячейки строки
    for cell in &b.children {
        emit_table_cell(cell, out, dpr);
    }
}

/// Эмитируем ячейку таблицы.
///
/// В `separate` режиме каждая ячейка рисует все 4 границы.
/// В `collapse` режиме layout уже зануляет border-spacing; каждая ячейка
/// рисует только top+left границы, чтобы избежать двойного рисования
/// по общим рёбрам (Phase 0 упрощение; полный алгоритм §17.6.2 — Phase 2).
fn emit_table_cell(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    emit_table_cell_background(b, out, dpr);
    emit_table_cell_border(b, out);
    emit_table_cell_content(b, out, dpr);
}

/// Emit a table cell's background colour + background image.
///
/// CSS Tables L2 §17.6.1.1 — `empty-cells: hide`: a cell with no in-flow content
/// draws neither background nor borders (separated-borders model only).
fn emit_table_cell_background(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    if is_hidden_empty_cell(b) {
        return;
    }
    if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        out.push(DisplayCommand::FillRect { rect: b.rect, color: bg });
    }
    emit_background_image(out, b, dpr);
}

/// Emit a table cell's borders. In collapse mode neighbouring cells overlap by the
/// shared grid-line width; backgrounds are emitted in a separate earlier pass so this
/// border survives a thinner neighbour's fill (BUG-200).
pub(crate) fn emit_table_cell_border(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    if is_hidden_empty_cell(b) {
        return;
    }
    let s = &b.style;
    let has_border = s.border_top_style.is_visible()
        || s.border_right_style.is_visible()
        || s.border_bottom_style.is_visible()
        || s.border_left_style.is_visible();
    if !has_border {
        return;
    }
    let cur = s.color;
    out.push(DisplayCommand::DrawBorder {
        rect: b.rect,
        widths: [
            s.border_top_width, s.border_right_width,
            s.border_bottom_width, s.border_left_width,
        ],
        colors: [
            s.border_top_color.resolve(cur),
            s.border_right_color.resolve(cur),
            s.border_bottom_color.resolve(cur),
            s.border_left_color.resolve(cur),
        ],
        styles: [
            s.border_top_style, s.border_right_style,
            s.border_bottom_style, s.border_left_style,
        ],
        radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
    });
}

/// Emit a table cell's content (text, nested blocks, …).
fn emit_table_cell_content(b: &LayoutBox, out: &mut Vec<DisplayCommand>, dpr: f32) {
    for child in &b.children {
        walk(child, out, dpr, None);
    }
}
