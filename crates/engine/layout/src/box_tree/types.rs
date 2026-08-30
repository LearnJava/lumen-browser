use super::*;

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node: NodeId,
    /// Border-box rectangle: (x, y) is the top-left corner after margin,
    /// (width, height) includes padding + border but NOT margin.
    pub rect: Rect,
    /// Computed style of this box, shared with the cascade cache
    /// (`CounterMap::styles`) and with every previous-frame tree that still
    /// holds it, behind copy-on-write.
    ///
    /// BUG-341 S12: this used to be an owned `ComputedStyle` — a 3.2 KB,
    /// 302-field struct with ~30 heap-allocated fields. Every box therefore
    /// paid a deep copy in `build_box` (from the cascade cache), a second one
    /// in `lay_out_inner` (which snapshots the style to dodge the borrow
    /// checker), and a third in every whole-tree `clone()` the incremental
    /// pipeline does per frame to persist `prev`. Measured on `CC12_HOVER`:
    /// 1.2 ms of `lay_out`'s 3.7 ms, plus 1.5 ms of per-cycle bookkeeping.
    /// Behind an `Arc` all three become refcount bumps, and the handful of
    /// passes that genuinely rewrite a used value (`font-size-adjust`, flex
    /// item stretch, container queries) take the copy via
    /// [`std::sync::Arc::make_mut`] on exactly the boxes they touch.
    ///
    /// Reads are unchanged: `Arc` derefs to `ComputedStyle`, so `b.style.field`
    /// and `&b.style` (coerced to `&ComputedStyle`) both still work.
    pub style: Arc<ComputedStyle>,
    pub kind: BoxKind,
    pub children: Vec<LayoutBox>,
    /// HTML `colspan` attribute (table cells only). Number of columns this cell spans.
    /// Always ≥ 1; defaults to 1 for non-table-cell boxes.
    pub col_span: u32,
    /// HTML `rowspan` attribute (table cells only). Number of rows this cell spans.
    /// Always ≥ 1; defaults to 1 for non-table-cell boxes.
    pub row_span: u32,
    /// SVG `transform` attribute for `<g>` groups (Phase 2: nested transforms).
    /// Only used for Block boxes that represent SVG groups; None for all other boxes.
    pub svg_group_transform: Option<SvgTransform>,
    /// Horizontal scroll offset in CSS px for `overflow: scroll` / `overflow: auto`
    /// containers. Updated by shell on wheel/touch events via `set_scroll_position()`.
    /// Zero for non-scrollable boxes.
    pub scroll_x: f32,
    /// Vertical scroll offset in CSS px. Same semantics as `scroll_x`.
    pub scroll_y: f32,
    /// Incremental-layout dirty flags (EE-3). Only consulted during
    /// `lay_out_incremental` passes — normal `lay_out` ignores this field.
    /// Set via `mark_dirty`; cleared via `clear_dirty` / `lay_out_incremental`.
    pub dirty: crate::incremental::DirtyBits,
    /// Provenance for introspection (ADR-025 §1): where this box came from,
    /// distinct from `node` above. `node` stays the hot-path "whose style
    /// applies here" answer and is never `None`; `origin` is what
    /// `explain_element`/`ProvenanceIndex` read and correctly says "no DOM
    /// origin" instead of aliasing the document root.
    pub origin: BoxOrigin,
}

/// Where a layout box came from — the identity of a box for all
/// introspection purposes (ADR-025 §1). Replaces the `NodeId::from_index(0)`
/// "no DOM origin" sentinel, which collided with the document root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxOrigin {
    /// The DOM node this box belongs to, or `None` for boxes with no DOM
    /// origin (anonymous boxes, generated content). Never a sentinel value —
    /// use `None`, not `NodeId::from_index(0)`.
    pub node: Option<NodeId>,
    /// Why this box exists — disambiguates the many boxes one node can
    /// produce (an element's principal box vs. an anonymous wrapper around
    /// its inline children, for example).
    pub role: BoxRole,
}

impl Default for BoxOrigin {
    /// `node: None` + `BoxRole::Element` — used only as a placeholder for
    /// construction sites that predate provenance tracking (test fixtures,
    /// benchmark scaffolding). Production box constructors always set both
    /// fields explicitly instead of relying on this default.
    fn default() -> Self {
        BoxOrigin { node: None, role: BoxRole::Element }
    }
}

/// Disambiguates the many boxes one DOM node — or no node at all — can
/// produce (ADR-025 §1). Paired with `BoxOrigin::node` as the identity of a
/// box; `role` alone or `node` alone is never enough (an anonymous wrapper
/// must never be reported as its parent element).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxRole {
    /// The principal box of an element.
    Element,
    /// Anonymous block-level wrapper (CSS 2.1 §9.2.1.1) or other
    /// block-level box synthesised with no element of its own — table
    /// fixup boxes, `appearance: base-select` scaffolding, drop-cap float
    /// wrappers. `node` is the *containing* element; this role is what makes
    /// the wrapper distinguishable from it.
    AnonymousBlock,
    /// Anonymous inline-level wrapper — `anon_inline_run`,
    /// `anon_inline_block_row`, collapsed inline-block whitespace gaps.
    /// `node` is the containing element or the source text node.
    AnonymousInlineRun,
    /// Pseudo-element box or segment (`::before`, `::after`,
    /// `::first-letter`, `::first-line`, `::marker`'s content run).
    Pseudo(PseudoKind),
    /// List marker box (`::marker`'s own box, CSS Lists L3 §3).
    ListMarker,
    /// `content:` generated content with no DOM text/image node of its own.
    GeneratedContent,
    /// Scaffolding box with no rendered-page meaning at all — pre-navigation
    /// placeholders and benchmark harnesses that need a `LayoutBox` value
    /// before any real layout has run.
    Placeholder,
}

/// Отрезок inline-контента с собственным стилем (до layout).
#[derive(Debug, Clone)]
pub struct InlineSegment {
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved px space before this segment's first word:
    /// margin_left + border_left_width + padding_left of the inline element.
    pub pre_space: f32,
    /// Resolved px space after this segment's last word:
    /// padding_right + border_right_width + margin_right of the inline element.
    pub post_space: f32,
    /// True when this segment comes from inside an inline element box
    /// (not anonymous text directly in a block container). Used by the painter
    /// to know whether to draw the element's own background/border.
    pub is_element_box: bool,
    /// Non-None when this segment is an inline-replaced `<img>`. Contains the
    /// resolved image URL. `text` holds the alt attribute.
    pub img_src: Option<String>,
    /// `loading="lazy"` on the inline `<img>` — emit `LazyImageSlot` instead of `DrawImage`.
    pub img_is_lazy: bool,
    /// Pre-computed pixel width for image segments (0.0 for text segments).
    pub img_width: f32,
    /// True when this segment represents a forced line break (CSS §4.1: newline
    /// in white-space: pre / pre-wrap text). `text` is empty in this case.
    pub forced_break: bool,
    /// CSS structural pseudo-element role of this segment.
    /// Split out by `collect_inline_segments` before wrapping.
    /// `apply_first_letter_pseudo` looks up the `::first-letter` rule and overrides
    /// the style of segments where `pseudo_kind == PseudoKind::FirstLetter`.
    pub pseudo_kind: PseudoKind,
    /// DOM text node that produced this segment, for Selection/Range mapping.
    /// `NodeId(0)` (document root) for generated content with no DOM origin.
    pub source_node: NodeId,
    /// UTF-8 byte offset of `text[0]` within the source text node's content.
    /// Always 0 for non-pre text (whole text node → one segment after whitespace
    /// collapsing); non-zero for pre/pre-wrap segments split at `\n`.
    pub source_char_offset: u32,
    /// UAX #9 embedding level of this segment's text (even = left-to-right,
    /// odd = right-to-left). Assigned by [`crate::bidi::resolve`], which splits
    /// a segment wherever the level changes, so the value is uniform across
    /// `text`. `0` until the bidi pass runs (and for paragraphs it skips).
    pub bidi_level: u8,
}

/// Marks an inline segment as the target of a CSS structural pseudo-element.
/// `apply_first_letter_pseudo` applies `::first-letter` styles from this marker
/// without touching layout geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PseudoKind {
    /// Regular content — no pseudo-element style override.
    #[default]
    None,
    /// CSS Pseudo-elements L4 §5.1 — typographic first letter of the block.
    /// Split from the first non-whitespace text node by `collect_inline_segments`.
    /// Applied by `apply_first_letter_pseudo` via
    /// `compute_pseudo_element_style(node, "first-letter")`, which overrides `seg.style`.
    FirstLetter,
    /// `::before` generated content (ADR-025 `BoxRole::Pseudo` tag only — not
    /// produced by `collect_inline_segments`, which has no notion of `::before`
    /// at the segment level).
    Before,
    /// `::after` generated content (`BoxRole::Pseudo` tag only, see `Before`).
    After,
    /// `::first-line` styled box (`BoxRole::Pseudo` tag only — applied by
    /// `split_first_line_boxes`, which works on whole boxes, not segments).
    /// Paint keys the pseudo-element's own background off this role.
    FirstLine,
    /// `::marker` list-marker content (`BoxRole::Pseudo` tag only — markers are
    /// `BoxKind::Marker` boxes, tagged `BoxRole::ListMarker` instead; this
    /// variant exists for the rare case a marker's content is itself an
    /// inline run needing a `PseudoKind`, e.g. future nested-marker content).
    Marker,
}

/// Позиционированный текстовый фрагмент в строке (после layout).
/// `x` — смещение от левого края inline-контейнера до начала ТЕКСТА
/// (после border+padding inline-элемента слева).
/// `width` — ширина текста фрагмента в пикселях.
/// `padding_left` / `padding_right` — разрешённые px padding-а inline-элемента
/// для этого фрагмента (ненулевые только для первого/последнего слова сегмента).
#[derive(Debug, Clone)]
pub struct InlineFrag {
    pub x: f32,
    pub width: f32,
    /// Vertical offset within the line box (CSS vertical-align). Positive = down.
    pub y_offset: f32,
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved padding_left of this frag's inline box start (0 if not a box start).
    pub padding_left: f32,
    /// Resolved padding_right of this frag's inline box end (0 if not a box end).
    pub padding_right: f32,
    /// True when this frag comes from an inline element box (not anonymous text).
    /// Used by the painter to draw element background/border.
    pub is_element_box: bool,
    /// Non-None when this frag represents an inline-replaced `<img>`.
    /// `text` holds the alt attribute; `width` is the rendered pixel width.
    pub img_src: Option<String>,
    /// `loading="lazy"` on the inline `<img>` — emit `LazyImageSlot` instead of `DrawImage`.
    pub img_is_lazy: bool,
    /// True when this fragment lies on the first formatted line of its block container.
    /// Set by `lay_out` after `wrap_inline_run` completes.
    /// `split_first_line_boxes` applies `compute_pseudo_element_style(node, "first-line")`
    /// to the box holding the first-line frags, overriding their style.
    pub is_first_line: bool,
    /// DOM text node that produced this fragment (for Selection/Range mapping).
    /// Matches the source `InlineSegment::source_node`. `NodeId(0)` for
    /// generated/anonymous content with no direct DOM text node.
    pub source_node: NodeId,
    /// UTF-8 byte offset of `text[0]` within the source text node's content.
    /// Computed in `wrap_inline_run` as words are taken from the segment.
    pub source_char_offset: u32,
    /// UAX #9 embedding level inherited from the source [`InlineSegment`].
    /// `align_lines` feeds it to [`crate::bidi::reorder_line`] for the L2 pass;
    /// paint feeds it to [`crate::bidi::visual_text`], which is what turns an
    /// odd level into right-to-left glyph order.
    pub bidi_level: u8,
}

#[derive(Debug, Clone)]
pub enum BoxKind {
    /// Block-уровневый бокс (элемент или корень документа).
    Block,
    /// Анонимный контейнер для потока inline-контента (текст + inline-элементы).
    /// `segments` — сырые отрезки до lay_out; `lines` — позиционированные строки
    /// после lay_out. Каждая строка — `Vec<InlineFrag>`.
    /// `first_line_style` — pre-computed `::first-line` pseudo-element style for the owning
    /// element. `None` if no rule matches. Applied by `lay_out()` to frags on `lines[0]`.
    InlineRun {
        segments: Vec<InlineSegment>,
        lines: Vec<Vec<InlineFrag>>,
        /// CSS Pseudo-elements L4 §5.3: computed ::first-line style. Set during build_box(),
        /// applied in lay_out() after wrap_inline_run() to first-line frags.
        first_line_style: Option<Box<crate::style::ComputedStyle>>,
    },
    /// Анонимный контейнер для горизонтального потока `display: inline-block`
    /// элементов. Сами дочерние боксы хранятся в `LayoutBox.children`. При
    /// layout дети раскладываются горизонтально слева направо; высота строки
    /// = высота самого высокого дочернего элемента.
    InlineBlockRow,
    /// Replaced element: изображение (`<img>`). Inline-уровневый atomic-бокс
    /// (UA-дефолт `display: inline`, IFC-2): собирается в `InlineBlockRow` и
    /// делит строку с текстом, а на базовую линию садится нижней кромкой margin
    /// box (CSS 2.1 §10.8.1 — `inline_baseline` возвращает для него `None`).
    /// `src` — путь / URL ресурса (декодирование откладывается на следующий
    /// шаг), `alt` — alternate-текст для отображения и AT, размеры берутся из
    /// `style.width`/`style.height` (которые могут происходить из CSS или
    /// HTML-атрибутов как presentational hints).
    Image {
        src: String,
        alt: String,
        /// `loading="lazy"` (HTML LS §lazy-loading): fetch deferred until proximity check.
        /// Display list emits `LazyImageSlot` instead of `DrawImage` when `true`.
        is_lazy: bool,
    },
    /// Replaced element: HTML `<video>` element (HTML spec §14).
    ///
    /// Phase 0: rendered as a grey `DrawImage` placeholder (the video src is
    /// not fetched or decoded). Intrinsic size comes from `width`/`height`
    /// HTML attributes; UA default is 300×150 CSS px (HTML spec §14.1).
    /// `poster` is the optional poster-image URL shown before playback starts.
    Video {
        /// Primary video source URL (`src` attribute).
        src: String,
        /// Poster image URL (`poster` attribute), may be empty.
        poster: String,
    },
    /// Replaced element: HTML `<canvas>` element — CPU-rasterized drawing surface
    /// (HTML Living Standard §4.12.4).
    ///
    /// Phase 0: the pixel buffer is produced by JS Canvas 2D drawing operations
    /// (`canvas.getContext('2d')`) and rendered via a `DrawImage` command keyed by
    /// `canvas:{node_id}`. Intrinsic size comes from the `width`/`height` content
    /// attributes; UA defaults are 300×150 CSS px (HTML LS §4.12.4).
    Canvas {
        /// Canvas bitmap width in CSS pixels (from `width` attribute, default 300).
        width: u32,
        /// Canvas bitmap height in CSS pixels (from `height` attribute, default 150).
        height: u32,
    },
    /// Replaced element: HTML `<audio>` element (HTML spec §4.8.10).
    ///
    /// Phase 0: no audio playback. Without `controls` attribute: 0×0 (invisible).
    /// With `controls` attribute: full-width × 40px grey bar (UA default per spec).
    /// `src` is the primary audio source URL.
    Audio {
        /// Primary audio source URL (`src` attribute), may be empty.
        src: String,
        /// Whether the `controls` attribute is present (shows a 40px control bar).
        controls: bool,
    },
    /// Replaced element: HTML `<iframe>` element (HTML spec §4.8.5).
    ///
    /// Phase 0: rendered as a grey `DrawImage` placeholder (no sub-document
    /// navigation). Intrinsic size comes from `width`/`height` HTML attributes;
    /// UA defaults are 300×150 CSS px (HTML spec §4.8.5). `src` is the URL
    /// to display in paint-side label and in JS `src` property. When `srcdoc`
    /// is `Some`, the inline HTML was parsed via [`build_iframe_document`] and
    /// is available for future Phase 1 sub-document rendering.
    Iframe {
        /// Primary document URL (`src` attribute), may be empty.
        src: String,
        /// Inline HTML content from `srcdoc` attribute (HTML spec §4.8.5).
        /// `None` if the element has no `srcdoc` attribute.
        srcdoc: Option<String>,
    },
    /// Replaced element: HTML form control (`<input>`, `<button>`, `<select>`,
    /// `<textarea>`). Phase 0: block-level replaced. Размеры берутся из
    /// `style.width`/`style.height` (UA defaults из `apply_ua_form_controls`).
    /// `kind` зарезервирован для paint-специализаций в следующих фазах.
    FormControl {
        kind: FormControlKind,
    },
    /// CSS 2.1 §17 — строка таблицы (`display: table-row`). Дочерние
    /// боксы — ячейки (`display: table-cell`), которые раскладываются
    /// горизонтально слева направо. Высота строки = max высота ячейки.
    TableRow,
    /// Схлопнутый межэлементный пробел в InlineBlockRow.
    /// Не рисуется; участвует только как горизонтальный gap между
    /// inline-block соседями (CSS white-space collapsing §4.1.2).
    InlineSpace,
    /// Не участвует в layout (whitespace, комментарий, doctype, display:none).
    Skip,
    /// CSS Lists L3 §2.1 — `::marker` pseudo-element for `display: list-item`.
    /// `text` — marker string for counter types (1., a., i., …); empty for bullet
    /// types (disc/circle/square) which are rendered as geometric shapes.
    /// `position` — inside/outside flow. `list_style_type` — used by the display-list
    /// emitter to choose geometric (disc/circle/square) vs text rendering.
    /// For `outside` (default) positioned left of the principal block, out of flow.
    /// `image` — CSS Lists L3 §2.3 `list-style-image`: resolved URL when set. When
    /// present it replaces the `list_style_type`/`text` marker (the painter emits a
    /// `DrawImage` instead of a bullet/counter). Same URL key used by
    /// `collect_background_image_requests`, so the shell fetches and registers it.
    Marker {
        text: String,
        position: ListStylePosition,
        list_style_type: ListStyleType,
        image: Option<String>,
    },
    /// CSS Display L3 §8 — `display: flow-root`. Establishes a Block Formatting
    /// Context: contains floats, prevents margin escape. Laid out identically to
    /// Block in Phase 0; BFC float-containment wired when float layout is added.
    /// CSS: flow-root
    FlowRoot,
    /// CSS Display L3 §7.2 — `display: contents`. The element itself generates no
    /// box. Children are flattened into the parent's formatting context by
    /// `flatten_contents()` during `build_box`. Must never appear in the final
    /// layout tree that reaches `lay_out`.
    Contents,
    /// CSS 2.1 §17 — table container (`display: table` / `display: inline-table`).
    /// Direct children are `TableRowGroup` or `TableRow` boxes. Layout computes
    /// global column widths across all rows before positioning each row.
    Table,
    /// CSS 2.1 §17 — row group (`display: table-row-group`, `table-header-group`,
    /// `table-footer-group`). Rendered as a transparent wrapper; rows inside are
    /// collected by the parent `Table` box during column-width computation.
    TableRowGroup,
    /// SVG root element (`<svg>`). Acts as a replaced element in CSS flow:
    /// `rect` is its border-box in document coordinates (CSS width × height).
    /// `view_box` maps SVG user-unit space to this rect for shape coordinate transforms.
    /// Children are `SvgShape` and `Block` (for `<g>` groups) boxes.
    /// CSS: width, height (from attributes as presentational hints), fill, stroke — P4 wires.
    SvgRoot {
        /// Parsed `viewBox` attribute. `None` when attribute absent: shapes use 1:1 px mapping.
        view_box: Option<ViewBox>,
        /// Parsed `preserveAspectRatio` attribute for aspect-ratio preservation.
        preserve_aspect_ratio: PreserveAspectRatio,
    },
    /// Individual SVG shape (`<rect>`, `<circle>`, `<ellipse>`, `<line>`, `<path>`).
    /// `LayoutBox.rect` is the bounding box in *document coordinates* (post-viewBox scaling).
    /// `shape` carries the original SVG user-unit geometry for accurate paint-side rendering.
    /// CSS: fill, stroke, stroke-width, opacity — P4 wires via ComputedStyle SVG fields.
    SvgShape {
        /// Geometric primitive in SVG user units (before viewBox scaling).
        shape: SvgShapeKind,
        /// Parsed SVG `transform` presentation attribute (Phase 2: nested transforms).
        /// Composed with parent transforms during layout for accurate positioning.
        /// This is layout *input* — the element's own transform — and must never be
        /// mutated by layout (BUG-262: an inline-block `<svg>` that wraps to a new
        /// line is laid out twice; overwriting this field on the first pass poisoned
        /// the second pass, drifting the shape outside its clip).
        svg_transform: SvgTransform,
        /// Document-space paint matrix `viewport ∘ parent ∘ element`, computed by
        /// layout (`lay_out_svg_element_position`) and consumed by paint as the
        /// canvas CTM for rotate/skew (BUG-244). Layout *output* only; defaults to
        /// identity at construction. Kept separate from `svg_transform` so re-layout
        /// (inline-block wrap, incremental relayout) always recomposes from the
        /// pristine element transform rather than a previous pass's result.
        svg_paint_matrix: SvgTransform,
    },
    /// SVG text element (`<text>`, `<tspan>`, `<textPath>`).
    /// `LayoutBox.rect` is the text bounding box in *document coordinates*.
    /// Text content is measured via `TextMeasurer` and positioned according to SVG text attributes.
    /// CSS: fill, stroke, font-family, font-size — P4 wires via ComputedStyle SVG fields.
    /// // CSS: text-anchor, dominant-baseline, dx, dy
    SvgText {
        /// Text content (concatenated from text nodes within `<text>`, `<tspan>`, `<textPath>`).
        text: String,
        /// SVG `x` attribute in user units (baseline x position). 0.0 if absent.
        x: f32,
        /// SVG `y` attribute in user units (baseline y position). 0.0 if absent.
        y: f32,
        /// SVG `dx` attribute in user units (horizontal offset). 0.0 if absent.
        dx: f32,
        /// SVG `dy` attribute in user units (vertical offset). 0.0 if absent.
        dy: f32,
        /// Text anchor alignment: start/middle/end. Defaults to "start" per SVG spec.
        text_anchor: SvgTextAnchor,
        /// Dominant baseline alignment: auto/baseline/hanging/middle/etc. Defaults to "auto" per SVG spec.
        dominant_baseline: SvgDominantBaseline,
        /// Baseline shift (sub/super/length/percentage). Defaults to `baseline` (no shift) per SVG spec.
        baseline_shift: SvgBaselineShift,
        /// Parsed SVG `transform` presentation attribute.
        svg_transform: SvgTransform,
    },
}
