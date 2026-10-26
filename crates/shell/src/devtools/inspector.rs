//! DevTools DOM inspector panel (§7E.1) with Computed tab (§7E.2) and Styles
//! panel (§PH3-1).
//!
//! Lets the user inspect the rendered page: while the inspector is active,
//! moving the mouse highlights the box under the cursor with a Chrome-style
//! [`DisplayCommand::BoxModelOverlay`] (margin / border / padding / content),
//! and clicking a box "pins" it — showing its DOM label, [`NodeId`] and a
//! computed-style map in a right-docked side panel.
//!
//! The panel has four tabs:
//! - **Elements** — box-model geometry + most-used CSS properties (§7E.1).
//! - **Styles** — CSS rules that matched the selected element, with selectors
//!   and raw declaration values as written in the stylesheet (§PH3-1).
//! - **Computed** — full CSS computed-style map (~55 properties) from P4's
//!   [`lumen_layout::computed_style_to_map`] (§7E.2).
//! - **Network** — HTTP request log table (§7E.4, CC-9).
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
use lumen_layout::{Color, LayoutBox};
use lumen_paint::DisplayCommand;

use super::network_panel::NetworkEntry;

// ── Layout constants ────────────────────────────────────────────────────────────

/// Width of the right-docked side panel in CSS px (`.insp-panel` in `chrome.html`).
pub const PANEL_WIDTH: f32 = 300.0;
/// Maximum number of property rows visible without scrolling.
const MAX_VISIBLE_ROWS: usize = 22;
/// Maximum number of request rows visible on the Network tab without scrolling.
const NET_MAX_VISIBLE_ROWS: usize = 24;

// ── Types ───────────────────────────────────────────────────────────────────────

/// Which tab of the DevTools inspector panel is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    /// Elements tab — box-model geometry and most-used CSS properties.
    #[default]
    Elements,
    /// Styles tab — CSS rules matched by the selected element, with selectors
    /// and raw declaration values (§PH3-1).
    Styles,
    /// Computed tab — full CSS computed-style map (~55 properties, §7E.2).
    Computed,
    /// Network tab — request log table: method / status / time / URL (§7E.4, CC-9).
    Network,
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
    /// Styles-tab: CSS rules matched by this element (§PH3-1).
    ///
    /// Each entry is `(selector_text, declarations)`. Rules are in source
    /// order from the stylesheet.
    pub styles_rules: Vec<(String, Vec<(String, String)>)>,
    /// Computed-tab: full CSS computed-style map sorted alphabetically (§7E.2).
    pub computed_props: Vec<(String, String)>,
    /// First property row to show in the Elements tab (scroll position, 0 = top).
    pub scroll_offset: usize,
    /// First visual line to skip in the Styles tab (header + decl lines).
    pub styles_scroll_offset: usize,
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
    /// Snapshot of network-request entries for the Network tab, oldest first.
    /// Refreshed from the shared `NetworkLog` before each redraw via
    /// [`set_network_entries`]. Independent of [`selected`] — the Network tab is
    /// page-wide, not per-element.
    ///
    /// [`set_network_entries`]: DomInspectorPanel::set_network_entries
    /// [`selected`]: DomInspectorPanel::selected
    pub network_entries: Vec<NetworkEntry>,
    /// How many request rows to skip from the bottom on the Network tab
    /// (0 = show the newest tail; scrolling up grows it towards older rows).
    pub network_scroll_offset: usize,
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
    /// `styles_rules` — Styles-tab list: `(selector, declarations)` pairs in source order.
    /// `computed_props` — Computed-tab list (full CSS map, sorted alphabetically).
    pub fn select(
        &mut self,
        node: NodeId,
        label: String,
        props: Vec<(String, String)>,
        styles_rules: Vec<(String, Vec<(String, String)>)>,
        computed_props: Vec<(String, String)>,
    ) {
        self.selected = Some(SelectedNode {
            node,
            label,
            props,
            styles_rules,
            computed_props,
            scroll_offset: 0,
            styles_scroll_offset: 0,
            computed_scroll_offset: 0,
        });
    }

    /// Switch the active tab to `tab`.
    pub fn switch_tab(&mut self, tab: InspectorTab) {
        self.active_tab = tab;
    }

    /// Replace the Network-tab snapshot with `entries` (oldest first). Clamps the
    /// scroll offset so it never points past the available rows. Call before
    /// building the panel on each redraw.
    pub fn set_network_entries(&mut self, entries: Vec<NetworkEntry>) {
        let max = entries.len().saturating_sub(NET_MAX_VISIBLE_ROWS);
        self.network_scroll_offset = self.network_scroll_offset.min(max);
        self.network_entries = entries;
    }

    /// Returns `true` if `x` is inside the right-docked panel, given window CSS width.
    ///
    /// Used by the shell click handler to distinguish panel UI interactions from
    /// page hit-tests.
    pub fn is_panel_click(&self, x: f32, win_w_css: f32) -> bool {
        x >= win_w_css - PANEL_WIDTH
    }

    /// Scroll the active tab's list up.
    ///
    /// For the Elements/Styles/Computed tabs "up" means towards the top of the
    /// property list; for the Network tab it means towards older requests.
    pub fn scroll_up(&mut self, n: usize) {
        if self.active_tab == InspectorTab::Network {
            let max = self.network_entries.len().saturating_sub(NET_MAX_VISIBLE_ROWS);
            self.network_scroll_offset = (self.network_scroll_offset + n).min(max);
            return;
        }
        let Some(sel) = self.selected.as_mut() else { return };
        match self.active_tab {
            InspectorTab::Elements => {
                sel.scroll_offset = sel.scroll_offset.saturating_sub(n);
            }
            InspectorTab::Styles => {
                sel.styles_scroll_offset = sel.styles_scroll_offset.saturating_sub(n);
            }
            InspectorTab::Computed => {
                sel.computed_scroll_offset = sel.computed_scroll_offset.saturating_sub(n);
            }
            InspectorTab::Network => {}
        }
    }

    /// Scroll the active tab's list down, clamped so the last page stays visible.
    ///
    /// For the Elements/Styles/Computed tabs "down" means towards the bottom of
    /// the property list; for the Network tab it means towards newer requests.
    pub fn scroll_down(&mut self, n: usize) {
        if self.active_tab == InspectorTab::Network {
            self.network_scroll_offset = self.network_scroll_offset.saturating_sub(n);
            return;
        }
        let Some(sel) = self.selected.as_mut() else { return };
        match self.active_tab {
            InspectorTab::Elements => {
                let max = sel.props.len().saturating_sub(MAX_VISIBLE_ROWS);
                sel.scroll_offset = (sel.scroll_offset + n).min(max);
            }
            InspectorTab::Styles => {
                let total_lines = styles_total_lines(&sel.styles_rules);
                let max = total_lines.saturating_sub(MAX_VISIBLE_ROWS);
                sel.styles_scroll_offset = (sel.styles_scroll_offset + n).min(max);
            }
            InspectorTab::Computed => {
                let max = sel.computed_props.len().saturating_sub(MAX_VISIBLE_ROWS);
                sel.computed_scroll_offset = (sel.computed_scroll_offset + n).min(max);
            }
            InspectorTab::Network => {}
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
) -> Vec<DisplayCommand> {
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
        NodeData::ProcessingInstruction { target, .. } => format!("?{target}"),
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
    out.push(("font-weight".into(), s.font_weight.serialize()));
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

// ── Styles-tab helpers ─────────────────────────────────────────────────────────

/// Count the total visual lines in the Styles-tab flat view.
///
/// Each rule contributes 1 line for the selector + N lines for its declarations
/// + 1 separator line (except after the last rule). Used for scroll clamping.
fn styles_total_lines(rules: &[(String, Vec<(String, String)>)]) -> usize {
    if rules.is_empty() {
        return 0;
    }
    rules.iter().map(|(_, decls)| 1 + decls.len()).sum::<usize>() + rules.len() - 1
}

// ── Chrome model ────────────────────────────────────────────────────────────────

impl DomInspectorPanel {
    /// Snapshot for the engine chrome's `#inspPanel` (UX-CHROME-PANELS): the
    /// visible window of the active tab. The scroll offsets stay here — the
    /// chrome has no scroll container for it, the wheel is routed to
    /// [`DomInspectorPanel::scroll_up`]/[`scroll_down`](Self::scroll_down).
    pub fn chrome_model(&self) -> lumen_chrome::ChromeInspectorModel {
        use lumen_chrome::{ChromeInspectorModel, ChromeInspectorRowModel as Row};
        let mut m = ChromeInspectorModel {
            open: self.visible,
            tab: match self.active_tab {
                InspectorTab::Elements => 0,
                InspectorTab::Styles => 1,
                InspectorTab::Computed => 2,
                InspectorTab::Network => 3,
            },
            ..ChromeInspectorModel::default()
        };
        if !self.visible {
            return m;
        }
        if self.active_tab == InspectorTab::Network {
            let total = self.network_entries.len();
            let end = total.saturating_sub(self.network_scroll_offset);
            let start = end.saturating_sub(NET_MAX_VISIBLE_ROWS);
            m.net_rows = self.network_entries[start..end]
                .iter()
                .map(super::network_panel::chrome_row)
                .collect();
            if total > NET_MAX_VISIBLE_ROWS {
                m.position = format!("{end}/{total}");
            }
            return m;
        }
        let Some(sel) = self.selected.as_ref() else {
            m.hint = "Hover a box, then click to inspect.".to_owned();
            return m;
        };
        m.label = sel.label.clone();
        m.node = format!("NodeId({})", sel.node.index());
        let (rows, scroll): (Vec<Row>, usize) = match self.active_tab {
            InspectorTab::Elements => (prop_rows(&sel.props), sel.scroll_offset),
            InspectorTab::Computed => (prop_rows(&sel.computed_props), sel.computed_scroll_offset),
            InspectorTab::Styles => {
                if sel.styles_rules.is_empty() {
                    m.hint = "(no CSS rules matched)".to_owned();
                }
                (style_rows(&sel.styles_rules), sel.styles_scroll_offset)
            }
            InspectorTab::Network => (Vec::new(), 0),
        };
        let total = rows.len();
        let start = scroll.min(total);
        let end = (start + MAX_VISIBLE_ROWS).min(total);
        if total > MAX_VISIBLE_ROWS {
            m.position = format!("{end}/{total}");
        }
        m.rows = rows[start..end].to_vec();
        m
    }
}

fn prop_rows(props: &[(String, String)]) -> Vec<lumen_chrome::ChromeInspectorRowModel> {
    props
        .iter()
        .map(|(k, v)| lumen_chrome::ChromeInspectorRowModel { kind: 0, key: k.clone(), value: v.clone() })
        .collect()
}

/// Flat line list of the Styles tab: per rule a selector line and its
/// declarations, rules separated by one separator line (counted by
/// [`styles_total_lines`]).
fn style_rows(rules: &[(String, Vec<(String, String)>)]) -> Vec<lumen_chrome::ChromeInspectorRowModel> {
    use lumen_chrome::ChromeInspectorRowModel as Row;
    let mut out = Vec::new();
    for (idx, (selector, decls)) in rules.iter().enumerate() {
        if idx > 0 {
            out.push(Row { kind: 2, ..Row::default() });
        }
        out.push(Row { kind: 1, key: selector.clone(), value: String::new() });
        out.extend(decls.iter().map(|(k, v)| Row { kind: 0, key: k.clone(), value: v.clone() }));
    }
    out
}

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
            vec![],
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
        p.select(NodeId::from_index(1), "div".into(), many.clone(), vec![], many);
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
        p.select(NodeId::from_index(1), "div".into(), props.clone(), vec![], props);
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
    fn network_is_fourth_tab_variant() {
        // Network is the fourth tab (after Elements, Styles, Computed).
        let mut p = DomInspectorPanel::new();
        p.switch_tab(InspectorTab::Network);
        assert_eq!(p.active_tab, InspectorTab::Network);
        assert_ne!(InspectorTab::Network, InspectorTab::default());
    }

    #[test]
    fn set_network_entries_clamps_scroll_offset() {
        let mut p = DomInspectorPanel::new();
        p.network_scroll_offset = 999;
        // Few entries → nothing to scroll → offset clamps to 0.
        p.set_network_entries(net_entries(&[("GET", "https://a/", 200)]));
        assert_eq!(p.network_scroll_offset, 0);
        assert_eq!(p.network_entries.len(), 1);
    }

    #[test]
    fn network_scroll_clamps_to_entry_count() {
        let mut p = DomInspectorPanel::new();
        p.switch_tab(InspectorTab::Network);
        let rows: Vec<(&str, &str, u16)> =
            (0..NET_MAX_VISIBLE_ROWS + 10).map(|_| ("GET", "https://a/x", 200)).collect();
        p.set_network_entries(net_entries(&rows));
        // Scroll up (towards older) is clamped to (total - visible cap).
        p.scroll_up(9999);
        assert_eq!(p.network_scroll_offset, (NET_MAX_VISIBLE_ROWS + 10) - NET_MAX_VISIBLE_ROWS);
        // Scroll down (towards newest) returns to the tail.
        p.scroll_down(9999);
        assert_eq!(p.network_scroll_offset, 0);
    }

    #[test]
    fn network_scroll_does_not_touch_element_selection() {
        // Scrolling the Network tab must not move the Elements/Computed offsets.
        let mut p = DomInspectorPanel::new();
        let many: Vec<(String, String)> =
            (0..MAX_VISIBLE_ROWS + 5).map(|i| (format!("k{i}"), "v".into())).collect();
        p.select(NodeId::from_index(1), "div".into(), many.clone(), vec![], many);
        p.switch_tab(InspectorTab::Network);
        let rows: Vec<(&str, &str, u16)> =
            (0..NET_MAX_VISIBLE_ROWS + 3).map(|_| ("GET", "https://a/x", 200)).collect();
        p.set_network_entries(net_entries(&rows));
        p.scroll_up(2);
        assert_eq!(p.network_scroll_offset, 2);
        let sel = p.selected.as_ref().unwrap();
        assert_eq!(sel.scroll_offset, 0);
        assert_eq!(sel.computed_scroll_offset, 0);
    }

    /// Completed network entries as the shell feeds them (same path as the JS log).
    fn net_entries(rows: &[(&str, &str, u16)]) -> Vec<NetworkEntry> {
        use super::super::network_panel::NetworkLog;
        let mut log = NetworkLog::default();
        for (m, u, s) in rows {
            log.record_js(m, u, Some(*s), Some(7));
        }
        log.entries.clone()
    }

    #[test]
    fn chrome_model_hidden_is_closed() {
        let m = DomInspectorPanel::new().chrome_model();
        assert!(!m.open);
        assert!(m.rows.is_empty() && m.net_rows.is_empty());
    }

    #[test]
    fn chrome_model_without_selection_shows_hint() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        let m = p.chrome_model();
        assert!(m.open);
        assert!(m.hint.starts_with("Hover a box"));
        assert!(m.label.is_empty());
    }

    #[test]
    fn chrome_model_elements_window_and_position() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        let many: Vec<(String, String)> =
            (0..MAX_VISIBLE_ROWS + 5).map(|i| (format!("k{i}"), "v".into())).collect();
        p.select(NodeId::from_index(7), "div#a".into(), many, vec![], vec![]);
        p.scroll_down(2);
        let m = p.chrome_model();
        assert_eq!(m.label, "div#a");
        assert_eq!(m.node, "NodeId(7)");
        assert_eq!(m.rows.len(), MAX_VISIBLE_ROWS);
        assert_eq!(m.rows[0].key, "k2");
        assert_eq!(m.position, format!("{}/{}", MAX_VISIBLE_ROWS + 2, MAX_VISIBLE_ROWS + 5));
    }

    #[test]
    fn chrome_model_styles_has_selectors_and_separators() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        let rules = vec![
            ("a".to_owned(), vec![("color".to_owned(), "red".to_owned())]),
            ("b".to_owned(), vec![("margin".to_owned(), "0".to_owned())]),
        ];
        p.select(NodeId::from_index(1), "x".into(), vec![], rules, vec![]);
        p.switch_tab(InspectorTab::Styles);
        let m = p.chrome_model();
        let kinds: Vec<u8> = m.rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, vec![1, 0, 2, 1, 0]);
        assert_eq!(m.tab, 1);
    }

    #[test]
    fn chrome_model_styles_without_rules_hints() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        p.select(NodeId::from_index(1), "x".into(), vec![], vec![], vec![]);
        p.switch_tab(InspectorTab::Styles);
        assert_eq!(p.chrome_model().hint, "(no CSS rules matched)");
    }

    #[test]
    fn chrome_model_network_tab_needs_no_selection() {
        let mut p = DomInspectorPanel::new();
        p.toggle();
        p.switch_tab(InspectorTab::Network);
        p.set_network_entries(net_entries(&[("GET", "https://a/x", 200)]));
        let m = p.chrome_model();
        assert_eq!(m.tab, 3);
        assert_eq!(m.net_rows.len(), 1);
        assert_eq!(m.net_rows[0].status, "200");
        assert!(m.hint.is_empty());
    }
}
