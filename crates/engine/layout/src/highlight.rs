//! Highlight pseudo-elements (CSS Pseudo-Elements L4 §3): `::selection`,
//! `::target-text`, `::spelling-error`, `::grammar-error` and `::highlight(name)`.
//!
//! A highlight is a set of DOM ranges ([`HighlightLayer`]) plus the cascade of one
//! highlight pseudo-element over the text they cover. [`resolve_highlights`] turns the
//! layers into [`Highlights`]: for every laid-out text fragment that a range touches,
//! the fragment cut into [`HighlightPiece`]s (the covered and the uncovered stretches,
//! in pixels and in bytes) and, on each covered piece, the [`HighlightLook`]s stacked
//! over it. Paint reads the result and needs neither the DOM nor the stylesheet.
//!
//! The cascade follows highlight inheritance (§3.3): a highlight style inherits from
//! the same pseudo-element of the parent element, not from the originating element, so
//! `div::selection { color: green }` also colours the selected text of a child `<span>`.
//! Lengths (`em` in a `text-shadow`) resolve against the text's own font.

use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeData, NodeId, Range};

use crate::style::{highlight_inherited_style, Color, ComputedStyle, UserSelect};
use crate::{BoxKind, InlineFrag, LayoutBox, TextMeasurer};

/// UA default `::selection` background when no author `::selection` rule applies
/// (CSS Pseudo-Elements L4 §3.2: the UA's own highlight colours stand in only while
/// the author styles neither the colour nor the background).
pub const UA_SELECTION_BACKGROUND: Color = Color { r: 0x30, g: 0x8a, b: 0xff, a: 110 };

/// `color` placeholder of the chain's root parent: a highlight style whose `color` is
/// still this value declared none, so the text keeps its own colour.
const UNDECLARED_COLOR: Color = Color { r: 0x01, g: 0x02, b: 0x03, a: 0x04 };

/// One highlight: the pseudo-element that styles it and the ranges it covers.
pub struct HighlightLayer<'a> {
    /// Pseudo-element request as [`crate::style::compute_pseudo_element_style`] takes it:
    /// `"selection"`, `"target-text"`, `"spelling-error"`, `"grammar-error"` or
    /// `"highlight(<name>)"`.
    pub pseudo: &'a str,
    /// Covered ranges (collapsed ones cover nothing).
    pub ranges: &'a [Range],
    /// Background the UA paints when no author rule styles this pseudo-element.
    pub ua_background: Option<Color>,
}

/// Document, stylesheet and metrics a resolve runs against.
pub struct HighlightEnv<'a> {
    /// The document the layout tree was built from.
    pub doc: &'a Document,
    /// Its author stylesheet.
    pub sheet: &'a Stylesheet,
    /// Viewport the media queries of `sheet` are evaluated against.
    pub viewport: Size,
    /// `prefers-color-scheme: dark`.
    pub dark_mode: bool,
    /// Glyph advances — cuts a fragment at a character boundary.
    pub measurer: &'a dyn TextMeasurer,
}

/// What one highlight looks like over a stretch of text.
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightLook {
    /// Filled behind the glyphs; `None` — nothing.
    pub background: Option<Color>,
    /// Glyph colour; `None` — the text keeps its own.
    pub color: Option<Color>,
    /// The highlight pseudo-element's computed style (shadows, decorations, stroke).
    style: Arc<ComputedStyle>,
}

impl HighlightLook {
    /// `base` with this highlight's text properties laid over it: the glyph colour, the
    /// shadows, the decorations and the stroke come from the highlight, everything else
    /// (font, geometry) stays the text's own.
    pub fn apply_to(&self, base: &ComputedStyle) -> ComputedStyle {
        let h = &*self.style;
        let mut s = base.clone();
        if let Some(c) = self.color {
            s.color = c;
        }
        s.text_fill_color = h.text_fill_color;
        s.text_shadow = h.text_shadow.clone();
        s.text_decoration_line = h.text_decoration_line;
        s.text_decoration_color = h.text_decoration_color;
        s.text_decoration_style = h.text_decoration_style;
        s.text_decoration_thickness = h.text_decoration_thickness;
        s.text_decoration_skip_ink = h.text_decoration_skip_ink;
        s.text_underline_position = h.text_underline_position;
        s.text_underline_offset = h.text_underline_offset;
        s.text_underline_offset_pct = h.text_underline_offset_pct;
        s.text_stroke_width = h.text_stroke_width;
        s.text_stroke_color = h.text_stroke_color;
        s
    }
}

/// A stretch of one fragment's text with the highlights over it.
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightPiece {
    /// Byte range within `InlineFrag::text`.
    pub bytes: std::ops::Range<usize>,
    /// Horizontal start, relative to the fragment's `x`.
    pub x: f32,
    /// Width in px.
    pub width: f32,
    /// Highlights over the stretch, lowest layer first; empty — not highlighted.
    pub looks: Vec<Arc<HighlightLook>>,
}

/// Fragments cut at highlight boundaries — see the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Highlights {
    frags: HashMap<(NodeId, u32), Vec<HighlightPiece>>,
}

impl Highlights {
    /// No fragment is touched by a highlight.
    pub fn is_empty(&self) -> bool {
        self.frags.is_empty()
    }

    /// The pieces `frag` is cut into; `None` when no highlight touches it.
    pub fn pieces(&self, frag: &InlineFrag) -> Option<&[HighlightPiece]> {
        self.frags.get(&(frag.source_node, frag.source_char_offset)).map(Vec::as_slice)
    }
}

/// Tree-order position `(preorder index of a node, byte offset in it)`.
type Pos = (usize, u32);

/// Byte spans of the text nodes a range covers, in the node's own coordinates.
type NodeSpans = HashMap<NodeId, Vec<(u32, u32)>>;

/// Resolves `layers` against the laid-out tree `root`. `None` when no highlight
/// reaches a text fragment — the common case, which must stay free.
pub fn resolve_highlights(root: &LayoutBox, env: &HighlightEnv<'_>, layers: &[HighlightLayer<'_>]) -> Option<Highlights> {
    if layers.iter().all(|l| l.ranges.iter().all(Range::is_collapsed)) {
        return None;
    }
    let order = TreeOrder::new(env.doc);
    let spans: Vec<NodeSpans> = layers.iter().map(|l| order.node_spans(env.doc, l.ranges)).collect();
    if spans.iter().all(HashMap::is_empty) {
        return None;
    }
    let memo = vec![StyleMemo::new(); layers.len()];
    let mut cx = Resolver { env, layers, spans, memo, out: Highlights::default() };
    cx.walk(root);
    (!cx.out.is_empty()).then_some(cx.out)
}

/// Preorder numbering of the document — what turns range endpoints into comparable
/// positions.
struct TreeOrder {
    pre: HashMap<NodeId, usize>,
    /// Exclusive end of each node's subtree in preorder.
    end: HashMap<NodeId, usize>,
    /// Text nodes in tree order.
    texts: Vec<NodeId>,
}

impl TreeOrder {
    fn new(doc: &Document) -> Self {
        let mut o = Self { pre: HashMap::new(), end: HashMap::new(), texts: Vec::new() };
        // Iterative DFS: (node, next child index).
        let mut stack: Vec<(NodeId, usize)> = Vec::new();
        let mut n = 0usize;
        let root = doc.root();
        o.pre.insert(root, n);
        n += 1;
        stack.push((root, 0));
        while let Some((node, child)) = stack.pop() {
            let kids = &doc.get(node).children;
            // A host's shadow root comes before its light children: the flat tree
            // shows the shadow content in the host's place.
            let shadow = doc.shadow_root_of(node);
            let lead = usize::from(shadow.is_some());
            if child < kids.len() + lead {
                stack.push((node, child + 1));
                let k = match shadow {
                    Some(sr) if child == 0 => sr,
                    _ => kids[child - lead],
                };
                o.pre.insert(k, n);
                n += 1;
                if matches!(doc.get(k).data, NodeData::Text(_)) {
                    o.texts.push(k);
                }
                stack.push((k, 0));
            } else {
                o.end.insert(node, n);
            }
        }
        o
    }

    /// Where a range endpoint sits: inside a text node, or on the boundary before a
    /// child of an element (`(preorder of the next node, 0)`).
    fn pos(&self, doc: &Document, container: NodeId, offset: u32) -> Option<Pos> {
        let node = doc.get(container);
        if matches!(node.data, NodeData::Text(_)) {
            return Some((*self.pre.get(&container)?, offset));
        }
        // Offset 0 of a shadow host reaches the shadow content that stands in for its children.
        let lead = match doc.shadow_root_of(container) {
            Some(sr) if offset == 0 => Some(sr),
            _ => None,
        };
        let at = match lead.or_else(|| node.children.get(offset as usize).copied()) {
            Some(child) => *self.pre.get(&child)?,
            None => *self.end.get(&container)?,
        };
        Some((at, 0))
    }

    /// The text of each text node `ranges` covers, as byte spans of that node.
    fn node_spans(&self, doc: &Document, ranges: &[Range]) -> NodeSpans {
        let mut out: NodeSpans = HashMap::new();
        for r in ranges.iter().filter(|r| !r.is_collapsed()) {
            let (Some(a), Some(b)) = (
                self.pos(doc, r.start.container, r.start.offset),
                self.pos(doc, r.end.container, r.end.offset),
            ) else {
                continue;
            };
            let (start, end) = if a <= b { (a, b) } else { (b, a) };
            // `texts` is in preorder: only the nodes from `start` to `end` are visited.
            let from = self.texts.partition_point(|t| self.pre[t] < start.0);
            let to = self.texts.partition_point(|t| self.pre[t] <= end.0);
            for &t in &self.texts[from..to] {
                let p = self.pre[&t];
                let NodeData::Text(s) = &doc.get(t).data else { continue };
                let lo = if p == start.0 { start.1 } else { 0 };
                let hi = if p == end.0 { end.1 } else { u32::MAX };
                // An element boundary `(p, 0)` as the end excludes the node at `p`.
                if p == end.0 && end.1 == 0 {
                    continue;
                }
                let hi = hi.min(s.len() as u32);
                if lo < hi {
                    out.entry(t).or_default().push((lo, hi));
                }
            }
        }
        out
    }
}

struct Resolver<'a, 'b> {
    env: &'a HighlightEnv<'a>,
    layers: &'a [HighlightLayer<'b>],
    spans: Vec<NodeSpans>,
    /// Per layer: element → the highlight style of the chain ending at that element.
    memo: Vec<StyleMemo>,
    out: Highlights,
}

impl Resolver<'_, '_> {
    fn walk(&mut self, b: &LayoutBox) {
        if let BoxKind::InlineRun { lines, .. } = &b.kind {
            for frag in lines.iter().flatten() {
                if !frag.text.is_empty() && frag.img_src.is_none() {
                    self.frag(frag);
                }
            }
        }
        for child in &b.children {
            self.walk(child);
        }
    }

    /// The look of `layer` over text node `text`, whose own style is `base`.
    fn look(&mut self, layer: usize, text: NodeId, base: &ComputedStyle) -> Option<Arc<HighlightLook>> {
        let element = self.env.doc.get(text).parent?;
        let style = chain_style(self.env, self.layers[layer].pseudo, element, base, &mut self.memo[layer]);
        let ua_bg = self.layers[layer].ua_background;
        match style {
            Some(style) => {
                let background = style
                    .background_color
                    .map(|c| c.resolve(if style.color == UNDECLARED_COLOR { base.color } else { style.color }))
                    .filter(|c| c.a > 0);
                let color = (style.color != UNDECLARED_COLOR).then_some(style.color);
                Some(Arc::new(HighlightLook { background, color, style }))
            }
            None => ua_bg.map(|bg| {
                Arc::new(HighlightLook {
                    background: Some(bg),
                    color: None,
                    style: Arc::new(chain_root(base)),
                })
            }),
        }
    }

    fn frag(&mut self, frag: &InlineFrag) {
        let spans = crate::frag_source_spans(frag);
        let base = frag.text.as_ptr() as usize;
        // A span's text is trimmed of the space that joins it to the next text node; the
        // piece keeps it, so a highlight over both nodes has no hole at the join.
        let starts: Vec<usize> = spans.iter().map(|s| s.text.as_ptr() as usize - base).collect();
        let mut pieces: Vec<HighlightPiece> = Vec::new();
        let mut any = false;
        for (i, span) in spans.iter().enumerate() {
            let start = starts[i];
            let (end, width) = match spans.get(i + 1) {
                Some(next) => (starts[i + 1], (next.x - span.x).max(0.0)),
                None => (frag.text.len(), (frag.width - span.x).max(0.0)),
            };
            let cuts = self.span_pieces(frag, span, start..end, width);
            any |= cuts.iter().any(|p| !p.looks.is_empty());
            pieces.extend(cuts);
        }
        if any {
            self.out.frags.insert((frag.source_node, frag.source_char_offset), pieces);
        }
    }

    /// One text-node span of a fragment (`bytes` of `frag.text`, `width` px wide), cut
    /// where a highlight starts or ends.
    fn span_pieces(
        &mut self,
        frag: &InlineFrag,
        span: &crate::FragSpan<'_>,
        bytes: std::ops::Range<usize>,
        width: f32,
    ) -> Vec<HighlightPiece> {
        let node = span.source_node;
        let (start, len) = (bytes.start, bytes.len());
        let text = &frag.text[bytes];
        // Segments of this node per layer, in the span's own byte coordinates.
        let mut segments: Vec<(usize, usize, usize)> = Vec::new();
        // The DOM text and the fragment's text differ where whitespace collapsed.
        let align = match &self.env.doc.get(node).data {
            NodeData::Text(raw) => Some(align_offsets(raw, span.source_char_offset as usize, text, frag.style.white_space.preserves_whitespace())),
            _ => None,
        };
        for (layer, spans) in self.spans.iter().enumerate() {
            if self.layers[layer].pseudo == "selection" && frag.style.user_select == UserSelect::None {
                continue;
            }
            for &(lo, hi) in spans.get(&node).map(Vec::as_slice).unwrap_or_default() {
                let (a, b) = match &align {
                    Some(map) => (map_offset(map, lo as usize), map_offset(map, hi as usize)),
                    None => (
                        lo.saturating_sub(span.source_char_offset) as usize,
                        hi.saturating_sub(span.source_char_offset) as usize,
                    ),
                };
                let (a, b) = (floor_boundary(text, a), floor_boundary(text, b));
                if a < b {
                    segments.push((a, b, layer));
                }
            }
        }
        let whole = || {
            vec![HighlightPiece { bytes: start..start + len, x: span.x, width, looks: Vec::new() }]
        };
        if segments.is_empty() || len == 0 {
            return whole();
        }
        // A right-to-left run is not cut: the highlight covers it whole.
        if frag.bidi_level % 2 == 1 {
            segments = vec![(0, len, segments[0].2)];
        }
        let mut cuts: Vec<usize> = vec![0, len];
        for &(a, b, _) in &segments {
            cuts.push(a);
            cuts.push(b);
        }
        cuts.sort_unstable();
        cuts.dedup();
        let total = text_width(text, frag, self.env.measurer);
        let x_at = |b: usize| -> f32 {
            let frac = if total > 0.0 {
                text_width(&text[..b], frag, self.env.measurer) / total
            } else {
                b as f32 / len as f32
            };
            span.x + width * frac
        };
        let mut looks_by_layer: HashMap<usize, Option<Arc<HighlightLook>>> = HashMap::new();
        let mut out = Vec::new();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let mut layers_here: Vec<usize> = segments
                .iter()
                .filter(|&&(sa, sb, _)| sa <= a && b <= sb)
                .map(|&(_, _, l)| l)
                .collect();
            layers_here.sort_unstable();
            layers_here.dedup();
            let mut looks = Vec::new();
            for l in layers_here {
                let look = match looks_by_layer.get(&l) {
                    Some(v) => v.clone(),
                    None => {
                        let v = self.look(l, node, &frag.style);
                        looks_by_layer.insert(l, v.clone());
                        v
                    }
                };
                looks.extend(look);
            }
            let (x0, x1) = (x_at(a), x_at(b));
            out.push(HighlightPiece { bytes: start + a..start + b, x: x0, width: (x1 - x0).max(0.0), looks });
        }
        out
    }
}

/// Element → the highlight style of the chain ending at it (`None`: no rule so far).
type StyleMemo = HashMap<NodeId, Option<Arc<ComputedStyle>>>;

/// The `pseudo` highlight style of `element` after highlight inheritance (§3.3): the
/// rules that match each ancestor's `::pseudo`, cascaded root-first, every level
/// inheriting from the one above it. `None` when no element of the chain has a rule.
/// `base` is the text's own style: the chain's root inherits its font and lengths from it.
fn chain_style(
    env: &HighlightEnv<'_>,
    pseudo: &str,
    element: NodeId,
    base: &ComputedStyle,
    memo: &mut StyleMemo,
) -> Option<Arc<ComputedStyle>> {
    let doc = env.doc;
    // Ancestors from the element up to the first one already in `memo`.
    let mut chain = Vec::new();
    let mut cur = Some(element);
    while let Some(n) = cur {
        if matches!(doc.get(n).data, NodeData::Element { .. }) {
            if memo.contains_key(&n) {
                break;
            }
            chain.push(n);
        }
        // A shadow root has no parent: the cascade continues at its host.
        cur = doc.get(n).parent.or_else(|| doc.shadow_host_of(n));
    }
    let mut parent = cur.and_then(|n| memo.get(&n).cloned()).flatten();
    for &el in chain.iter().rev() {
        let mut basis = match &parent {
            Some(p) => (**p).clone(),
            None => chain_root(base),
        };
        // `var()` in a highlight resolves against the originating element's custom
        // properties, not those a highlight above it declared (CSS Pseudo L4 §3.4).
        basis.custom_props = base.custom_props.clone();
        // A rule none of whose declarations applies (`foo: bar`) leaves the style as the
        // inherited one: an invalid declaration block is ignored, so it does not count as
        // the author styling the highlight (no UA background is suppressed by it).
        let inherited = highlight_inherited_style(&basis);
        let own = crate::style::compute_pseudo_element_style(
            doc,
            el,
            pseudo,
            env.sheet,
            &basis,
            env.viewport,
            env.dark_mode,
        )
        .filter(|s| *s != inherited);
        let style = match (own, &parent) {
            (Some(s), _) => Some(Arc::new(s)),
            (None, Some(p)) => Some(Arc::new(highlight_inherited_style(p))),
            (None, None) => None,
        };
        memo.insert(el, style.clone());
        parent = style;
    }
    parent
}

/// A stand-in for the originating element's computed style, from the two values of its
/// computed-style snapshot the highlight cascade needs: the text colour and the font size.
/// Anything missing or unparsable stays at its initial value.
pub fn originating_base(color: Option<&str>, font_size: Option<&str>) -> ComputedStyle {
    let mut base = ComputedStyle::root();
    if let Some(c) = color.and_then(crate::style::parse_color) {
        base.color = c;
    }
    if let Some(px) = font_size.and_then(|v| v.trim().strip_suffix("px")).and_then(|v| v.trim().parse::<f32>().ok()) {
        base.font_size = px;
    }
    base
}

/// Computed style of the `pseudo` highlight pseudo-element of `element` — what
/// `getComputedStyle(element, "::selection")` and its siblings read. `None` when no rule
/// of the chain styles it. `base` is the element's own style ([`originating_base`]): the
/// font and lengths of the chain, and the colour a highlight that declares none keeps.
pub fn compute_highlight_style(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    element: NodeId,
    pseudo: &str,
    base: &ComputedStyle,
) -> Option<ComputedStyle> {
    let env = HighlightEnv { doc, sheet, viewport, dark_mode, measurer: &NoMeasure };
    let mut style = chain_style(&env, pseudo, element, base, &mut StyleMemo::new()).map(|s| (*s).clone())?;
    if style.color == UNDECLARED_COLOR {
        style.color = base.color;
    }
    Some(style)
}

/// A measurer for callers that never cut a fragment.
struct NoMeasure;

impl TextMeasurer for NoMeasure {
    fn char_width(&self, _: char, _: f32) -> f32 {
        0.0
    }
}

/// The style a chain of highlight styles starts from: the text's own style with the
/// properties a highlight does not inherit from the originating element reset, and the
/// colour marked as undeclared.
fn chain_root(text: &ComputedStyle) -> ComputedStyle {
    let initial = ComputedStyle::root();
    let mut s = text.clone();
    s.color = UNDECLARED_COLOR;
    s.text_fill_color = initial.text_fill_color;
    s.text_shadow = Vec::new();
    s.text_decoration_line = initial.text_decoration_line;
    s.text_decoration_color = initial.text_decoration_color;
    s.text_decoration_style = initial.text_decoration_style;
    s.text_decoration_thickness = initial.text_decoration_thickness;
    s.text_stroke_width = initial.text_stroke_width;
    s.text_stroke_color = initial.text_stroke_color;
    s.text_emphasis_style = initial.text_emphasis_style;
    s.text_emphasis_color = initial.text_emphasis_color;
    s.background_color = None;
    s
}

/// Sum of the glyph advances of `text` in the font of `frag`.
fn text_width(text: &str, frag: &InlineFrag, m: &dyn TextMeasurer) -> f32 {
    text.chars().map(|c| m.char_width_with_families(c, frag.style.font_size, &frag.style.font_family)).sum()
}

/// `(offset in the DOM text, offset in the fragment text)` for every character of `text`
/// and for its end. The fragment's text is the DOM text from `start` with every run of
/// collapsible whitespace (CSS Text L3 §4.1.1) reduced to one space, unless `preserve`.
fn align_offsets(raw: &str, start: usize, text: &str, preserve: bool) -> Vec<(usize, usize)> {
    let is_ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}');
    let mut r = floor_boundary(raw, start);
    let mut out = Vec::with_capacity(text.len() + 1);
    for (ti, tc) in text.char_indices() {
        out.push((r, ti));
        if !preserve && is_ws(tc) {
            while let Some(c) = raw[r..].chars().next().filter(|&c| is_ws(c)) {
                r += c.len_utf8();
            }
        } else if let Some(c) = raw[r..].chars().next() {
            r += c.len_utf8();
        }
    }
    out.push((r, text.len()));
    out
}

/// The fragment-text offset of DOM-text offset `raw_off`, per [`align_offsets`]: the start
/// of the character it falls in.
fn map_offset(map: &[(usize, usize)], raw_off: usize) -> usize {
    match map.partition_point(|&(r, _)| r <= raw_off) {
        0 => 0,
        i => map[i - 1].1,
    }
}

/// `i` clamped to `s` and moved down to a char boundary.
fn floor_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests;
