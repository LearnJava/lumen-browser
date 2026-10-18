//! CSS Shapes L1 `shape-outside` → contour primitives for a float in a *vertical*
//! writing mode (LAYOUT-VFLOAT-3).
//!
//! The old [`super::shapes_floats::register_shape_outside`] understands only the
//! `Npx` forms and works in the horizontal frame. A vertical float needs more:
//! `<shape-box>` on its own (a rounded border/padding/content/margin box),
//! `at <position>` with keywords, `closest-side` / `farthest-side`, percentages,
//! elliptical `inset()` radii and a `linear-gradient()` image — and the contour
//! has to be turned by 90° into the float context's logical frame (`x` = inline
//! axis = physical `y`, `y` = block axis = physical `x`, mirrored for `vertical-rl`).
//!
//! [`resolve_shape`] works in physical px relative to the float's margin-box
//! top-left corner; [`Prim::rotate`] / [`Prim::translate`] bring a primitive into
//! the context's frame, where [`Prim::extent_at`] answers "which inline range does
//! the shape occupy at block position `y`".

use super::shapes_floats::{parse_shape_path_px, polygon_edge_x_at_y_margin};
use crate::style::{Length, ParsedGradient};
use lumen_core::geom::Size;

/// One piece of a float's contour. In [`resolve_shape`]'s output the coordinates
/// are physical px from the margin-box top-left; after [`Prim::rotate`] they are
/// the float context's logical `(inline, block)`.
#[derive(Clone, Debug)]
pub(crate) enum Prim {
    /// `circle()` / `ellipse()`: centre and the radii along `x` and `y`.
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    /// A closed polygon (`polygon()`, `path()`, one band of a gradient).
    Polygon(Vec<(f32, f32)>),
    /// A rounded rectangle `[x0, x1] × [y0, y1]`. `radii` are `(rx, ry)` of the
    /// corners `[x0·y0, x1·y0, x1·y1, x0·y1]` — in the physical frame that is
    /// `[top-left, top-right, bottom-right, bottom-left]`.
    RRect { x0: f32, y0: f32, x1: f32, y1: f32, radii: [(f32, f32); 4] },
}

impl Prim {
    /// Turns a physical primitive (origin: margin-box top-left, `mw` = margin-box
    /// width) into the logical frame: `x' = y`, `y' = x` (`rl`: `mw - x`).
    pub(crate) fn rotate(&self, mw: f32, rl: bool) -> Prim {
        let map = |(px, py): (f32, f32)| (py, if rl { mw - px } else { px });
        match self {
            Prim::Ellipse { cx, cy, rx, ry } => {
                let (x, y) = map((*cx, *cy));
                Prim::Ellipse { cx: x, cy: y, rx: *ry, ry: *rx }
            }
            Prim::Polygon(pts) => Prim::Polygon(pts.iter().map(|&p| map(p)).collect()),
            Prim::RRect { x0, y0, x1, y1, radii } => {
                let (ny0, ny1) = if rl { (mw - x1, mw - x0) } else { (*x0, *x1) };
                // physical [tl, tr, br, bl] → logical slot [x0y0, x1y0, x1y1, x0y1]
                let slots: [usize; 4] = if rl { [3, 0, 1, 2] } else { [0, 3, 2, 1] };
                let mut out = [(0.0, 0.0); 4];
                for (phys, &slot) in slots.iter().enumerate() {
                    out[slot] = (radii[phys].1, radii[phys].0);
                }
                Prim::RRect { x0: *y0, y0: ny0, x1: *y1, y1: ny1, radii: out }
            }
        }
    }

    /// Moves the primitive by `(dx, dy)`.
    pub(crate) fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            Prim::Ellipse { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            Prim::Polygon(pts) => {
                for p in pts.iter_mut() {
                    p.0 += dx;
                    p.1 += dy;
                }
            }
            Prim::RRect { x0, y0, x1, y1, .. } => {
                *x0 += dx;
                *x1 += dx;
                *y0 += dy;
                *y1 += dy;
            }
        }
    }

    /// The `x` range the primitive — grown outward by `margin` (`shape-margin`) —
    /// covers on the scanline `y`, or `None` when the scanline misses it.
    pub(crate) fn extent_at(&self, y: f32, margin: f32) -> Option<(f32, f32)> {
        let m = margin.max(0.0);
        match self {
            Prim::Ellipse { cx, cy, rx, ry } => {
                let (rx, ry) = (rx + m, ry + m);
                if ry <= 0.0 || rx <= 0.0 {
                    return None;
                }
                let norm = (y - cy) / ry;
                if norm.abs() > 1.0 {
                    return None;
                }
                let hw = rx * (1.0 - norm * norm).max(0.0).sqrt();
                Some((cx - hw, cx + hw))
            }
            Prim::Polygon(pts) => {
                let l = polygon_edge_x_at_y_margin(pts, y, m, false)?;
                let r = polygon_edge_x_at_y_margin(pts, y, m, true)?;
                Some((l, r))
            }
            Prim::RRect { x0, y0, x1, y1, radii } => {
                let (x0, y0, x1, y1) = (x0 - m, y0 - m, x1 + m, y1 + m);
                if y < y0 || y >= y1 || x1 <= x0 {
                    return None;
                }
                // A sharp corner grown by `m` becomes a quarter circle of radius `m`.
                let grown = |r: (f32, f32)| if m > 0.0 { (r.0 + m, r.1 + m) } else { r };
                let inward = |r: (f32, f32), lo_side: bool| {
                    let (rx, ry) = grown(r);
                    let d = if lo_side { y0 + ry - y } else { y - (y1 - ry) };
                    corner_inward(rx, ry, d)
                };
                let left = inward(radii[0], true).max(inward(radii[3], false));
                let right = inward(radii[1], true).max(inward(radii[2], false));
                Some((x0 + left, x1 - right))
            }
        }
    }
}

/// How far the edge of an elliptical corner (radii `rx`, `ry`) sits inside its
/// bounding square at a distance `d` from the far side of the corner band
/// (`d` is measured from the corner's outer edge towards the centre of the box:
/// `ry` at the band's start, `0` where the band ends). Zero outside the band.
fn corner_inward(rx: f32, ry: f32, d: f32) -> f32 {
    if rx <= 0.0 || ry <= 0.0 || d <= 0.0 {
        return 0.0;
    }
    let dy = d.min(ry);
    rx * (1.0 - (1.0 - (dy / ry) * (dy / ry)).max(0.0).sqrt())
}

/// An axis-aligned box in physical px from the margin-box top-left.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect4 {
    pub(crate) x0: f32,
    pub(crate) y0: f32,
    pub(crate) x1: f32,
    pub(crate) y1: f32,
}

impl Rect4 {
    fn w(&self) -> f32 {
        self.x1 - self.x0
    }
    fn h(&self) -> f32 {
        self.y1 - self.y0
    }
    fn grown(&self, t: f32, r: f32, b: f32, l: f32) -> Rect4 {
        Rect4 { x0: self.x0 - l, y0: self.y0 - t, x1: self.x1 + r, y1: self.y1 + b }
    }
}

/// The four boxes of a float (physical px, origin = margin-box top-left) and
/// what a `shape-outside` value needs besides them. Edge arrays are
/// `[top, right, bottom, left]`.
pub(crate) struct FloatBoxes {
    pub(crate) margin: Rect4,
    pub(crate) border: Rect4,
    pub(crate) padding: Rect4,
    pub(crate) content: Rect4,
    /// Border-box corner radii `(rx, ry)`, `[tl, tr, br, bl]`, already scaled
    /// down where adjacent radii overlap (CSS Backgrounds L3 §5.5).
    pub(crate) radii: [(f32, f32); 4],
    pub(crate) margins: [f32; 4],
    pub(crate) borders: [f32; 4],
    pub(crate) paddings: [f32; 4],
    /// The element's font size, for `em` lengths.
    pub(crate) em: f32,
    /// `shape-image-threshold`: alpha above which an image pixel is in the shape.
    pub(crate) threshold: f32,
}

impl FloatBoxes {
    /// Boxes of `child` whose laid-out border box is `child.rect`; `margins` are the
    /// resolved margins `[top, right, bottom, left]`, `cb_inline` the percentage basis
    /// of padding.
    pub(crate) fn of(
        child: &super::LayoutBox,
        margins: [f32; 4],
        cb_inline: f32,
        viewport: Size,
    ) -> FloatBoxes {
        let s = &child.style;
        let em = s.font_size;
        let pad = |l: &Length| l.resolve_or_zero(em, cb_inline, viewport);
        let paddings = [pad(&s.padding_top), pad(&s.padding_right), pad(&s.padding_bottom), pad(&s.padding_left)];
        let borders = [s.border_top_width, s.border_right_width, s.border_bottom_width, s.border_left_width];
        let (w, h) = (child.rect.width, child.rect.height);
        let [mt, mr, mb, ml] = margins;
        let margin = Rect4 { x0: 0.0, y0: 0.0, x1: ml + w + mr, y1: mt + h + mb };
        let border = Rect4 { x0: ml, y0: mt, x1: ml + w, y1: mt + h };
        let padding = border.grown(-borders[0], -borders[1], -borders[2], -borders[3]);
        let content = padding.grown(-paddings[0], -paddings[1], -paddings[2], -paddings[3]);
        let rad = |x: &Length, y: &Length| {
            let rx = x.resolve(em, Some(w), viewport).unwrap_or(0.0).max(0.0);
            let ry = y.resolve(em, Some(h), viewport).unwrap_or(0.0).max(0.0);
            (rx, ry)
        };
        let mut radii = [
            rad(&s.border_top_left_radius, &s.border_top_left_radius_y),
            rad(&s.border_top_right_radius, &s.border_top_right_radius_y),
            rad(&s.border_bottom_right_radius, &s.border_bottom_right_radius_y),
            rad(&s.border_bottom_left_radius, &s.border_bottom_left_radius_y),
        ];
        clamp_radii(&mut radii, w, h);
        FloatBoxes {
            margin,
            border,
            padding,
            content,
            radii,
            margins,
            borders,
            paddings,
            em,
            threshold: s.shape_image_threshold,
        }
    }

    /// The reference box a `<shape-box>` keyword names, with its corner radii
    /// (CSS Shapes L1 §3: the border radii shrunk by the border / padding widths
    /// for the inner boxes, grown by the margin for the margin box).
    fn reference(&self, kw: &str) -> (Rect4, [(f32, f32); 4]) {
        let sub = |r: (f32, f32), dx: f32, dy: f32| ((r.0 - dx).max(0.0), (r.1 - dy).max(0.0));
        let inset = |edges: [f32; 4], base: [(f32, f32); 4]| {
            let [t, rt, b, l] = edges;
            [sub(base[0], l, t), sub(base[1], rt, t), sub(base[2], rt, b), sub(base[3], l, b)]
        };
        match kw {
            "border-box" => (self.border, self.radii),
            "padding-box" => (self.padding, inset(self.borders, self.radii)),
            "content-box" => {
                let p = inset(self.borders, self.radii);
                (self.content, inset(self.paddings, p))
            }
            _ => {
                let [t, r, b, l] = self.margins;
                let grow = |q: (f32, f32), dx: f32, dy: f32| {
                    if q.0 > 0.0 && q.1 > 0.0 { (q.0 + dx, q.1 + dy) } else { (0.0, 0.0) }
                };
                let radii = [
                    grow(self.radii[0], l, t),
                    grow(self.radii[1], r, t),
                    grow(self.radii[2], r, b),
                    grow(self.radii[3], l, b),
                ];
                (self.margin, radii)
            }
        }
    }
}

/// CSS Backgrounds L3 §5.5 — scales every radius by the smallest `side / sum of
/// adjacent radii` that is below `1`, so neighbours never overlap.
fn clamp_radii(radii: &mut [(f32, f32); 4], w: f32, h: f32) {
    let mut f = 1.0_f32;
    let mut take = |side: f32, sum: f32| {
        if sum > side && sum > 0.0 {
            f = f.min(side / sum);
        }
    };
    take(w, radii[0].0 + radii[1].0);
    take(w, radii[3].0 + radii[2].0);
    take(h, radii[0].1 + radii[3].1);
    take(h, radii[1].1 + radii[2].1);
    if f < 1.0 {
        for r in radii.iter_mut() {
            *r = (r.0 * f, r.1 * f);
        }
    }
}

/// A `<length-percentage>` before the reference size is known.
#[derive(Clone, Copy, Debug)]
enum Lp {
    Px(f32),
    Pct(f32),
}

impl Lp {
    fn parse(tok: &str, em: f32) -> Option<Lp> {
        let t = tok.trim();
        if let Some(p) = t.strip_suffix('%') {
            return p.parse::<f32>().ok().map(Lp::Pct);
        }
        let units: [(&str, f32); 7] =
            [("px", 1.0), ("em", em), ("rem", 16.0), ("pt", 4.0 / 3.0), ("in", 96.0), ("cm", 96.0 / 2.54), ("mm", 96.0 / 25.4)];
        for (u, k) in units {
            if let Some(n) = t.strip_suffix(u) {
                return n.parse::<f32>().ok().map(|v| Lp::Px(v * k));
            }
        }
        // A unitless zero is the only bare number a length accepts.
        t.parse::<f32>().ok().filter(|v| *v == 0.0).map(Lp::Px)
    }

    fn of(self, basis: f32) -> f32 {
        match self {
            Lp::Px(v) => v,
            Lp::Pct(p) => p / 100.0 * basis,
        }
    }
}

/// Splits `s` on whitespace outside parentheses and quotes.
fn split_ws(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut quote) = (0_i32, None::<usize>, None::<char>);
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                start.get_or_insert(i);
            }
            '(' => {
                depth += 1;
                start.get_or_insert(i);
            }
            ')' => depth -= 1,
            c if c.is_whitespace() && depth <= 0 => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// Splits `s` on commas outside parentheses and quotes.
fn split_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut quote) = (0_i32, 0, None::<char>);
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn is_shape_box(t: &str) -> bool {
    matches!(t, "margin-box" | "border-box" | "padding-box" | "content-box")
}

/// CSS Shapes L1 §3 — turns a `shape-outside` value (`<basic-shape> || <shape-box>`,
/// `<shape-box>`, or a `linear-gradient()`) into contour primitives in physical px
/// from the margin-box top-left. `None` = not understood (the float keeps its
/// rectangular wrapping); an empty list = a valid shape that excludes nothing.
pub(crate) fn resolve_shape(sv: &str, bx: &FloatBoxes) -> Option<Vec<Prim>> {
    let toks = split_ws(sv.trim());
    let mut func: Option<&str> = None;
    let mut kw = None::<String>;
    for t in &toks {
        let lower = t.to_ascii_lowercase();
        if is_shape_box(&lower) {
            if kw.replace(lower).is_some() {
                return None;
            }
        } else if t.contains('(') {
            if func.replace(t).is_some() {
                return None;
            }
        } else {
            return None;
        }
    }
    let Some(func) = func else {
        // A bare `<shape-box>`: the box itself, with its rounded corners.
        let (r, radii) = bx.reference(kw.as_deref()?);
        return Some(vec![Prim::RRect { x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1, radii }]);
    };
    let open = func.find('(')?;
    let close = func.rfind(')')?;
    if close < open {
        return None;
    }
    let name = func[..open].trim().to_ascii_lowercase();
    let inner = &func[open + 1..close];
    let (rect, _) = bx.reference(kw.as_deref().unwrap_or("margin-box"));
    match name.as_str() {
        "circle" => resolve_circle(&inner.to_ascii_lowercase(), rect, bx),
        "ellipse" => resolve_ellipse(&inner.to_ascii_lowercase(), rect, bx),
        "inset" => resolve_inset(&inner.to_ascii_lowercase(), rect, bx),
        "polygon" => resolve_polygon(&inner.to_ascii_lowercase(), rect, bx),
        "path" => {
            let pts = parse_shape_path_px(func)?;
            Some(vec![Prim::Polygon(pts.into_iter().map(|(x, y)| (x + rect.x0, y + rect.y0)).collect())])
        }
        "linear-gradient" => resolve_linear_gradient(func, rect, bx),
        _ => None,
    }
}

/// `<position>` (CSS Values L4 §6.4.1, 1–4 tokens) → `(x, y)` from the box's
/// physical top-left. No tokens = `center`.
fn parse_position(toks: &[&str], rect: Rect4, em: f32) -> Option<(f32, f32)> {
    let (w, h) = (rect.w(), rect.h());
    type Item<'a> = (Option<&'a str>, Option<Lp>);
    let is_kw = |t: &str| matches!(t, "left" | "right" | "top" | "bottom" | "center");
    let axis = |k: Option<&str>| match k {
        Some("left" | "right") => 'x',
        Some("top" | "bottom") => 'y',
        _ => 'c',
    };
    // Coordinate along an axis of extent `basis`: `right` / `bottom` count from the far edge.
    let along = |(k, off): Item, basis: f32| -> f32 {
        let o = off.map_or(0.0, |o| o.of(basis));
        match k {
            Some("right" | "bottom") => basis - o,
            Some("center") => basis / 2.0,
            _ => o,
        }
    };
    fn item(t: &str, em: f32) -> Option<Item<'_>> {
        if matches!(t, "left" | "right" | "top" | "bottom" | "center") {
            Some((Some(t), None))
        } else {
            Lp::parse(t, em).map(|lp| (None, Some(lp)))
        }
    }
    let (x, y) = match toks {
        [] => (w / 2.0, h / 2.0),
        [a] => {
            let a = item(a, em)?;
            if axis(a.0) == 'y' { (w / 2.0, along(a, h)) } else { (along(a, w), h / 2.0) }
        }
        [a, b] => {
            let (a, b) = (item(a, em)?, item(b, em)?);
            let (first, second) = if axis(a.0) == 'y' || axis(b.0) == 'x' { (b, a) } else { (a, b) };
            if axis(first.0) == 'y' || axis(second.0) == 'x' {
                return None;
            }
            (along(first, w), along(second, h))
        }
        _ => {
            // `<keyword> <offset>?` pairs: `left 40px top 40px`, `left 10px top`, `center top 10px`.
            let mut pairs: Vec<Item> = Vec::new();
            let mut it = toks.iter().peekable();
            while let Some(&t) = it.next() {
                if !is_kw(t) {
                    return None;
                }
                let off = match it.peek() {
                    Some(n) if t != "center" && !is_kw(n) => Lp::parse(it.next()?, em),
                    _ => None,
                };
                pairs.push((Some(t), off));
            }
            let [p, q] = pairs.as_slice() else { return None };
            let (mut ap, mut aq) = (axis(p.0), axis(q.0));
            if ap == 'c' {
                ap = if aq == 'x' { 'y' } else { 'x' };
            }
            if aq == 'c' {
                aq = if ap == 'x' { 'y' } else { 'x' };
            }
            match (ap, aq) {
                ('x', 'y') => (along(*p, w), along(*q, h)),
                ('y', 'x') => (along(*q, w), along(*p, h)),
                _ => return None,
            }
        }
    };
    Some((rect.x0 + x, rect.y0 + y))
}

/// Distances from `(cx, cy)` to the left, right, top and bottom sides of `r`.
fn side_distances(cx: f32, cy: f32, r: Rect4) -> [f32; 4] {
    [(cx - r.x0).abs(), (r.x1 - cx).abs(), (cy - r.y0).abs(), (r.y1 - cy).abs()]
}

/// `[<radius>]? [at <position>]?` split into its two token lists.
fn split_at_keyword<'a, 'b>(toks: &'b [&'a str]) -> (&'b [&'a str], &'b [&'a str], bool) {
    match toks.iter().position(|t| *t == "at") {
        Some(i) => (&toks[..i], &toks[i + 1..], true),
        None => (toks, &[], false),
    }
}

fn resolve_circle(inner: &str, rect: Rect4, bx: &FloatBoxes) -> Option<Vec<Prim>> {
    let toks = split_ws(inner);
    let (rad, pos, has_at) = split_at_keyword(&toks);
    if rad.len() > 1 || (has_at && pos.is_empty()) {
        return None;
    }
    let (cx, cy) = parse_position(pos, rect, bx.em)?;
    let d = side_distances(cx, cy, rect);
    let r = match rad.first().copied() {
        None | Some("closest-side") => d.iter().copied().fold(f32::INFINITY, f32::min),
        Some("farthest-side") => d.iter().copied().fold(0.0, f32::max),
        Some(t) => Lp::parse(t, bx.em)?.of(((rect.w() * rect.w() + rect.h() * rect.h()) / 2.0).sqrt()),
    };
    if r < 0.0 {
        return None;
    }
    Some(if r > 0.0 { vec![Prim::Ellipse { cx, cy, rx: r, ry: r }] } else { Vec::new() })
}

fn resolve_ellipse(inner: &str, rect: Rect4, bx: &FloatBoxes) -> Option<Vec<Prim>> {
    let toks = split_ws(inner);
    let (rad, pos, has_at) = split_at_keyword(&toks);
    if !matches!(rad.len(), 0 | 2) || (has_at && pos.is_empty()) {
        return None;
    }
    let (cx, cy) = parse_position(pos, rect, bx.em)?;
    let d = side_distances(cx, cy, rect);
    let radius = |t: Option<&&str>, near: f32, far: f32, basis: f32| -> Option<f32> {
        match t.copied() {
            None | Some("closest-side") => Some(near),
            Some("farthest-side") => Some(far),
            Some(t) => Some(Lp::parse(t, bx.em)?.of(basis)),
        }
    };
    let rx = radius(rad.first(), d[0].min(d[1]), d[0].max(d[1]), rect.w())?;
    let ry = radius(rad.get(1), d[2].min(d[3]), d[2].max(d[3]), rect.h())?;
    if rx < 0.0 || ry < 0.0 {
        return None;
    }
    Some(if rx > 0.0 && ry > 0.0 { vec![Prim::Ellipse { cx, cy, rx, ry }] } else { Vec::new() })
}

/// Expands the 1–4 value shorthand `[top, right, bottom, left]`
/// (or `[top-left, top-right, bottom-right, bottom-left]` for radii).
fn expand4<T: Copy>(v: &[T]) -> Option<[T; 4]> {
    match *v {
        [a] => Some([a, a, a, a]),
        [a, b] => Some([a, b, a, b]),
        [a, b, c] => Some([a, b, c, b]),
        [a, b, c, d] => Some([a, b, c, d]),
        _ => None,
    }
}

/// `<length-percentage>{1,4} [ / <length-percentage>{1,4} ]?` → corner radii
/// `[tl, tr, br, bl]` for a `w`×`h` box (CSS Backgrounds L3 §5.5).
fn parse_corner_radii(toks: &[&str], w: f32, h: f32, em: f32) -> Option<[(f32, f32); 4]> {
    let (hs, vs) = match toks.iter().position(|t| *t == "/") {
        Some(i) => (&toks[..i], Some(&toks[i + 1..])),
        None => (toks, None),
    };
    let lps = |ts: &[&str]| -> Option<[Lp; 4]> {
        let v: Option<Vec<Lp>> = ts.iter().map(|t| Lp::parse(t, em)).collect();
        expand4(&v?)
    };
    let hx = lps(hs)?;
    let vy = match vs {
        Some(v) => lps(v)?,
        None => hx,
    };
    let mut out = [(0.0, 0.0); 4];
    for i in 0..4 {
        let (rx, ry) = (hx[i].of(w).max(0.0), vy[i].of(h).max(0.0));
        out[i] = if rx > 0.0 && ry > 0.0 { (rx, ry) } else { (0.0, 0.0) };
    }
    Some(out)
}

fn resolve_inset(inner: &str, rect: Rect4, bx: &FloatBoxes) -> Option<Vec<Prim>> {
    let spaced = inner.replace('/', " / ");
    let toks = split_ws(&spaced);
    let (edges, round) = match toks.iter().position(|t| *t == "round") {
        Some(i) => (&toks[..i], Some(&toks[i + 1..])),
        None => (&toks[..], None),
    };
    let lps: Option<Vec<Lp>> = edges.iter().map(|t| Lp::parse(t, bx.em)).collect();
    let [t, r, b, l] = expand4(&lps?)?;
    let inner_rect = Rect4 {
        x0: rect.x0 + l.of(rect.w()),
        y0: rect.y0 + t.of(rect.h()),
        x1: rect.x1 - r.of(rect.w()),
        y1: rect.y1 - b.of(rect.h()),
    };
    if inner_rect.x1 <= inner_rect.x0 || inner_rect.y1 <= inner_rect.y0 {
        return Some(Vec::new());
    }
    let mut radii = match round {
        Some(rt) => parse_corner_radii(rt, inner_rect.w(), inner_rect.h(), bx.em)?,
        None => [(0.0, 0.0); 4],
    };
    clamp_radii(&mut radii, inner_rect.w(), inner_rect.h());
    Some(vec![Prim::RRect { x0: inner_rect.x0, y0: inner_rect.y0, x1: inner_rect.x1, y1: inner_rect.y1, radii }])
}

fn resolve_polygon(inner: &str, rect: Rect4, bx: &FloatBoxes) -> Option<Vec<Prim>> {
    let mut items = split_commas(inner);
    if let Some(first) = items.first()
        && matches!(first.trim(), "nonzero" | "evenodd")
    {
        items.remove(0);
    }
    let mut pts = Vec::with_capacity(items.len());
    for item in items {
        let toks = split_ws(item.trim());
        let [a, b] = toks.as_slice() else { return None };
        pts.push((rect.x0 + Lp::parse(a, bx.em)?.of(rect.w()), rect.y0 + Lp::parse(b, bx.em)?.of(rect.h())));
    }
    Some(if pts.len() >= 3 { vec![Prim::Polygon(pts)] } else { Vec::new() })
}

/// Clips polygon `pts` to the half-plane where `f(p) ≥ bound` (`keep_above`) or
/// `f(p) ≤ bound` (Sutherland–Hodgman).
fn clip_half_plane(pts: &[(f32, f32)], f: &dyn Fn((f32, f32)) -> f32, bound: f32, keep_above: bool) -> Vec<(f32, f32)> {
    let g = |p: (f32, f32)| if keep_above { f(p) - bound } else { bound - f(p) };
    let mut out = Vec::new();
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let (ga, gb) = (g(a), g(b));
        if ga >= 0.0 {
            out.push(a);
        }
        if (ga >= 0.0) != (gb >= 0.0) {
            let t = ga / (ga - gb);
            out.push((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
        }
    }
    out
}

/// CSS Shapes L1 §3.3 — a `linear-gradient()` shape: the part of the reference box
/// whose gradient colour is more opaque than `shape-image-threshold`, as one convex
/// polygon per opaque interval along the gradient line.
fn resolve_linear_gradient(func: &str, rect: Rect4, bx: &FloatBoxes) -> Option<Vec<Prim>> {
    let ParsedGradient::Linear { angle_deg, corner, stops, repeating: false } =
        crate::style::parse_background_gradient(func)
    else {
        return None;
    };
    if stops.is_empty() {
        return None;
    }
    let (w, h) = (rect.w(), rect.h());
    let ang = corner.map_or(angle_deg, |c| c.angle_deg(w, h)).to_radians();
    let (dx, dy) = (ang.sin(), -ang.cos());
    let len = (w * ang.sin()).abs() + (h * ang.cos()).abs();
    if len <= 0.0 {
        return Some(Vec::new());
    }
    // Stop positions along the gradient line, px (CSS Images L3 §3.4.3).
    let mut pos: Vec<Option<f32>> = Vec::with_capacity(stops.len());
    for st in &stops {
        pos.push(match &st.position {
            None => None,
            Some(Length::Percent(p)) => Some(p / 100.0 * len),
            Some(Length::Px(v)) => Some(*v),
            Some(Length::Em(v)) => Some(v * bx.em),
            Some(_) => return None,
        });
    }
    let last = pos.len() - 1;
    pos[0].get_or_insert(0.0);
    pos[last].get_or_insert(len);
    let mut i = 1;
    while i < pos.len() {
        if pos[i].is_some() {
            i += 1;
            continue;
        }
        let mut k = i;
        while pos[k].is_none() {
            k += 1;
        }
        let (from, to) = (pos[i - 1].unwrap_or(0.0), pos[k].unwrap_or(len));
        for (n, slot) in (i..k).enumerate() {
            pos[slot] = Some(from + (to - from) * (n + 1) as f32 / (k - i + 1) as f32);
        }
        i = k;
    }
    let mut at: Vec<f32> = pos.iter().map(|p| p.unwrap_or(0.0)).collect();
    for n in 1..at.len() {
        at[n] = at[n].max(at[n - 1]);
    }
    let alpha: Vec<f32> = stops.iter().map(|s| f32::from(s.color.a) / 255.0).collect();
    let thr = bx.threshold.clamp(0.0, 1.0);

    // Opaque intervals of the gradient-line coordinate.
    const BIG: f32 = 1.0e6;
    let mut pieces: Vec<(f32, f32, f32, f32)> = vec![(-BIG, at[0], alpha[0], alpha[0])];
    for n in 0..at.len() - 1 {
        pieces.push((at[n], at[n + 1], alpha[n], alpha[n + 1]));
    }
    pieces.push((at[last], BIG, alpha[last], alpha[last]));
    let mut spans: Vec<(f32, f32)> = Vec::new();
    for (p0, p1, a0, a1) in pieces {
        let (in0, in1) = (a0 > thr, a1 > thr);
        let cross = || p0 + (thr - a0) / (a1 - a0) * (p1 - p0);
        match (in0, in1) {
            (true, true) => spans.push((p0, p1)),
            (true, false) => spans.push((p0, cross())),
            (false, true) => spans.push((cross(), p1)),
            (false, false) => {}
        }
    }
    spans.retain(|(lo, hi)| hi - lo > 1.0e-4);
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for (lo, hi) in spans {
        match merged.last_mut() {
            Some(m) if lo <= m.1 + 1.0e-3 => m.1 = m.1.max(hi),
            _ => merged.push((lo, hi)),
        }
    }

    let (ccx, ccy) = ((rect.x0 + rect.x1) / 2.0, (rect.y0 + rect.y1) / 2.0);
    let along = |p: (f32, f32)| (p.0 - ccx) * dx + (p.1 - ccy) * dy + len / 2.0;
    let boxpoly = [(rect.x0, rect.y0), (rect.x1, rect.y0), (rect.x1, rect.y1), (rect.x0, rect.y1)];
    let mut out = Vec::new();
    for (lo, hi) in merged {
        let mut poly = boxpoly.to_vec();
        if lo > -BIG / 2.0 {
            poly = clip_half_plane(&poly, &along, lo, true);
        }
        if hi < BIG / 2.0 {
            poly = clip_half_plane(&poly, &along, hi, false);
        }
        if poly.len() >= 3 {
            out.push(Prim::Polygon(poly));
        }
    }
    Some(out)
}
