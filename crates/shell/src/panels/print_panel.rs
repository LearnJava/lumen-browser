//! Print dialog panel (E-1 + W-2b).
//!
//! A centred 560×420 px modal opened by `Ctrl+P`. Exposes print settings:
//!
//! - **Paper size** — A4 / Letter / Legal pill-buttons.
//! - **Orientation** — Portrait / Landscape pill-buttons.
//! - **Margins** — Normal / Narrow / Wide pill-buttons.
//! - **Scale** — document zoom 50–200% (W-2b new field).
//! - **Page range** — editable text field (`"all"` by default).
//! - **Color mode** — Color / Grayscale pill-buttons.
//! - **Output file** — editable text field (default `"output.pdf"`).
//! - **Print** / **Cancel** buttons at the bottom.
//!
//! Hit testing: [`hit_test`]. Rendering: [`build_panel`].

use lumen_core::geom::Rect;
use lumen_layout::{BorderStyle, Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};

// ── Geometry ─────────────────────────────────────────────────────────────────

/// Panel width in CSS px.
pub const PANEL_W: f32 = 560.0;
/// Panel height in CSS px (expanded for W-2b scale field + CC-8 background row).
pub const PANEL_H: f32 = 462.0;
/// Header bar height.
const HEADER_H: f32 = 36.0;
/// Content row height.
const ROW_H: f32 = 42.0;
/// Horizontal padding.
const PAD_H: f32 = 16.0;
/// Vertical padding before first row.
const PAD_V: f32 = 6.0;
/// Label column width.
const LABEL_W: f32 = 96.0;
/// Close `×` hit zone width.
const CLOSE_W: f32 = 28.0;
/// Font size for labels and pills.
const FONT_SIZE: f32 = 12.0;
/// Header title font size.
const HEADER_FONT: f32 = 13.0;
/// Pill button height.
const PILL_H: f32 = 26.0;
/// Button height (Print/Cancel).
const BTN_H: f32 = 32.0;
/// Button width.
const BTN_W: f32 = 100.0;
/// Text field height.
const FIELD_H: f32 = 26.0;

// ── Colours ───────────────────────────────────────────────────────────────────

const PANEL_BG: Color = Color { r: 18, g: 18, b: 26, a: 254 };
const PANEL_BORDER: Color = Color { r: 52, g: 52, b: 66, a: 255 };
const HEADER_BG: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const HEADER_TEXT: Color = Color { r: 210, g: 210, b: 225, a: 255 };
const CLOSE_COL: Color = Color { r: 180, g: 80, b: 80, a: 255 };
const LABEL_COL: Color = Color { r: 190, g: 190, b: 210, a: 255 };
const PILL_BG: Color = Color { r: 36, g: 36, b: 52, a: 230 };
const PILL_ACTIVE: Color = Color { r: 58, g: 90, b: 160, a: 230 };
const PILL_TEXT: Color = Color { r: 190, g: 190, b: 210, a: 255 };
const ROW_EVEN: Color = Color { r: 22, g: 22, b: 32, a: 255 };
const ROW_ODD: Color = Color { r: 26, g: 26, b: 36, a: 255 };
const SEPARATOR: Color = Color { r: 36, g: 36, b: 50, a: 255 };
const FIELD_BG: Color = Color { r: 30, g: 30, b: 44, a: 255 };
const FIELD_BORDER: Color = Color { r: 60, g: 60, b: 80, a: 255 };
const FIELD_FOCUS: Color = Color { r: 70, g: 100, b: 180, a: 255 };
const FIELD_TEXT: Color = Color { r: 220, g: 220, b: 240, a: 255 };
const BTN_PRINT: Color = Color { r: 55, g: 90, b: 160, a: 255 };
const BTN_CANCEL: Color = Color { r: 40, g: 40, b: 58, a: 255 };
const BTN_TEXT: Color = Color { r: 220, g: 220, b: 240, a: 255 };

// ── Domain types ──────────────────────────────────────────────────────────────

/// Paper size for the print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperSize {
    /// ISO A4 (210 × 297 mm).
    A4,
    /// US Letter (8.5 × 11 in).
    Letter,
    /// US Legal (8.5 × 14 in).
    Legal,
}

/// Page orientation for the print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Taller than wide.
    Portrait,
    /// Wider than tall.
    Landscape,
}

/// Margin preset for the print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginPreset {
    /// Standard ~19 mm margins.
    Normal,
    /// Small ~6 mm margins.
    Narrow,
    /// Large ~25 mm margins.
    Wide,
}

/// Output colour mode for the print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// Full-colour output.
    Color,
    /// Greyscale output.
    Grayscale,
}

/// Which editable text field currently has keyboard focus in the print panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintField {
    /// The page-range text field (e.g. `"all"` or `"1-3,5"`).
    PageRange,
    /// The output-file path text field (e.g. `"output.pdf"`).
    OutputPath,
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Print dialog panel state.
///
/// `visible` gates rendering and hit-testing. The fields are working
/// copies of the print settings; they are read in the shell handler
/// when the user clicks **Print** to kick off `do_print_to_pdf()`.
pub struct PrintPanel {
    /// Whether the panel is currently visible.
    pub visible: bool,
    /// Selected paper size.
    pub paper: PaperSize,
    /// Selected page orientation.
    pub orientation: Orientation,
    /// Selected margin preset.
    pub margins: MarginPreset,
    /// Document zoom level in percent (50–200%, W-2b new field).
    pub scale: i32,
    /// Page range string: `"all"` or an explicit range such as `"1-3,5"`.
    pub page_range: String,
    /// Output colour mode.
    pub color_mode: ColorMode,
    /// Whether CSS background graphics are printed (CC-8). When `false`, the
    /// print pipeline strips background fills / images / gradients before
    /// rasterising each page.
    pub print_backgrounds: bool,
    /// Destination file path (relative or absolute).
    pub output_path: String,
    /// Which text field is currently focused, if any.
    pub editing_field: Option<PrintField>,
}

impl PrintPanel {
    /// Create a new hidden panel with default print settings.
    pub fn new() -> Self {
        Self {
            visible: false,
            paper: PaperSize::A4,
            orientation: Orientation::Portrait,
            margins: MarginPreset::Normal,
            scale: 100,
            page_range: "all".to_owned(),
            color_mode: ColorMode::Color,
            print_backgrounds: true,
            output_path: "output.pdf".to_owned(),
            editing_field: None,
        }
    }

    /// Toggle panel visibility; clears the active editing field on hide.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.editing_field = None;
        }
    }

    /// Hide the panel and clear the editing field.
    pub fn close(&mut self) {
        self.visible = false;
        self.editing_field = None;
    }

    /// Append a character to the currently focused text field.
    pub fn push_char(&mut self, ch: char) {
        match self.editing_field {
            Some(PrintField::PageRange) => self.page_range.push(ch),
            Some(PrintField::OutputPath) => self.output_path.push(ch),
            None => {}
        }
    }

    /// Delete the last character from the currently focused text field.
    pub fn pop_char(&mut self) {
        match self.editing_field {
            Some(PrintField::PageRange) => { self.page_range.pop(); }
            Some(PrintField::OutputPath) => { self.output_path.pop(); }
            None => {}
        }
    }

    /// Resolve margin values (top/bottom, left/right) in CSS px at 96 DPI.
    ///
    /// Used by the shell to build [`lumen_layout::PaginationContext`].
    pub fn margin_px(&self) -> (f32, f32) {
        match self.margins {
            MarginPreset::Normal => (48.0, 48.0),
            MarginPreset::Narrow => (18.0, 18.0),
            MarginPreset::Wide => (72.0, 72.0),
        }
    }
}

impl Default for PrintPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit testing ───────────────────────────────────────────────────────────────

/// Result of a click on (or near) the print panel.
#[derive(Debug, Clone, PartialEq)]
pub enum PrintHit {
    /// The close `×` button — hide without printing.
    Close,
    /// A paper-size pill was clicked.
    PaperSize(PaperSize),
    /// An orientation pill was clicked.
    Orientation(Orientation),
    /// A margin preset pill was clicked.
    Margins(MarginPreset),
    /// The scale decrease (−) button was clicked (W-2b).
    ScaleDecrease,
    /// The scale increase (+) button was clicked (W-2b).
    ScaleIncrease,
    /// The page-range text field was clicked.
    PageRangeField,
    /// A colour-mode pill was clicked.
    ColorMode(ColorMode),
    /// A background-graphics toggle pill was clicked (CC-8); payload = new value.
    Backgrounds(bool),
    /// The output-path text field was clicked.
    OutputPathField,
    /// The **Print** button was clicked.
    Print,
    /// The **Cancel** button was clicked.
    Cancel,
    /// Click inside the panel on a non-interactive area — swallow.
    Inside,
    /// Click outside the panel — pass through.
    Outside,
}

/// Top-left corner of the centred panel.
fn panel_origin(win_w: f32, win_h: f32) -> (f32, f32) {
    (
        ((win_w - PANEL_W) * 0.5).max(0.0),
        ((win_h - PANEL_H) * 0.5).max(0.0),
    )
}

/// Row y positions (relative to panel origin).
fn row_y(idx: usize) -> f32 {
    HEADER_H + PAD_V + idx as f32 * ROW_H
}

/// Classify a click at `(x, y)` CSS px.
pub fn hit_test(panel: &PrintPanel, x: f32, y: f32, win_w: f32, win_h: f32) -> PrintHit {
    if !panel.visible {
        return PrintHit::Outside;
    }
    let (px, py) = panel_origin(win_w, win_h);
    if x < px || x > px + PANEL_W || y < py || y > py + PANEL_H {
        return PrintHit::Outside;
    }

    // Close button: top-right of header.
    let ly = y - py;
    let lx = x - px;
    if ly < HEADER_H && lx > PANEL_W - CLOSE_W {
        return PrintHit::Close;
    }

    // Buttons row: Print / Cancel near bottom.
    let btn_y = PANEL_H - BTN_H - PAD_V * 2.0;
    if ly >= btn_y && ly < btn_y + BTN_H {
        let cancel_x = PANEL_W - PAD_H - BTN_W;
        let print_x = cancel_x - BTN_W - 8.0;
        if lx >= print_x && lx < print_x + BTN_W {
            return PrintHit::Print;
        }
        if lx >= cancel_x && lx < cancel_x + BTN_W {
            return PrintHit::Cancel;
        }
    }

    // Content rows.
    let avail_w = PANEL_W - PAD_H * 2.0 - LABEL_W;

    // Row 0: Paper size.
    let r0 = row_y(0);
    if ly >= r0 && ly < r0 + ROW_H {
        const PAPERS: [PaperSize; 3] = [PaperSize::A4, PaperSize::Letter, PaperSize::Legal];
        let pill_w = avail_w / 3.0;
        let pills_x = PAD_H + LABEL_W;
        for (i, size) in PAPERS.iter().enumerate() {
            let bx = pills_x + i as f32 * pill_w;
            if lx >= bx && lx < bx + pill_w - 2.0 {
                return PrintHit::PaperSize(*size);
            }
        }
        return PrintHit::Inside;
    }

    // Row 1: Orientation.
    let r1 = row_y(1);
    if ly >= r1 && ly < r1 + ROW_H {
        const ORIENTS: [Orientation; 2] = [Orientation::Portrait, Orientation::Landscape];
        let pill_w = avail_w / 2.0;
        let pills_x = PAD_H + LABEL_W;
        for (i, orient) in ORIENTS.iter().enumerate() {
            let bx = pills_x + i as f32 * pill_w;
            if lx >= bx && lx < bx + pill_w - 2.0 {
                return PrintHit::Orientation(*orient);
            }
        }
        return PrintHit::Inside;
    }

    // Row 2: Margins.
    let r2 = row_y(2);
    if ly >= r2 && ly < r2 + ROW_H {
        const MARGINS: [MarginPreset; 3] = [MarginPreset::Normal, MarginPreset::Narrow, MarginPreset::Wide];
        let pill_w = avail_w / 3.0;
        let pills_x = PAD_H + LABEL_W;
        for (i, preset) in MARGINS.iter().enumerate() {
            let bx = pills_x + i as f32 * pill_w;
            if lx >= bx && lx < bx + pill_w - 2.0 {
                return PrintHit::Margins(*preset);
            }
        }
        return PrintHit::Inside;
    }

    // Row 3: Scale +/- buttons (W-2b).
    let r3 = row_y(3);
    if ly >= r3 && ly < r3 + ROW_H {
        let field_x = PAD_H + LABEL_W;
        let btn_w = 36.0;
        let btn_gap = 4.0;
        let decrease_x = field_x;
        let increase_x = field_x + btn_w + btn_gap;
        if lx >= decrease_x && lx < decrease_x + btn_w {
            return PrintHit::ScaleDecrease;
        }
        if lx >= increase_x && lx < increase_x + btn_w {
            return PrintHit::ScaleIncrease;
        }
        return PrintHit::Inside;
    }

    // Row 4: Page range text field (was row 3).
    let r4 = row_y(4);
    if ly >= r4 && ly < r4 + ROW_H {
        let field_x = PAD_H + LABEL_W;
        if lx >= field_x {
            return PrintHit::PageRangeField;
        }
        return PrintHit::Inside;
    }

    // Row 5: Color mode (was row 4).
    let r5 = row_y(5);
    if ly >= r5 && ly < r5 + ROW_H {
        const MODES: [ColorMode; 2] = [ColorMode::Color, ColorMode::Grayscale];
        let pill_w = avail_w / 2.0;
        let pills_x = PAD_H + LABEL_W;
        for (i, mode) in MODES.iter().enumerate() {
            let bx = pills_x + i as f32 * pill_w;
            if lx >= bx && lx < bx + pill_w - 2.0 {
                return PrintHit::ColorMode(*mode);
            }
        }
        return PrintHit::Inside;
    }

    // Row 6: Output path text field (was row 5).
    let r6 = row_y(6);
    if ly >= r6 && ly < r6 + ROW_H {
        let field_x = PAD_H + LABEL_W;
        if lx >= field_x {
            return PrintHit::OutputPathField;
        }
        return PrintHit::Inside;
    }

    // Row 7: Background graphics toggle (CC-8).
    let r7 = row_y(7);
    if ly >= r7 && ly < r7 + ROW_H {
        const VALUES: [bool; 2] = [true, false];
        let pill_w = avail_w / 2.0;
        let pills_x = PAD_H + LABEL_W;
        for (i, on) in VALUES.iter().enumerate() {
            let bx = pills_x + i as f32 * pill_w;
            if lx >= bx && lx < bx + pill_w - 2.0 {
                return PrintHit::Backgrounds(*on);
            }
        }
        return PrintHit::Inside;
    }

    PrintHit::Inside
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the centred print dialog overlay.
///
/// Returns an empty `DisplayList` when `panel.visible` is `false`.
/// `px`/`py` are the top-left coordinates of the panel in CSS px.
pub fn build_panel(panel: &PrintPanel, px: f32, py: f32) -> DisplayList {
    if !panel.visible {
        return Vec::new();
    }

    // Panel background + border.
    let mut out: DisplayList = vec![DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        color: PANEL_BG,
        radii: uniform_radii(6.0),
    }];
    out.push(DisplayCommand::DrawBorder {
        rect: Rect::new(px, py, PANEL_W, PANEL_H),
        widths: [1.0; 4],
        colors: [PANEL_BORDER; 4],
        styles: [BorderStyle::Solid; 4],
        radii: uniform_radii(6.0),
    });

    // Header.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(px, py, PANEL_W, HEADER_H),
        color: HEADER_BG,
        radii: CornerRadii {
            tl: 6.0, tl_y: 6.0, tr: 6.0, tr_y: 6.0,
            br: 0.0, br_y: 0.0, bl: 0.0, bl_y: 0.0,
        },
    });
    out.push(make_text(
        "Печать".to_owned(),
        px + PAD_H,
        py + (HEADER_H - HEADER_FONT) / 2.0,
        PANEL_W - PAD_H * 2.0,
        HEADER_FONT,
        FontWeight::BOLD,
        HEADER_TEXT,
    ));
    out.push(make_text(
        "×".to_owned(),
        px + PANEL_W - CLOSE_W + 4.0,
        py + (HEADER_H - HEADER_FONT) / 2.0 - 1.0,
        CLOSE_W,
        HEADER_FONT + 3.0,
        FontWeight::NORMAL,
        CLOSE_COL,
    ));

    // Content rows.
    emit_paper_row(&mut out, panel, px, py);
    emit_orientation_row(&mut out, panel, px, py);
    emit_margins_row(&mut out, panel, px, py);
    emit_scale_row(&mut out, panel, px, py);
    emit_page_range_row(&mut out, panel, px, py);
    emit_color_row(&mut out, panel, px, py);
    emit_output_path_row(&mut out, panel, px, py);
    emit_backgrounds_row(&mut out, panel, px, py);

    // Separator before buttons.
    let sep_y = row_y(8);
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px + PAD_H, py + sep_y, PANEL_W - PAD_H * 2.0, 1.0),
        color: SEPARATOR,
    });

    // Buttons: Print / Cancel.
    emit_buttons(&mut out, px, py);

    out
}

// ── Row emitters ──────────────────────────────────────────────────────────────

fn emit_paper_row(out: &mut DisplayList, panel: &PrintPanel, px: f32, py: f32) {
    let ry = py + row_y(0);
    emit_row_bg(out, px, ry, 0);
    emit_label(out, "Бумага", px, ry);
    const PAPERS: [(PaperSize, &str); 3] = [
        (PaperSize::A4, "A4"),
        (PaperSize::Letter, "Letter"),
        (PaperSize::Legal, "Legal"),
    ];
    emit_pills_3(out, &PAPERS, panel.paper, px, ry, |a, b| a == b);
}

fn emit_orientation_row(out: &mut DisplayList, panel: &PrintPanel, px: f32, py: f32) {
    let ry = py + row_y(1);
    emit_row_bg(out, px, ry, 1);
    emit_label(out, "Ориентация", px, ry);
    const ORIENTS: [(Orientation, &str); 2] = [
        (Orientation::Portrait, "Портрет"),
        (Orientation::Landscape, "Альбом"),
    ];
    emit_pills_2(out, &ORIENTS, panel.orientation, px, ry, |a, b| a == b);
}

fn emit_margins_row(out: &mut DisplayList, panel: &PrintPanel, px: f32, py: f32) {
    let ry = py + row_y(2);
    emit_row_bg(out, px, ry, 0);
    emit_label(out, "Поля", px, ry);
    const MARGINS: [(MarginPreset, &str); 3] = [
        (MarginPreset::Normal, "Обычные"),
        (MarginPreset::Narrow, "Узкие"),
        (MarginPreset::Wide, "Широкие"),
    ];
    emit_pills_3(out, &MARGINS, panel.margins, px, ry, |a, b| a == b);
}

fn emit_scale_row(out: &mut DisplayList, panel: &PrintPanel, px: f32, py: f32) {
    let ry = py + row_y(3);
    emit_row_bg(out, px, ry, 1);
    emit_label(out, "Масштаб", px, ry);
    let field_x = px + PAD_H + LABEL_W;
    let field_y = ry + (ROW_H - FIELD_H) / 2.0;
    let btn_w = 36.0;
    let btn_gap = 4.0;

    // − button
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(field_x, field_y, btn_w, FIELD_H),
        color: FIELD_BG,
        radii: uniform_radii(3.0),
    });
    out.push(make_text(
        "−".to_owned(),
        field_x + 2.0,
        field_y + (FIELD_H - FONT_SIZE) / 2.0 - 2.0,
        btn_w - 4.0,
        FONT_SIZE + 2.0,
        FontWeight::NORMAL,
        FIELD_TEXT,
    ));

    // + button
    let plus_x = field_x + btn_w + btn_gap;
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(plus_x, field_y, btn_w, FIELD_H),
        color: FIELD_BG,
        radii: uniform_radii(3.0),
    });
    out.push(make_text(
        "+".to_owned(),
        plus_x + 2.0,
        field_y + (FIELD_H - FONT_SIZE) / 2.0 - 2.0,
        btn_w - 4.0,
        FONT_SIZE + 2.0,
        FontWeight::NORMAL,
        FIELD_TEXT,
    ));

    // Scale value display
    let display_x = plus_x + btn_w + btn_gap;
    let display_w = (PANEL_W - PAD_H * 2.0 - LABEL_W) - (btn_w * 2.0 + btn_gap * 2.0);
    out.push(make_text(
        format!("{}%", panel.scale),
        display_x + 4.0,
        field_y + (FIELD_H - FONT_SIZE) / 2.0,
        display_w,
        FONT_SIZE,
        FontWeight::NORMAL,
        FIELD_TEXT,
    ));
}

fn emit_page_range_row(out: &mut DisplayList, panel: &PrintPanel, px: f32, py: f32) {
    let ry = py + row_y(4);
    emit_row_bg(out, px, ry, 0);
    emit_label(out, "Страницы", px, ry);
    let field_x = px + PAD_H + LABEL_W;
    let field_w = PANEL_W - PAD_H * 2.0 - LABEL_W;
    let field_y = ry + (ROW_H - FIELD_H) / 2.0;
    let focused = panel.editing_field == Some(PrintField::PageRange);
    emit_text_field(out, &panel.page_range, field_x, field_y, field_w, focused);
}

fn emit_color_row(out: &mut DisplayList, panel: &PrintPanel, px: f32, py: f32) {
    let ry = py + row_y(5);
    emit_row_bg(out, px, ry, 1);
    emit_label(out, "Цвет", px, ry);
    const MODES: [(ColorMode, &str); 2] = [
        (ColorMode::Color, "Цветной"),
        (ColorMode::Grayscale, "Серый"),
    ];
    emit_pills_2(out, &MODES, panel.color_mode, px, ry, |a, b| a == b);
}

fn emit_output_path_row(out: &mut DisplayList, panel: &PrintPanel, px: f32, py: f32) {
    let ry = py + row_y(6);
    emit_row_bg(out, px, ry, 0);
    emit_label(out, "Файл", px, ry);
    let field_x = px + PAD_H + LABEL_W;
    let field_w = PANEL_W - PAD_H * 2.0 - LABEL_W;
    let field_y = ry + (ROW_H - FIELD_H) / 2.0;
    let focused = panel.editing_field == Some(PrintField::OutputPath);
    emit_text_field(out, &panel.output_path, field_x, field_y, field_w, focused);
}

fn emit_backgrounds_row(out: &mut DisplayList, panel: &PrintPanel, px: f32, py: f32) {
    let ry = py + row_y(7);
    emit_row_bg(out, px, ry, 1);
    emit_label(out, "Фон", px, ry);
    const VALUES: [(bool, &str); 2] = [(true, "Вкл"), (false, "Выкл")];
    emit_pills_2(out, &VALUES, panel.print_backgrounds, px, ry, |a, b| a == b);
}

fn emit_buttons(out: &mut DisplayList, px: f32, py: f32) {
    let btn_y = py + PANEL_H - BTN_H - PAD_V * 2.0;
    let cancel_x = px + PANEL_W - PAD_H - BTN_W;
    let print_x = cancel_x - BTN_W - 8.0;

    // Cancel button.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(cancel_x, btn_y, BTN_W, BTN_H),
        color: BTN_CANCEL,
        radii: uniform_radii(4.0),
    });
    out.push(make_text(
        "Отмена".to_owned(),
        cancel_x + 4.0,
        btn_y + (BTN_H - FONT_SIZE) / 2.0,
        BTN_W - 8.0,
        FONT_SIZE,
        FontWeight::NORMAL,
        BTN_TEXT,
    ));

    // Print button.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(print_x, btn_y, BTN_W, BTN_H),
        color: BTN_PRINT,
        radii: uniform_radii(4.0),
    });
    out.push(make_text(
        "Печать".to_owned(),
        print_x + 4.0,
        btn_y + (BTN_H - FONT_SIZE) / 2.0,
        BTN_W - 8.0,
        FONT_SIZE,
        FontWeight::BOLD,
        BTN_TEXT,
    ));
}

// ── Generic pill emitters ─────────────────────────────────────────────────────

fn emit_pills_3<T: Copy>(
    out: &mut DisplayList,
    items: &[(T, &str); 3],
    current: T,
    px: f32,
    row_y_abs: f32,
    eq: impl Fn(T, T) -> bool,
) {
    let avail_w = PANEL_W - PAD_H * 2.0 - LABEL_W;
    let pill_w = avail_w / 3.0;
    let pills_x = px + PAD_H + LABEL_W;
    let pill_y = row_y_abs + (ROW_H - PILL_H) / 2.0;
    for (i, (val, label)) in items.iter().enumerate() {
        let bx = pills_x + i as f32 * pill_w;
        let active = eq(*val, current);
        emit_pill(out, bx, pill_y, pill_w - 3.0, label, active);
    }
}

fn emit_pills_2<T: Copy>(
    out: &mut DisplayList,
    items: &[(T, &str); 2],
    current: T,
    px: f32,
    row_y_abs: f32,
    eq: impl Fn(T, T) -> bool,
) {
    let avail_w = PANEL_W - PAD_H * 2.0 - LABEL_W;
    let pill_w = avail_w / 2.0;
    let pills_x = px + PAD_H + LABEL_W;
    let pill_y = row_y_abs + (ROW_H - PILL_H) / 2.0;
    for (i, (val, label)) in items.iter().enumerate() {
        let bx = pills_x + i as f32 * pill_w;
        let active = eq(*val, current);
        emit_pill(out, bx, pill_y, pill_w - 3.0, label, active);
    }
}

fn emit_pill(out: &mut DisplayList, x: f32, y: f32, w: f32, label: &str, active: bool) {
    let bg = if active { PILL_ACTIVE } else { PILL_BG };
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(x, y, w, PILL_H),
        color: bg,
        radii: uniform_radii(4.0),
    });
    out.push(make_text(
        label.to_owned(),
        x + 4.0,
        y + (PILL_H - FONT_SIZE) / 2.0,
        w - 8.0,
        FONT_SIZE,
        if active { FontWeight::BOLD } else { FontWeight::NORMAL },
        PILL_TEXT,
    ));
}

fn emit_text_field(out: &mut DisplayList, value: &str, x: f32, y: f32, w: f32, focused: bool) {
    let border_col = if focused { FIELD_FOCUS } else { FIELD_BORDER };
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(x, y, w, FIELD_H),
        color: FIELD_BG,
        radii: uniform_radii(3.0),
    });
    out.push(DisplayCommand::DrawBorder {
        rect: Rect::new(x, y, w, FIELD_H),
        widths: [1.0; 4],
        colors: [border_col; 4],
        styles: [BorderStyle::Solid; 4],
        radii: uniform_radii(3.0),
    });
    out.push(make_text(
        value.to_owned(),
        x + 6.0,
        y + (FIELD_H - FONT_SIZE) / 2.0,
        w - 12.0,
        FONT_SIZE,
        FontWeight::NORMAL,
        FIELD_TEXT,
    ));
}

fn emit_label(out: &mut DisplayList, text: &str, px: f32, row_y_abs: f32) {
    out.push(make_text(
        text.to_owned(),
        px + PAD_H,
        row_y_abs + (ROW_H - FONT_SIZE) / 2.0,
        LABEL_W,
        FONT_SIZE,
        FontWeight::NORMAL,
        LABEL_COL,
    ));
}

fn emit_row_bg(out: &mut DisplayList, px: f32, row_y_abs: f32, parity: u8) {
    let color = if parity == 0 { ROW_EVEN } else { ROW_ODD };
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(px, row_y_abs, PANEL_W, ROW_H),
        color,
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn uniform_radii(r: f32) -> CornerRadii {
    CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r }
}

#[allow(clippy::too_many_arguments)]
fn make_text(
    text: String,
    x: f32,
    y: f32,
    w: f32,
    font_size: f32,
    weight: FontWeight,
    color: Color,
) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text,
        font_size,
        color,
        font_family: Vec::new(),
        font_weight: weight,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_panel() -> PrintPanel {
        PrintPanel::new()
    }

    // ── State ─────────────────────────────────────────────────────────────────

    #[test]
    fn new_panel_hidden_with_defaults() {
        let p = make_panel();
        assert!(!p.visible);
        assert_eq!(p.paper, PaperSize::A4);
        assert_eq!(p.orientation, Orientation::Portrait);
        assert_eq!(p.margins, MarginPreset::Normal);
        assert_eq!(p.scale, 100);
        assert_eq!(p.page_range, "all");
        assert_eq!(p.color_mode, ColorMode::Color);
        assert_eq!(p.output_path, "output.pdf");
        assert_eq!(p.editing_field, None);
    }

    #[test]
    fn toggle_shows_and_hides() {
        let mut p = make_panel();
        p.toggle();
        assert!(p.visible);
        p.toggle();
        assert!(!p.visible);
    }

    #[test]
    fn toggle_clears_editing_field_on_hide() {
        let mut p = make_panel();
        p.visible = true;
        p.editing_field = Some(PrintField::PageRange);
        p.toggle();
        assert!(!p.visible);
        assert_eq!(p.editing_field, None);
    }

    #[test]
    fn push_pop_char_in_page_range() {
        let mut p = make_panel();
        p.visible = true;
        p.editing_field = Some(PrintField::PageRange);
        p.page_range.clear();
        p.push_char('1');
        p.push_char('-');
        p.push_char('3');
        assert_eq!(p.page_range, "1-3");
        p.pop_char();
        assert_eq!(p.page_range, "1-");
    }

    #[test]
    fn push_pop_char_in_output_path() {
        let mut p = make_panel();
        p.editing_field = Some(PrintField::OutputPath);
        p.output_path = "doc".to_owned();
        p.push_char('.');
        p.push_char('p');
        assert_eq!(p.output_path, "doc.p");
        p.pop_char();
        assert_eq!(p.output_path, "doc.");
    }

    #[test]
    fn margin_px_values() {
        let mut p = make_panel();
        assert_eq!(p.margin_px(), (48.0, 48.0));
        p.margins = MarginPreset::Narrow;
        assert_eq!(p.margin_px(), (18.0, 18.0));
        p.margins = MarginPreset::Wide;
        assert_eq!(p.margin_px(), (72.0, 72.0));
    }

    #[test]
    fn scale_default_100_percent() {
        let p = make_panel();
        assert_eq!(p.scale, 100);
    }

    #[test]
    fn print_backgrounds_default_on() {
        let p = make_panel();
        assert!(p.print_backgrounds);
    }

    #[test]
    fn hit_backgrounds_off_pill() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let avail_w = PANEL_W - PAD_H * 2.0 - LABEL_W;
        let pill_w = avail_w / 2.0;
        let pills_x = px + PAD_H + LABEL_W;
        let row_abs_y = py + row_y(7) + ROW_H / 2.0;
        // Second pill = Off (false).
        let hit = hit_test(&p, pills_x + pill_w * 1.5, row_abs_y, 800.0, 600.0);
        assert_eq!(hit, PrintHit::Backgrounds(false));
    }

    #[test]
    fn build_visible_has_background_row_label() {
        let mut p = make_panel();
        p.visible = true;
        let dl = build_panel(&p, 0.0, 0.0);
        let has_label = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Фон")
        });
        assert!(has_label, "must show 'Фон' background row label");
    }

    #[test]
    fn scale_can_increase_decrease() {
        let mut p = make_panel();
        p.scale = 100;
        p.scale = (p.scale + 10).min(200);
        assert_eq!(p.scale, 110);
        p.scale = (p.scale - 10).max(50);
        assert_eq!(p.scale, 100);
    }

    // ── hit_test ──────────────────────────────────────────────────────────────

    #[test]
    fn hit_outside_when_hidden() {
        let p = make_panel();
        assert_eq!(hit_test(&p, 400.0, 300.0, 800.0, 600.0), PrintHit::Outside);
    }

    #[test]
    fn hit_outside_when_visible_but_off_panel() {
        let mut p = make_panel();
        p.visible = true;
        assert_eq!(hit_test(&p, 5.0, 5.0, 800.0, 600.0), PrintHit::Outside);
    }

    #[test]
    fn hit_close_button() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let hit = hit_test(&p, px + PANEL_W - 5.0, py + HEADER_H / 2.0, 800.0, 600.0);
        assert_eq!(hit, PrintHit::Close);
    }

    #[test]
    fn hit_paper_size_first_pill() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let avail_w = PANEL_W - PAD_H * 2.0 - LABEL_W;
        let pill_w = avail_w / 3.0;
        let pills_x = px + PAD_H + LABEL_W;
        let row_abs_y = py + row_y(0) + ROW_H / 2.0;
        let hit = hit_test(&p, pills_x + pill_w * 0.5, row_abs_y, 800.0, 600.0);
        assert_eq!(hit, PrintHit::PaperSize(PaperSize::A4));
    }

    #[test]
    fn hit_orientation_landscape() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let avail_w = PANEL_W - PAD_H * 2.0 - LABEL_W;
        let pill_w = avail_w / 2.0;
        let pills_x = px + PAD_H + LABEL_W;
        let row_abs_y = py + row_y(1) + ROW_H / 2.0;
        // Second pill = Landscape
        let hit = hit_test(&p, pills_x + pill_w * 1.5, row_abs_y, 800.0, 600.0);
        assert_eq!(hit, PrintHit::Orientation(Orientation::Landscape));
    }

    #[test]
    fn hit_page_range_field() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let field_x = px + PAD_H + LABEL_W + 10.0;
        // Page range moved to row 4 when W-2b inserted the Scale row at row 3.
        let row_abs_y = py + row_y(4) + ROW_H / 2.0;
        assert_eq!(hit_test(&p, field_x, row_abs_y, 800.0, 600.0), PrintHit::PageRangeField);
    }

    #[test]
    fn hit_print_button() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let btn_y_abs = py + PANEL_H - BTN_H - PAD_V * 2.0 + BTN_H / 2.0;
        let cancel_x = px + PANEL_W - PAD_H - BTN_W;
        let print_x = cancel_x - BTN_W - 8.0 + BTN_W / 2.0;
        assert_eq!(hit_test(&p, print_x, btn_y_abs, 800.0, 600.0), PrintHit::Print);
    }

    #[test]
    fn hit_cancel_button() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let btn_y_abs = py + PANEL_H - BTN_H - PAD_V * 2.0 + BTN_H / 2.0;
        let cancel_x = px + PANEL_W - PAD_H - BTN_W + BTN_W / 2.0;
        assert_eq!(hit_test(&p, cancel_x, btn_y_abs, 800.0, 600.0), PrintHit::Cancel);
    }

    #[test]
    fn hit_scale_decrease_button() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let row_abs_y = py + row_y(3) + ROW_H / 2.0;
        let field_x = px + PAD_H + LABEL_W;
        let hit = hit_test(&p, field_x + 6.0, row_abs_y, 800.0, 600.0);
        assert_eq!(hit, PrintHit::ScaleDecrease);
    }

    #[test]
    fn hit_scale_increase_button() {
        let mut p = make_panel();
        p.visible = true;
        let (px, py) = panel_origin(800.0, 600.0);
        let row_abs_y = py + row_y(3) + ROW_H / 2.0;
        let field_x = px + PAD_H + LABEL_W;
        let btn_w = 36.0;
        let btn_gap = 4.0;
        let hit = hit_test(&p, field_x + btn_w + btn_gap + 6.0, row_abs_y, 800.0, 600.0);
        assert_eq!(hit, PrintHit::ScaleIncrease);
    }

    // ── Rendering ─────────────────────────────────────────────────────────────

    #[test]
    fn build_hidden_returns_empty() {
        let p = make_panel();
        assert!(build_panel(&p, 0.0, 0.0).is_empty());
    }

    #[test]
    fn build_visible_has_header_title() {
        let mut p = make_panel();
        p.visible = true;
        let dl = build_panel(&p, 0.0, 0.0);
        let has_title = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "Печать")
        });
        assert!(has_title, "must show 'Печать' header title");
    }

    #[test]
    fn build_visible_has_row_labels() {
        let mut p = make_panel();
        p.visible = true;
        let dl = build_panel(&p, 0.0, 0.0);
        let texts: Vec<&str> = dl.iter().filter_map(|c| {
            if let DisplayCommand::DrawText { text, .. } = c { Some(text.as_str()) } else { None }
        }).collect();
        assert!(texts.contains(&"Бумага"), "must show 'Бумага'");
        assert!(texts.contains(&"Ориентация"), "must show 'Ориентация'");
        assert!(texts.contains(&"Поля"), "must show 'Поля'");
        assert!(texts.contains(&"Масштаб"), "must show 'Масштаб' (W-2b)");
        assert!(texts.contains(&"Страницы"), "must show 'Страницы'");
        assert!(texts.contains(&"Цвет"), "must show 'Цвет'");
        assert!(texts.contains(&"Файл"), "must show 'Файл'");
    }

    #[test]
    fn build_visible_displays_scale_percentage() {
        let mut p = make_panel();
        p.visible = true;
        p.scale = 150;
        let dl = build_panel(&p, 0.0, 0.0);
        let texts: Vec<&str> = dl.iter().filter_map(|c| {
            if let DisplayCommand::DrawText { text, .. } = c { Some(text.as_str()) } else { None }
        }).collect();
        assert!(texts.iter().any(|t| t.contains("150%")), "must display scale as '150%'");
    }
}
