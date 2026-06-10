//! DevTools DOM inspector panel (§7E.1) with Computed tab (§7E.2).
//!
//! Lets the user inspect the rendered page: while the inspector is active,
//! moving the mouse highlights the box under the cursor with a Chrome-style
//! [`DisplayCommand::BoxModelOverlay`] (margin / border / padding / content),
//! and clicking a box "pins" it — showing its DOM label, [`NodeId`] and a
//! computed-style map in a right-docked side panel.
//!
//! The panel has two tabs:
//! - **Elements** — box-model geometry + most-used CSS properties (§7E.1).
//! - **Computed** — full CSS computed-style map (~55 properties) from P4's
//!   [`lumen_layout::computed_style_to_map`] (§7E.2).
//!
//! Toggle with `Ctrl+Shift+I` (the standard DevTools inspector binding; `F12`
//! is already taken by the JS console, see [`super::console_panel`]).
//!
//! # Architecture
//!
//! The panel is pure UI state: the shell drives it from input handlers.
//! - `CursorMoved` while [`DomInspectorPanel::visible`] → hit-test the page and
//!   call [`DomInspectorPanel::set_hovered`].
//! - A left click while visible → [`DomInspectorPanel::select`] (and the shell
//!   suppresses normal navigation / JS dispatch).
//! - A click inside the panel header tab row → [`DomInspectorPanel::click_tab_at`]
//!   switches active tab without re-selecting.
//!
//! All rendering happens in the redraw compositing step:
//! - [`build_box_overlay`] emits the hovered box-model overlay (page → viewport
//!   coordinates via the supplied offset).
//! - [`build_inspector_panel`] emits the right-docked computed-style side panel.

use lumen_core::geom::{Rect, Size};
use lumen_dom::{Document, NodeData, NodeId};
use lumen_layout::{Color, FontStyle, FontWeight, LayoutBox};
use lumen_paint::{DisplayCommand, DisplayList};

// ── Colours ───────────────────────────────────────────────────────────────────

const PANEL_BG: Color = Color { r: 24, g: 24, b: 28, a: 244 };
const HEADER_BG: Color = Color { r: 32, g: 33, b: 38, a: 255 };
const TAB_ACTIVE_BG: Color = Color { r: 24, g: 24, b: 28, a: 255 };
const TAB_INACTIVE_BG: Color = Color { r: 40, g: 41, b: 48, a: 255 };
const TAB_ACTIVE_LINE: Color = Color { r: 66, g: 135, b: 245, a: 255 };
const FG_KEY: Color = Color { r: 130, g: 180, b: 250, a: 255 };
const FG_VAL: Color = Color { r: 210, g: 212, b: 218, a: 255 };
const FG_DIM: Color = Color { r: 150, g: 152, b: 160, a: 255 };
const FG_TAG: Color = Color { r: 240, g: 170, b: 110, a: 255 };
const FG_TAB: Color = Color { r: 190, g: 192, b: 200, a: 255 };
const FG_TAB_ACTIVE: Color = Color { r: 220, g: 222, b: 230, a: 255 };

// ── Layout constants ────────────────────────────────────────────────────────────

/// Width of the right-docked side panel in CSS px.
pub const PANEL_WIDTH: f32 = 300.0;
const HEADER_H: f32 = 30.0;
/// Height of the tab row below the header.
pub const TAB_ROW_H: f32 = 26.0;
const LINE_H: f32 = 18.0;
const FONT_SIZE: f32 = 12.0;
const H_PAD: f32 = 10.0;
/// Width of the "Elements" tab button.
const TAB_ELEMENTS_W: f32 = 82.0;
/// Width of the "Computed" tab button.
const TAB_COMPUTED_W: f32 = 90.0;
/// Maximum number of property rows visible without scrolling.
const MAX_VISIBLE_ROWS: usize = 22;

// ── Types ───────────────────────────────────────────────────────────────────────

/// Which tab of the DevTools inspector panel is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    /// Elements tab — box-model geometry and most-used CSS properties.
    #[default]
    Elements,
    /// Computed tab — full CSS computed-style map (~55 properties, §7E.2).
    Computed,
}

/// A node currently pinned by the inspector, with its computed-style snapshot.
#[derive(Debug, Clone)]
pub struct SelectedNode {
    /// DOM node that was clicked.
    pub node: NodeId,
    /// Human-readable element label, e.g. `div#main.card` (or `#text`).
    pub label: String,
    /// Elements-tab: box-model geometry + most-used CSS properties.
    pub props: Vec<(String, String)>,
    /// Computed-tab: full CSS computed-style map sorted alphabetically (§7E.2).
    pub computed_props: Vec<(String, String)>,
    /// First property row to show in the Elements tab (scroll position, 0 = top).
    pub scroll_offset: usize,
    /// First property row to show in the Computed tab.
    pub computed_scroll_offset: usize,
}

/// DevTools DOM inspector panel state.
///
/// Holds the currently hovered node (for the box-model overlay) and the pinned
/// selection (for the computed-style side panel). Toggled with `Ctrl+Shift+I`.
#[derive(Debug, Clone, Default)]
pub struct DomInspectorPanel {
    /// Whether the inspector is active. When `false`, no overlay or panel draws
    /// and the shell does not intercept hover / click.
    pub visible: bool,
    /// Node currently under the cursor (for the live box-model overlay).
    pub hovered: Option<NodeId>,
    /// Node pinned by the last click, with its computed-style snapshot.
    pub selected: Option<SelectedNode>,
    /// Which tab is currently shown.
    pub active_tab: InspectorTab,
}

impl DomInspectorPanel {
    /// Create a hidden inspector with no hover or selection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle inspector activity. Clears hover (but keeps the last selection)
    /// when turning off so the overlay does not linger.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.hovered = None;
        }
    }

    /// Update the node under the cursor. Returns `true` when the value changed
    /// (so the caller can request a redraw only on a real transition).
    pub fn set_hovered(&mut self, node: Option<NodeId>) -> bool {
        if self.hovered == node {
            return false;
        }
        self.hovered = node;
        true
    }

    /// Pin a node as the current selection.
    ///
    /// `props` — Elements-tab list (box-model geometry + most-used CSS properties).
    /// `computed_props` — Computed-tab list (full CSS map, sorted alphabetically).
    pub fn select(
        &mut self,
        node: NodeId,
        label: String,
        props: Vec<(String, String)>,
        computed_props: Vec<(String, String)>,
    ) {
        self.selected = Some(SelectedNode {
            node,
            label,
            props,
            computed_props,
            scroll_offset: 0,
            computed_scroll_offset: 0,
        });
    }

    /// Switch the active tab to `tab`.
    pub fn switch_tab(&mut self, tab: InspectorTab) {
        self.active_tab = tab;
    }

    /// Returns `true` if `x` is inside the right-docked panel, given window CSS width.
    ///
    /// Used by the shell click handler to distinguish panel UI interactions from
    /// page hit-tests.
    pub fn is_panel_click(&self, x: f32, win_w_css: f32) -> bool {
        x >= win_w_css - PANEL_WIDTH
    }

    /// Handle a click that is inside the panel. Switches tab when the click lands
    /// on the tab row. Returns `true` when the click was consumed.
    pub fn click_tab_at(&mut self, x: f32, y: f32, win_w_css: f32, top: f32) -> bool {
        let panel_x = win_w_css - PANEL_WIDTH;
        let tab_y = top + HEADER_H;
        if y < tab_y || y > tab_y + TAB_ROW_H {
            return false;
        }
        let local_x = x - panel_x;
        if local_x < TAB_ELEMENTS_W {
            self.switch_tab(InspectorTab::Elements);
            return true;
        }
        if local_x < TAB_ELEMENTS_W + TAB_COMPUTED_W {
            self.switch_tab(InspectorTab::Computed);
            return true;
        }
        false
    }

    /// Scroll the active tab's property list up (towards the top).
    pub fn scroll_up(&mut self, n: usize) {
        let Some(sel) = self.selected.as_mut() else { return };
        match self.active_tab {
            InspectorTab::Elements => {
                sel.scroll_offset = sel.scroll_offset.saturating_sub(n);
            }
            InspectorTab::Computed => {
                sel.computed_scroll_offset = sel.computed_scroll_offset.saturating_sub(n);
            }
        }
    }

    /// Scroll the active tab's property list down (towards the bottom), clamped
    /// so the last page of rows stays visible.
    pub fn scroll_down(&mut self, n: usize) {
        let Some(sel) = self.selected.as_mut() else { return };
        match self.active_tab {
            InspectorTab::Elements => {
                let max = sel.props.len().saturating_sub(MAX_VISIBLE_ROWS);
                sel.scroll_offset = (sel.scroll_offset + n).min(max);
            }
            InspectorTab::Computed => {
                let max = sel.computed_props.len().saturating_sub(MAX_VISIBLE_ROWS);
                sel.computed_scroll_offset = (sel.computed_scroll_offset + n).min(max);
            }
        }
    }
}

// ── Layout-tree helpers ─────────────────────────────────────────────────────────

/// Find the [`LayoutBox`] for `node` in document order. Returns `None` when the
/// node has no box (e.g. `display: none` or not in the tree).
pub fn find_box(root: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
    if root.node == node {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_box(child, node) {
            return Some(found);
        }
    }
    None
}

/// Compute the four box-model rectangles for `lb` in document (page) coordinates.
///
/// `lb.rect` is the border-box (includes padding + border, excludes margin).
/// Padding and border come from the resolved [`lumen_layout::ComputedStyle`]; margins are
/// resolved with `auto` → 0 (percentages → 0, matching the layout caveat that
/// `%` margins are not yet honoured). Returns `(margin, border, padding, content)`
/// from outermost to innermost.
pub fn box_model_rects(lb: &LayoutBox, viewport: Size) -> (Rect, Rect, Rect, Rect) {
    let s = &lb.style;
    let em = s.font_size;

    let bt = s.border_top_width;
    let br = s.border_right_width;
    let bb = s.border_bottom_width;
    let bl = s.border_left_width;

    let pt = s.padding_top.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    let pb = s.padding_bottom.resolve_or_zero(em, 0.0, viewport);
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);

    let mt = margin_px(&s.margin_top, em, viewport);
    let mr = margin_px(&s.margin_right, em, viewport);
    let mb = margin_px(&s.margin_bottom, em, viewport);
    let ml = margin_px(&s.margin_left, em, viewport);

    let border = lb.rect;
    let margin = Rect::new(
        border.x - ml,
        border.y - mt,
        border.width + ml + mr,
        border.height + mt + mb,
    );
    let padding = Rect::new(
        border.x + bl,
        border.y + bt,
        (border.width - bl - br).max(0.0),
        (border.height - bt - bb).max(0.0),
    );
    let content = Rect::new(
        padding.x + pl,
        padding.y + pt,
        (padding.width - pl - pr).max(0.0),
        (padding.height - pt - pb).max(0.0),
    );
    (margin, border, padding, content)
}

/// Resolve a `margin` [`lumen_layout::LengthOrAuto`] to px, treating `auto` as 0.
fn margin_px(m: &lumen_layout::LengthOrAuto, em: f32, viewport: Size) -> f32 {
    match m {
        lumen_layout::LengthOrAuto::Auto => 0.0,
        lumen_layout::LengthOrAuto::Length(l) => l.resolve_or_zero(em, 0.0, viewport),
    }
}

/// Build the box-model overlay for the hovered box, translated from page
/// coordinates into viewport coordinates by `offset = (dx, dy)`.
///
/// `dx` typically accounts for a left side-panel (vertical tabs) minus
/// `scroll_x`; `dy` for the tab-bar height minus `scroll_y`. Returns an empty
/// list when the inspector is hidden or there is no hovered box.
pub fn build_box_overlay(
    panel: &DomInspectorPanel,
    root: &LayoutBox,
    viewport: Size,
    offset: (f32, f32),
) -> DisplayList {
    if !panel.visible {
        return Vec::new();
    }
    let Some(node) = panel.hovered else {
        return Vec::new();
    };
    let Some(lb) = find_box(root, node) else {
        return Vec::new();
    };
    let (margin, border, padding, content) = box_model_rects(lb, viewport);
    let (dx, dy) = offset;
    vec![DisplayCommand::BoxModelOverlay {
        margin: shift(margin, dx, dy),
        border: shift(border, dx, dy),
        padding: shift(padding, dx, dy),
        content: shift(content, dx, dy),
    }]
}

fn shift(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(r.x + dx, r.y + dy, r.width, r.height)
}

// ── Computed-style extraction ────────────────────────────────────────────────────

/// Build a human-readable DOM label for `node`, e.g. `div#main.card`, `#text`,
/// or `#comment`. Element id/class come from the source attributes.
pub fn element_label(doc: &Document, node: NodeId) -> String {
    match &doc.get(node).data {
        NodeData::Element { name, attrs } => {
            let mut label = name.local.to_string();
            if let Some(id) = attrs.iter().find(|a| a.name.local == "id") {
                let v = id.value.trim();
                if !v.is_empty() {
                    label.push('#');
                    label.push_str(v);
                }
            }
            if let Some(class) = attrs.iter().find(|a| a.name.local == "class") {
                for c in class.value.split_whitespace() {
                    label.push('.');
                    label.push_str(c);
                }
            }
            label
        }
        NodeData::Text(_) => "#text".to_string(),
        NodeData::Comment(_) => "#comment".to_string(),
        NodeData::Document => "#document".to_string(),
        NodeData::Doctype { .. } => "#doctype".to_string(),
        NodeData::ShadowRoot { .. } => "#shadow-root".to_string(),
        NodeData::DocumentFragment => "#document-fragment".to_string(),
    }
}

/// Extract a curated computed-style map from a [`LayoutBox`] as ordered
/// `(property, value)` pairs for the **Elements** tab. Covers the box model and
/// the most common visual properties; geometry rows come from the resolved
/// layout `rect`.
pub fn computed_style_map(lb: &LayoutBox) -> Vec<(String, String)> {
    let s = &lb.style;
    let mut out: Vec<(String, String)> = Vec::with_capacity(16);

    let r = lb.rect;
    out.push(("x".into(), fmt_px(r.x)));
    out.push(("y".into(), fmt_px(r.y)));
    out.push(("width".into(), fmt_px(r.width)));
    out.push(("height".into(), fmt_px(r.height)));

    out.push(("display".into(), format!("{:?}", s.display).to_lowercase()));
    out.push(("position".into(), format!("{:?}", s.position).to_lowercase()));
    out.push(("color".into(), fmt_color(s.color)));
    out.push((
        "background-color".into(),
        s.background_color
            .as_ref()
            .map_or_else(|| "transparent".to_string(), |_| "set".to_string()),
    ));
    out.push(("font-size".into(), fmt_px(s.font_size)));
    out.push(("font-weight".into(), s.font_weight.0.to_string()));
    out.push(("opacity".into(), fmt_num(s.opacity)));

    out.push((
        "margin".into(),
        fmt_quad(
            margin_str(&s.margin_top),
            margin_str(&s.margin_right),
            margin_str(&s.margin_bottom),
            margin_str(&s.margin_left),
        ),
    ));
    out.push((
        "border-width".into(),
        fmt_quad(
            fmt_px(s.border_top_width),
            fmt_px(s.border_right_width),
            fmt_px(s.border_bottom_width),
            fmt_px(s.border_left_width),
        ),
    ));
    out.push((
        "padding".into(),
        fmt_quad(
            fmt_len(&s.padding_top),
            fmt_len(&s.padding_right),
            fmt_len(&s.padding_bottom),
            fmt_len(&s.padding_left),
        ),
    ));

    out
}

fn fmt_px(v: f32) -> String {
    format!("{v:.1}px")
}

fn fmt_num(v: f32) -> String {
    format!("{v:.2}")
}

fn fmt_len(l: &lumen_layout::Length) -> String {
    fmt_px(l.px())
}

fn margin_str(m: &lumen_layout::LengthOrAuto) -> String {
    match m {
        lumen_layout::LengthOrAuto::Auto => "auto".to_string(),
        lumen_layout::LengthOrAuto::Length(l) => fmt_px(l.px()),
    }
}

fn fmt_quad(t: String, r: String, b: String, l: String) -> String {
    if t == r && r == b && b == l {
        t
    } else {
        format!("{t} {r} {b} {l}")
    }
}

fn fmt_color(c: Color) -> String {
    if c.a == 255 {
        format!("rgb({}, {}, {})", c.r, c.g, c.b)
    } else {
        format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, c.a)
    }
}

// ── Rendering: side panel ─────────────────────────────────────────────────────────

/// Build the right-docked inspector side panel.
///
/// `(win_w, win_h)` are window dimensions in CSS px. The panel is anchored to
/// the right edge below `top` (the tab-bar height) and shows the pinned node's
/// label, [`NodeId`] and a scrollable property list in the active tab.
/// Returns an empty list when the inspector is hidden.
pub fn build_inspector_panel(
    panel: &DomInspectorPanel,
    (win_w, win_h): (u32, u32),
    top: f32,
) -> DisplayList {
    if !panel.visible {
        return Vec::new();
    }

    let panel_x = win_w as f32 - PANEL_WIDTH;
    let panel_h = win_h as f32 - top;
    let mut out: DisplayList = Vec::with_capacity(16 + MAX_VISIBLE_ROWS * 2);

    // Background + left border.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(panel_x, top, PANEL_WIDTH, panel_h),
        color: PANEL_BG,
    });
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(panel_x, top, 1.0, panel_h),
        color: HEADER_BG,
    });

    // Header.
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(panel_x, top, PANEL_WIDTH, HEADER_H),
        color: HEADER_BG,
    });
    out.push(make_text(
        "Inspector  (Ctrl+Shift+I)".to_string(),
        panel_x + H_PAD,
        top + (HEADER_H - FONT_SIZE) / 2.0,
        PANEL_WIDTH - H_PAD * 2.0,
        FONT_SIZE,
        FG_DIM,
    ));

    // Tab row.
    let tab_y = top + HEADER_H;
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(panel_x, tab_y, PANEL_WIDTH, TAB_ROW_H),
        color: HEADER_BG,
    });
    draw_tab(
        &mut out,
        "Elements",
        panel_x,
        tab_y,
        TAB_ELEMENTS_W,
        panel.active_tab == InspectorTab::Elements,
    );
    draw_tab(
        &mut out,
        "Computed",
        panel_x + TAB_ELEMENTS_W,
        tab_y,
        TAB_COMPUTED_W,
        panel.active_tab == InspectorTab::Computed,
    );

    let content_top = tab_y + TAB_ROW_H;

    let Some(sel) = panel.selected.as_ref() else {
        out.push(make_text(
            "Hover a box, then click to inspect.".to_string(),
            panel_x + H_PAD,
            content_top + 8.0,
            PANEL_WIDTH - H_PAD * 2.0,
            FONT_SIZE,
            FG_DIM,
        ));
        return out;
    };

    // Selected element label + NodeId.
    let mut y = content_top + 6.0;
    out.push(make_text(
        sel.label.clone(),
        panel_x + H_PAD,
        y,
        PANEL_WIDTH - H_PAD * 2.0,
        FONT_SIZE + 1.0,
        FG_TAG,
    ));
    y += LINE_H;
    out.push(make_text(
        format!("NodeId({})", sel.node.index()),
        panel_x + H_PAD,
        y,
        PANEL_WIDTH - H_PAD * 2.0,
        FONT_SIZE,
        FG_DIM,
    ));
    y += LINE_H + 4.0;

    // Property rows for the active tab.
    let (props, scroll_offset) = match panel.active_tab {
        InspectorTab::Elements => (&sel.props, sel.scroll_offset),
        InspectorTab::Computed => (&sel.computed_props, sel.computed_scroll_offset),
    };

    let total = props.len();
    let start = scroll_offset.min(total);
    let end = (start + MAX_VISIBLE_ROWS).min(total);
    for (key, val) in &props[start..end] {
        out.push(make_text(
            format!("{key}:"),
            panel_x + H_PAD,
            y,
            PANEL_WIDTH * 0.45,
            FONT_SIZE,
            FG_KEY,
        ));
        out.push(make_text(
            val.clone(),
            panel_x + PANEL_WIDTH * 0.46,
            y,
            PANEL_WIDTH * 0.5,
            FONT_SIZE,
            FG_VAL,
        ));
        y += LINE_H;
    }

    if total > MAX_VISIBLE_ROWS {
        out.push(make_text(
            format!("{end}/{total}"),
            panel_x + PANEL_WIDTH - 60.0,
            top + (HEADER_H - FONT_SIZE) / 2.0,
            54.0,
            FONT_SIZE,
            FG_DIM,
        ));
    }

    out
}

/// Emit a single tab button into `out`.
fn draw_tab(out: &mut DisplayList, label: &str, x: f32, y: f32, w: f32, active: bool) {
    let bg = if active { TAB_ACTIVE_BG } else { TAB_INACTIVE_BG };
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(x, y, w, TAB_ROW_H),
        color: bg,
    });
    if active {
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(x, y + TAB_ROW_H - 2.0, w, 2.0),
            color: TAB_ACTIVE_LINE,
        });
    }
    let fg = if active { FG_TAB_ACTIVE } else { FG_TAB };
    out.push(make_text(
        label.to_string(),
        x + 8.0,
        y + (TAB_ROW_H - FONT_SIZE) / 2.0,
        w - 16.0,
        FONT_SIZE,
        fg,
    ));
}

fn make_text(text: String, x: f32, y: f32, w: f32, font_size: f32, color: Color) -> DisplayCommand {
    DisplayCommand::DrawText {
        rect: Rect::new(x, y, w, font_size * 1.4),
        text,
        font_size,
        color,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
        highlight_name: None,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::geom::Size;
    use lumen_layout::layout;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn build(html: &str, css: &str) -> (Document, LayoutBox) {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout(&doc, &sheet, VP);
        (doc, root)
    }

    fn by_tag(doc: &Document, tag: &str) -> NodeId {
        let mut stack = vec![doc.root()];
        while let Some(id) = stack.pop() {
            let n = doc.get(id);
            if let NodeData::Element { name, .. } = &n.data
                && name.local == tag
            {
                return id;
            }
            for &c in n.children.iter().rev() {
                stack.push(c);
            }
        }
        panic!("no <{tag}>");
    }

    #[test]
    fn new_is_hidden_empty() {
        let p = DomInspectorPanel::new();
        assert!(!p.visible);
        assert!(p.hovered.is_none());
        assert!(p.selected.is_none());
        assert_eq!(p.active_tab, InspectorTab::Elements);
    }

    #[test]
    fn toggle_clears_hover_when_hiding() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        assert!(p.visible);
        p.hovered = Some(NodeId::from_index(3));
        p.toggle();
        assert!(!p.visible);
        assert!(p.hovered.is_none());
    }

    #[test]
    fn set_hovered_reports_change() {
        let mut p = DomInspectorPanel::new();
        assert!(p.set_hovered(Some(NodeId::from_index(1))));
        assert!(!p.set_hovered(Some(NodeId::from_index(1))));
        assert!(p.set_hovered(None));
    }

    #[test]
    fn find_box_locates_node() {
        let (doc, root) = build("<div><p>x</p></div>", "p { height: 40px; }");
        let p_id = by_tag(&doc, "p");
        let lb = find_box(&root, p_id).expect("p box");
        assert_eq!(lb.node, p_id);
    }

    #[test]
    fn box_model_nesting_is_ordered() {
        let (doc, root) = build(
            "<div></div>",
            "div { width: 100px; height: 50px; margin: 10px; border: 4px solid black; padding: 6px; }",
        );
        let div = by_tag(&doc, "div");
        let lb = find_box(&root, div).expect("div box");
        let (margin, border, padding, content) = box_model_rects(lb, VP);
        // margin ⊇ border ⊇ padding ⊇ content on every edge.
        assert!(margin.x <= border.x && border.x <= padding.x && padding.x <= content.x);
        assert!(margin.y <= border.y && border.y <= padding.y && padding.y <= content.y);
        assert!(margin.width >= border.width && border.width >= padding.width);
        assert!(padding.width >= content.width);
        // border box is exactly lb.rect.
        assert_eq!(border.width, lb.rect.width);
    }

    #[test]
    fn box_overlay_hidden_when_invisible() {
        let (doc, root) = build("<div></div>", "div { height: 30px; }");
        let div = by_tag(&doc, "div");
        let mut p = DomInspectorPanel::new();
        p.set_hovered(Some(div));
        assert!(build_box_overlay(&p, &root, VP, (0.0, 0.0)).is_empty());
    }

    #[test]
    fn box_overlay_emits_command_when_hovered() {
        let (doc, root) = build("<div></div>", "div { height: 30px; }");
        let div = by_tag(&doc, "div");
        let mut p = DomInspectorPanel::new();
        p.toggle();
        p.set_hovered(Some(div));
        let dl = build_box_overlay(&p, &root, VP, (5.0, 7.0));
        assert_eq!(dl.len(), 1);
        assert!(matches!(dl[0], DisplayCommand::BoxModelOverlay { .. }));
    }

    #[test]
    fn box_overlay_applies_offset() {
        let (doc, root) = build("<div></div>", "div { height: 30px; margin: 0; }");
        let div = by_tag(&doc, "div");
        let base = find_box(&root, div).unwrap().rect;
        let mut p = DomInspectorPanel::new();
        p.toggle();
        p.set_hovered(Some(div));
        let dl = build_box_overlay(&p, &root, VP, (5.0, 7.0));
        if let DisplayCommand::BoxModelOverlay { border, .. } = &dl[0] {
            assert!((border.x - (base.x + 5.0)).abs() < 1e-3);
            assert!((border.y - (base.y + 7.0)).abs() < 1e-3);
        } else {
            panic!("expected BoxModelOverlay");
        }
    }

    #[test]
    fn element_label_includes_id_and_class() {
        let doc = lumen_html_parser::parse(r#"<div id="main" class="card big">x</div>"#);
        let div = by_tag(&doc, "div");
        assert_eq!(element_label(&doc, div), "div#main.card.big");
    }

    #[test]
    fn computed_style_map_has_geometry_and_display() {
        let (doc, root) = build("<div></div>", "div { width: 120px; height: 40px; }");
        let div = by_tag(&doc, "div");
        let lb = find_box(&root, div).unwrap();
        let map = computed_style_map(lb);
        let keys: Vec<&str> = map.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"width"));
        assert!(keys.contains(&"height"));
        assert!(keys.contains(&"display"));
        assert!(keys.contains(&"padding"));
        let w = map.iter().find(|(k, _)| k == "width").unwrap();
        assert_eq!(w.1, "120.0px");
    }

    #[test]
    fn select_stores_snapshot() {
        let mut p = DomInspectorPanel::new();
        p.select(
            NodeId::from_index(7),
            "div".to_string(),
            vec![("display".into(), "block".into())],
            vec![("color".into(), "rgb(0,0,0)".into())],
        );
        let sel = p.selected.as_ref().unwrap();
        assert_eq!(sel.node, NodeId::from_index(7));
        assert_eq!(sel.props.len(), 1);
        assert_eq!(sel.computed_props.len(), 1);
        assert_eq!(sel.scroll_offset, 0);
        assert_eq!(sel.computed_scroll_offset, 0);
    }

    #[test]
    fn switch_tab_changes_active_tab() {
        let mut p = DomInspectorPanel::new();
        assert_eq!(p.active_tab, InspectorTab::Elements);
        p.switch_tab(InspectorTab::Computed);
        assert_eq!(p.active_tab, InspectorTab::Computed);
        p.switch_tab(InspectorTab::Elements);
        assert_eq!(p.active_tab, InspectorTab::Elements);
    }

    #[test]
    fn click_tab_at_switches_tabs() {
        let mut p = DomInspectorPanel::new();
        let win_w = 1280.0_f32;
        let top = 36.0_f32;
        let panel_x = win_w - PANEL_WIDTH;
        let tab_y = top + HEADER_H + TAB_ROW_H / 2.0;
        // Click "Elements" tab.
        p.switch_tab(InspectorTab::Computed);
        assert!(p.click_tab_at(panel_x + 10.0, tab_y, win_w, top));
        assert_eq!(p.active_tab, InspectorTab::Elements);
        // Click "Computed" tab.
        assert!(p.click_tab_at(panel_x + TAB_ELEMENTS_W + 10.0, tab_y, win_w, top));
        assert_eq!(p.active_tab, InspectorTab::Computed);
        // Click outside tab row → not consumed.
        assert!(!p.click_tab_at(panel_x + 10.0, top + 5.0, win_w, top));
    }

    #[test]
    fn is_panel_click_detects_right_side() {
        let p = DomInspectorPanel::new();
        let win_w = 1280.0_f32;
        assert!(p.is_panel_click(win_w - 10.0, win_w));
        assert!(!p.is_panel_click(win_w - PANEL_WIDTH - 1.0, win_w));
    }

    #[test]
    fn scroll_per_active_tab() {
        let mut p = DomInspectorPanel::new();
        let many: Vec<(String, String)> =
            (0..MAX_VISIBLE_ROWS + 5).map(|i| (format!("k{i}"), "v".into())).collect();
        p.select(NodeId::from_index(1), "div".into(), many.clone(), many);
        // Elements tab scroll.
        p.scroll_down(3);
        assert_eq!(p.selected.as_ref().unwrap().scroll_offset, 3);
        assert_eq!(p.selected.as_ref().unwrap().computed_scroll_offset, 0);
        // Computed tab scroll.
        p.switch_tab(InspectorTab::Computed);
        p.scroll_down(2);
        assert_eq!(p.selected.as_ref().unwrap().scroll_offset, 3);
        assert_eq!(p.selected.as_ref().unwrap().computed_scroll_offset, 2);
    }

    #[test]
    fn scroll_clamps_to_props_len() {
        let mut p = DomInspectorPanel::new();
        let props: Vec<(String, String)> =
            (0..MAX_VISIBLE_ROWS + 10).map(|i| (format!("k{i}"), "v".into())).collect();
        p.select(NodeId::from_index(1), "div".into(), props.clone(), props);
        p.scroll_down(9999);
        assert_eq!(p.selected.as_ref().unwrap().scroll_offset, 10);
        p.scroll_up(9999);
        assert_eq!(p.selected.as_ref().unwrap().scroll_offset, 0);
        p.switch_tab(InspectorTab::Computed);
        p.scroll_down(9999);
        assert_eq!(p.selected.as_ref().unwrap().computed_scroll_offset, 10);
        p.scroll_up(9999);
        assert_eq!(p.selected.as_ref().unwrap().computed_scroll_offset, 0);
    }

    #[test]
    fn panel_hidden_returns_empty() {
        let p = DomInspectorPanel::new();
        assert!(build_inspector_panel(&p, (1280, 800), 36.0).is_empty());
    }

    #[test]
    fn panel_visible_empty_has_header() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        let dl = build_inspector_panel(&p, (1280, 800), 36.0);
        let has_header = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text.contains("Inspector"))
        });
        assert!(has_header);
    }

    #[test]
    fn panel_shows_tab_buttons() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        let dl = build_inspector_panel(&p, (1280, 800), 36.0);
        assert!(dl.iter().any(|c| matches!(
            c, DisplayCommand::DrawText { text, .. } if text == "Elements"
        )));
        assert!(dl.iter().any(|c| matches!(
            c, DisplayCommand::DrawText { text, .. } if text == "Computed"
        )));
    }

    #[test]
    fn panel_shows_selection_label_and_node_id() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        p.select(
            NodeId::from_index(42),
            "p#intro".to_string(),
            vec![("display".into(), "block".into())],
            vec![("color".into(), "rgb(0,0,0)".into())],
        );
        let dl = build_inspector_panel(&p, (1280, 800), 36.0);
        assert!(dl.iter().any(|c| matches!(
            c, DisplayCommand::DrawText { text, .. } if text.contains("p#intro")
        )));
        assert!(dl.iter().any(|c| matches!(
            c, DisplayCommand::DrawText { text, .. } if text.contains("NodeId(42)")
        )));
        assert!(dl.iter().any(|c| matches!(
            c, DisplayCommand::DrawText { text, .. } if text.contains("display")
        )));
    }

    #[test]
    fn panel_computed_tab_shows_computed_props() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        p.select(
            NodeId::from_index(1),
            "div".into(),
            vec![("display".into(), "block".into())],
            vec![("color".into(), "rgb(255,0,0)".into())],
        );
        p.switch_tab(InspectorTab::Computed);
        let dl = build_inspector_panel(&p, (1280, 800), 36.0);
        assert!(dl.iter().any(|c| matches!(
            c, DisplayCommand::DrawText { text, .. } if text.contains("color")
        )));
    }
}
