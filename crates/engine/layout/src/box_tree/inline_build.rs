use super::*;

/// The computed `display` of `id`, as the box-build stage will see it.
///
/// BUG-341 S25. Every caller of this asks the same question — *which formatting
/// context does this child join* — about a node whose style
/// [`precompute_counters`] has already cascaded against this very `inherited`,
/// and which `build_box_inner` will build out of that cached entry regardless
/// of what a probe says. Re-running `compute_style` here therefore did not just
/// cost a second cascade per element child (14 of them on a chrome keystroke,
/// 0.21-0.25 ms of a 0.63 ms cycle): it let the probe and the box disagree.
/// Reading the cache makes the two answers the same one by construction.
///
/// The `compute_style` fallback stays for the genuine misses — a full pass over
/// a node the cascade did not visit, and any caller holding a `CounterMap` that
/// predates the node. It is not a performance path: on chrome it never fires
/// for an element.
pub(crate) fn probe_display(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
) -> Display {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.display_probes += 1;
        s.set(v);
    });
    match counters.style_arc(id) {
        Some(s) => s.display,
        None => {
            note_display_probe(|| compute_style(doc, id, sheet, inherited, viewport, dark_mode))
                .display
        }
    }
}

/// `display` плюс признак «бокс выведен из inline-потока»: CSS 2.1 §9.7 делает
/// плавающий и абсолютно позиционированный бокс блочным независимо от
/// объявленного `display`.
///
/// Отдельная функция, а не второй вызов [`probe_display`]: оба поля читаются из
/// одного и того же `ComputedStyle`, и повторный проход по каскаду на промахе
/// кэша стоил бы ровно столько же, сколько первый.
#[allow(clippy::too_many_arguments)]
fn probe_display_and_flow(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
) -> (Display, bool) {
    BOX_BUILD_STATS.with(|s| {
        let mut v = s.get();
        v.display_probes += 1;
        s.set(v);
    });
    let read = |s: &ComputedStyle| {
        (
            s.display,
            s.float_side != FloatSide::None
                || matches!(s.position, Position::Absolute | Position::Fixed),
        )
    };
    match counters.style_arc(id) {
        Some(s) => read(&s),
        None => read(&note_display_probe(|| {
            compute_style(doc, id, sheet, inherited, viewport, dark_mode)
        })),
    }
}

/// Порождает ли элемент содержимое, которое можно уплощить в `InlineSegment`-ы.
///
/// `<img>` и form controls исключены: это replaced-элементы, у них есть
/// собственная высота, которой у сегмента нет — как сегмент такой элемент
/// схлопывается в высоту строки ([BUG-728]). Они получают собственный бокс
/// (`BoxKind::Image` / `BoxKind::FormControl`), как и всё блочно-уровневое.
///
/// `inline-flex` / `inline-grid` тоже исключены ([BUG-739]): по CSS Display L3
/// §2.1 это **atomic inline-level** боксы — снаружи inline, внутри собственный
/// flex/grid formatting context. Сегмент такого контекста не несёт, поэтому
/// уплощение стоило элементу бокса целиком: ни фона, ни рамки, ни размеров,
/// flex/grid-алгоритм не запускался. Их место — рядом с `inline-block`, в
/// [`is_atomic_inline_level`].
///
/// `display` передаётся отдельно, чтобы вызывающий не считал стиль дважды:
/// [`collect_inline_segments`] к этому месту уже имеет вычисленный
/// `ComputedStyle` узла, а [`is_inline_content`] берёт `display` из кэша.
fn produces_inline_segments(doc: &Document, id: NodeId, display: Display) -> bool {
    if is_image_element(doc, id) || is_form_control_element(doc, id) {
        return false;
    }
    display == Display::Inline
}

/// То же для потомка inline-элемента: `display: contents` дополнительно
/// прозрачен — бокса он не порождает вовсе (CSS Display L3 §3.1), его дети
/// участвуют в inline-контексте родителя напрямую, поэтому уплощать надо
/// сквозь него. Собственный бокс достанется уже его не-inline потомкам.
///
/// На уровне сиблингов блочного контейнера `contents` этой поблажки не имеет
/// ([`is_inline_content`]) — там он и до [BUG-728] получал отдельный бокс.
fn produces_inline_segments_nested(doc: &Document, id: NodeId, display: Display) -> bool {
    if display == Display::Contents {
        // На replaced-элементе `contents` вычисляется в `inline` (§3.1), то
        // есть бокс у него остаётся — и высота, ради которой всё это.
        return !is_image_element(doc, id) && !is_form_control_element(doc, id);
    }
    produces_inline_segments(doc, id, display)
}

/// `<img>` — не inline-**контент**, хотя и inline-уровневый: он порождает
/// собственный `BoxKind::Image` вместо того, чтобы влиться в `InlineRun`
/// сегментом (у сегмента нет своей высоты — BUG-728). В строку он попадает
/// через [`is_atomic_inline_level`], как `inline-block` и form controls.
#[allow(clippy::too_many_arguments)]
pub(crate) fn is_inline_content(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
) -> bool {
    match &doc.get(id).data {
        // Control-only text (after BUG-120 stripping) is no more inline content
        // than whitespace-only text: it must not open an inline run / line box.
        NodeData::Text(s) => !s.chars().all(|c| c.is_whitespace() || is_invisible_control(c)),
        NodeData::Element { .. } => {
            if is_image_element(doc, id) || is_form_control_element(doc, id) {
                return false;
            }
            produces_inline_segments(
                doc,
                id,
                probe_display(doc, sheet, id, inherited, viewport, dark_mode, counters),
            )
        }
        _ => false,
    }
}

/// Является ли DOM-узел **atomic inline-level** элементом — таким, который
/// снаружи участвует в inline-контексте одним неделимым боксом, а внутри
/// заводит собственный formatting context (CSS Display L3 §2.1):
/// `inline-block`, `inline-flex`, `inline-grid`.
///
/// Все три собираются в `InlineBlockRow` и текут горизонтально рядом с текстом;
/// различает их только внутренний лэйаут, который выбирает `lay_out` по
/// `style.display` (ветки `Display::Flex | Display::InlineFlex` и
/// `Display::Grid | Display::InlineGrid`). До [BUG-739] `inline-flex`/
/// `inline-grid` не попадали сюда и уплощались в сегменты родителя, то есть
/// не получали бокса вовсе.
///
/// Form controls (`<input>`/`<select>`/`<button>`/…) участвуют как inline-block,
/// когда их computed `display` == InlineBlock (UA-дефолт из `default_display`):
/// их replaced/виджет-бокс (`BoxKind::FormControl`) собирается в
/// `InlineBlockRow` и течёт горизонтально рядом с текстом и соседними
/// контролами. Author `display:block` поверх → обычный block-бокс (эта функция
/// вернёт false).
///
/// `<img>` (IFC-2) — четвёртый случай: у него UA-дефолт `display: inline`, но
/// как replaced-элемент он неделим, поэтому inline-level он именно **atomic**
/// (CSS Display L3 §2.1), а не источник сегментов ([`produces_inline_segments`]
/// возвращает для него false). Поэтому у картинки принимается и `Inline`.
///
/// Плавающая или абсолютно позиционированная картинка сюда НЕ попадает: CSS 2.1
/// §9.7 выводит такой бокс из inline-потока и делает блочным независимо от
/// `display`, а обтекание умеет только блочная ветка `lay_out`. До IFC-2
/// `<img>` был блочным всегда, поэтому обтекание у него работало — сузить его
/// молча значило бы разменять одну раскладку на другую. Тот же случай у
/// плавающего `inline-block` разбирается по-старому (он и до IFC-2 собирался в
/// ряд, теряя float) — это отдельный дефект, здесь не трогается.
#[allow(clippy::too_many_arguments)]
pub(crate) fn is_atomic_inline_level(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
) -> bool {
    if !matches!(&doc.get(id).data, NodeData::Element { .. }) {
        return false;
    }
    if is_image_element(doc, id) {
        let (display, out_of_flow) =
            probe_display_and_flow(doc, sheet, id, inherited, viewport, dark_mode, counters);
        return !out_of_flow
            && matches!(
                display,
                Display::Inline
                    | Display::InlineBlock
                    | Display::InlineFlex
                    | Display::InlineGrid
            );
    }
    matches!(
        probe_display(doc, sheet, id, inherited, viewport, dark_mode, counters),
        Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
    )
}

/// Обнуляет box-model spacing анонимного контейнера (InlineRun / InlineBlockRow).
// Anonymous boxes inherit only inheritable properties from their parent; every
// non-inherited property takes its initial value (CSS 2.1 §9.2.2.1). Cloning the
// parent style and resetting the non-inherited longhands below approximates that.
// `float`, `clear` and `position` are non-inherited (CSS 2.1 §9.5.1/§9.5.2, CSS
// Positioned Layout L3 §3): an anonymous box must NOT float or be positioned
// (BUG-152 — an anonymous InlineRun cloning a floated parent's `float_side`
// re-entered the float branch of its own parent's layout loop, so `child_y` never
// advanced and the run overlapped the following block siblings).
pub(crate) fn anon_style(parent: &ComputedStyle) -> ComputedStyle {
    let mut s = parent.clone();
    s.float_side = FloatSide::None;
    s.clear = ClearSide::None;
    s.position = Position::Static;
    s.margin_top = LengthOrAuto::ZERO;
    s.margin_right = LengthOrAuto::ZERO;
    s.margin_bottom = LengthOrAuto::ZERO;
    s.margin_left = LengthOrAuto::ZERO;
    s.padding_top = Length::Px(0.0);
    s.padding_right = Length::Px(0.0);
    s.padding_bottom = Length::Px(0.0);
    s.padding_left = Length::Px(0.0);
    s.background_color = None;
    s.width = None;
    s.height = None;
    s.min_width = None;
    s.max_width = None;
    s.min_height = None;
    s.max_height = None;
    s.border_top_width = 0.0;
    s.border_right_width = 0.0;
    s.border_bottom_width = 0.0;
    s.border_left_width = 0.0;
    s.box_sizing = BoxSizing::ContentBox;
    s
}

/// `role` disambiguates the many different reasons callers wrap segments in an
/// anonymous inline run — a blockified flex/grid text item, a whitespace-flush
/// gap, or `::before`/`::after` generated content — per ADR-025 §1.
pub(crate) fn anon_inline_run(
    node: NodeId,
    parent: &ComputedStyle,
    segs: Vec<InlineSegment>,
    role: BoxRole,
) -> LayoutBox {
    LayoutBox {
        node,
        rect: Rect::ZERO,
        style: Arc::new(anon_style(parent)),
        kind: BoxKind::InlineRun { segments: segs, lines: vec![], first_line_style: None },
        children: vec![],
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(node), role },
    }
}

/// CSS Flexbox §4 / Grid §6: a contiguous run of text directly inside a flex or
/// grid container is wrapped in an anonymous (blockified) item. Returns `None`
/// for a whitespace/control-only run — such runs do not generate an item.
///
/// The item is an anonymous `Block` container (so its inline content formats into
/// line boxes like any block) holding a single `InlineRun` with the text. Without
/// this, the text node's box is `Skip` and the text vanishes — BUG-194: white
/// digit labels inside `.item { display: flex }` were dropped entirely.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_anon_text_item(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    parent: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
) -> Option<LayoutBox> {
    let NodeData::Text(s) = &doc.get(id).data else {
        return None;
    };
    if s.chars().all(|c| c.is_whitespace() || is_invisible_control(c)) {
        return None;
    }
    let mut segs = Vec::new();
    // Each anonymous text item is its own inline context — ::first-letter does not
    // apply to anonymous flex/grid items, so disable the candidate flag.
    let mut need_first_letter = false;
    // `id` — текстовый узел, у него нет потомков: escape-ов быть не может.
    let mut escapes = Vec::new();
    collect_inline_segments(
        doc, sheet, id, parent, viewport, &mut segs, &mut escapes, flat, counters, registry,
        &mut need_first_letter, dark_mode,
    );
    if segs.is_empty() {
        return None;
    }
    let run = anon_inline_run(id, parent, segs, BoxRole::AnonymousInlineRun);
    let mut item_style = anon_style(parent);
    // The anonymous item is blockified regardless of the container's own display.
    item_style.display = Display::Block;
    Some(LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style: Arc::new(item_style),
        kind: BoxKind::Block,
        children: vec![run],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: BoxOrigin { node: Some(id), role: BoxRole::AnonymousBlock },
    })
}

/// CSS Pseudo-elements L4 §5.4: applies `::first-letter` style to the first grapheme of the
/// `FirstLetter`-marked segment. Splits the segment if it contains more than one character so
/// only the first grapheme gets the pseudo-element style; the remainder keeps the original style.
/// No-op when no `FirstLetter` segment exists or no matching `::first-letter` rule is found.
fn apply_first_letter_pseudo(
    segs: &mut Vec<InlineSegment>,
    doc: &lumen_dom::Document,
    node: lumen_dom::NodeId,
    sheet: &lumen_css_parser::Stylesheet,
    parent: &crate::style::ComputedStyle,
    viewport: lumen_core::geom::Size,
    dark_mode: bool,
) {
    let Some(pos) = segs.iter().position(|s| s.pseudo_kind == PseudoKind::FirstLetter) else {
        return;
    };
    let Some(fl_style) = crate::style::compute_pseudo_element_style(
        doc, node, "first-letter", sheet, parent, viewport, dark_mode,
    ) else {
        return;
    };
    // CSS Pseudo-elements L4 §5.1: leading punctuation + first letter. Char-level
    // boundary (full grapheme cluster support requires unicode-segmentation,
    // which is not yet a dependency).
    let first_char_end = first_letter_text_len(&segs[pos].text);
    if first_char_end == 0 {
        return;
    }
    if first_char_end >= segs[pos].text.len() {
        // Single-character segment: layer the pseudo style on in place.
        segs[pos].style = crate::style::merge_pseudo_inherited(&segs[pos].style, parent, &fl_style);
        return;
    }
    // Multi-character: split into [first_char | rest], each with its own style.
    let rest_text = segs[pos].text[first_char_end..].to_string();
    let original_style = segs[pos].style.clone();
    let source_node = segs[pos].source_node;
    let post_space = segs[pos].post_space;
    segs[pos].text.truncate(first_char_end);
    segs[pos].style = crate::style::merge_pseudo_inherited(&original_style, parent, &fl_style);
    segs[pos].post_space = 0.0;
    segs.insert(pos + 1, InlineSegment {
        text: rest_text,
        style: original_style,
        pre_space: 0.0,
        post_space,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node,
        source_char_offset: first_char_end as u32,
        bidi_level: 0,
    });
}

/// Собирает поток inline-контента блочного контейнера в элементы будущего ряда,
/// разрезая его на местах [`InlineEscape`] (CSS 2.1 §9.2.1.1, [BUG-728]).
///
/// Сегменты между двумя escape-ами становятся отдельным `InlineRun`, каждый
/// escape — собственным боксом ровно на своём месте потока. `::first-letter`
/// применяется к каждому куску отдельно: маркер `PseudoKind::FirstLetter` стоит
/// ровно на одном сегменте, поэтому для остальных кусков это no-op — так
/// индексы escape-ов не сбиваются вставкой сегмента-остатка.
#[allow(clippy::too_many_arguments)]
pub(crate) fn split_inline_pieces(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    style: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
    prev_index: Option<&crate::incremental::ReuseIndex>,
    segs: Vec<InlineSegment>,
    escapes: Vec<InlineEscape>,
    out_items: &mut Vec<LayoutBox>,
) {
    let push_run = |chunk: Vec<InlineSegment>, out_items: &mut Vec<LayoutBox>| {
        if chunk.is_empty() {
            return;
        }
        let mut chunk = chunk;
        apply_first_letter_pseudo(&mut chunk, doc, id, sheet, style, viewport, dark_mode);
        out_items.push(anon_inline_run(id, style, chunk, BoxRole::AnonymousInlineRun));
    };
    let mut rest = segs;
    // Escape-ы приходят в порядке обхода, их `at` не убывает; идём с конца,
    // чтобы отрезать хвост `split_off`-ом без сдвигов уже отданных индексов.
    let mut tails: Vec<(Vec<InlineSegment>, NodeId, ComputedStyle)> = Vec::new();
    for esc in escapes.into_iter().rev() {
        let at = esc.at.min(rest.len());
        tails.push((rest.split_off(at), esc.node, esc.inherited));
    }
    push_run(std::mem::take(&mut rest), out_items);
    for (tail, node, inherited) in tails.into_iter().rev() {
        let child = build_box_or_reuse(
            doc, sheet, node, &inherited, viewport, flat, counters, registry, dark_mode, prev_index,
        );
        if !matches!(child.kind, BoxKind::Skip) {
            out_items.push(child);
        }
        push_run(tail, out_items);
    }
}

/// CSS Pseudo-elements L4 §5.3: `::first-line` относится к первой строке блока,
/// то есть к первому `InlineRun` его inline-контекста. Один сброс потока может
/// дать несколько прогонов (разрезы по [`InlineEscape`]), поэтому стиль ищет
/// первый подходящий бокс среди только что добавленных и взводит `assigned`,
/// чтобы следующие сбросы его не перетёрли.
pub(crate) fn assign_first_line_style(
    fresh: &mut [LayoutBox],
    first_line_style: &Option<Box<ComputedStyle>>,
    assigned: &mut bool,
) {
    if *assigned {
        return;
    }
    for item in fresh {
        if let BoxKind::InlineRun { first_line_style: ref mut fls, .. } = item.kind {
            *fls = first_line_style.clone();
            *assigned = true;
            return;
        }
    }
}

/// Ширина схлопнутого пробела, которым текстовый прогон граничит с соседом по
/// строке. Повторяет выбор шрифта из [`wrap_inline_run`]: кегль — контейнера,
/// семейство — первого сегмента, иначе прогон и зазор перед ним меряются
/// разными шрифтами (BUG-128).
fn inline_space_width(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> f32 {
    let (Some(m), BoxKind::InlineRun { segments, .. }) = (measurer, &b.kind) else {
        return 0.0;
    };
    let em = b.style.font_size;
    segments.first().map_or_else(
        || m.char_width(' ', em),
        |seg| m.char_width_with_families(' ', em, &seg.style.font_family),
    )
}

/// CSS Text L3 §4.1.1 — схлопнутый пробел, с которого текстовый прогон
/// начинается: `wrap_inline_run` срезает пробел в начале строки, поэтому зазор
/// между предшествующим atomic inline и текстом не записан больше нигде.
///
/// Считается по сегментам, а не по строкам: значение нужно ДО раскладки
/// прогона, чтобы знать, с какого x его класть.
pub(crate) fn inline_run_lead_space(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> f32 {
    let BoxKind::InlineRun { segments, .. } = &b.kind else {
        return 0.0;
    };
    if b.style.white_space.preserves_whitespace() {
        // Пробел сохранён в самом сегменте — он уже внутри фрагментов.
        return 0.0;
    }
    let starts_ws = segments
        .iter()
        .find(|seg| !seg.text.is_empty())
        .is_some_and(|seg| seg.text.starts_with(|c: char| c.is_whitespace()));
    if starts_ws { inline_space_width(b, measurer) } else { 0.0 }
}

/// Насколько текстовый прогон продвигает inline formatting context — ширина его
/// ПОСЛЕДНЕЙ строки плюс схлопнутый пробел, которым он заканчивается.
///
/// Бокс прогона широк ровно настолько, сколько ему предложили, а не настолько,
/// сколько занял текст, поэтому продвигаться по `rect.width` нельзя: следующий
/// atomic inline всегда оказывался бы за правым краем контейнера и переносился
/// на свою строку (IFC-1 — «Aa <span inline-block> Bb» раскладывался тремя
/// строками вместо одной). Важна только последняя строка: все предыдущие
/// закончились мягким переносом, и контент после прогона продолжает именно её.
pub(crate) fn inline_run_advance(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> f32 {
    let BoxKind::InlineRun { segments, lines, .. } = &b.kind else {
        return b.rect.width;
    };
    let Some(last) = lines.last() else {
        // Раскладки не было (нет measurer) — прежнее поведение.
        return b.rect.width;
    };
    let extent = last
        .iter()
        .map(|f| f.x + f.width)
        .fold(0.0_f32, f32::max);
    let trail = if b.style.white_space.preserves_whitespace() {
        0.0
    } else {
        let ends_ws = segments
            .iter()
            .rev()
            .find(|seg| !seg.text.is_empty())
            .is_some_and(|seg| seg.text.ends_with(|c: char| c.is_whitespace()));
        if ends_ws { inline_space_width(b, measurer) } else { 0.0 }
    };
    extent + trail
}

/// CSS 2.1 §10.8.1 — расстояние от верхней кромки border box до базовой линии,
/// которую бокс предлагает своему inline formatting context. `None` означает,
/// что такой линии нет и выравнивать бокс надо по нижней кромке margin box:
/// замещаемый элемент, пустой `inline-block` или `inline-block` с `overflow`,
/// отличным от `visible`.
pub(crate) fn inline_baseline(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> Option<f32> {
    match &b.kind {
        BoxKind::InlineRun { .. } => {
            let m = measurer?;
            let em = b.style.font_size;
            let line_h = step_line_height(em * b.style.line_height, b.style.line_height_step);
            // Базовая линия — у ПОСЛЕДНЕЙ строки прогона: именно её продолжает
            // контент, стоящий за прогоном. Отсчитывается от высоты бокса, а не
            // как `(n-1) * line_h`, чтобы не разойтись с `::first-line` и
            // `line-height-step`, которые делают строки разновысокими.
            let half_leading = (line_h - (m.ascent_px(em) + m.descent_px(em))) / 2.0;
            Some(b.rect.height - line_h + half_leading + m.ascent_px(em))
        }
        // Базовая линия замещаемого элемента — нижняя кромка margin box.
        BoxKind::Image { .. }
        | BoxKind::Video { .. }
        | BoxKind::Canvas { .. }
        | BoxKind::Audio { .. }
        | BoxKind::Iframe { .. }
        | BoxKind::SvgRoot { .. } => None,
        _ => {
            if b.style.overflow_x != Overflow::Visible || b.style.overflow_y != Overflow::Visible {
                return None;
            }
            if let BoxKind::FormControl { kind } = &b.kind
                && !form_control_has_text_baseline(kind)
            {
                return None;
            }
            if let Some(bl) = last_in_flow_baseline(b, measurer) {
                return Some(bl);
            }
            // HTML §15.5 — у текстового контрола line box своего потока нет
            // (значение рисует виджет), но базовая линия у него есть, и браузеры
            // берут её по тексту, а не по нижней кромке. Строка ставится по
            // центру content box — ровно так, как её рисует
            // `emit_input_value_text`, иначе раскладка и отрисовка разъедутся.
            if matches!(b.kind, BoxKind::FormControl { .. }) {
                let m = measurer?;
                let s = &b.style;
                let em = s.font_size;
                let pt = s.padding_top.resolve_or_zero(em, 0.0, Size::ZERO);
                let pb = s.padding_bottom.resolve_or_zero(em, 0.0, Size::ZERO);
                let inner_h = (b.rect.height
                    - s.border_top_width
                    - s.border_bottom_width
                    - pt
                    - pb)
                    .max(0.0);
                let line_h = step_line_height(em * s.line_height, s.line_height_step);
                let half_leading = (line_h - (m.ascent_px(em) + m.descent_px(em))) / 2.0;
                return Some(
                    s.border_top_width
                        + pt
                        + ((inner_h - line_h) / 2.0).max(0.0)
                        + half_leading
                        + m.ascent_px(em),
                );
            }
            None
        }
    }
}

/// Несёт ли контрол текст, по которому браузер берёт его базовую линию.
///
/// `checkbox`/`radio`/`color`/`file`/`range`/`progress`/`meter` — замещаемые
/// виджеты без текста: их базовая линия — нижняя кромка margin box (CSS 2.1
/// §10.8.1), и синтезировать текстовую линию для них значит поднять контрол над
/// строкой. `<textarea>` тоже выравнивается по нижней кромке (проверено против
/// Edge на TEST-34: `<select>` рядом с ним садится НИЖЕ его нижнего края —
/// значит базовая линия строки идёт по textarea, а не по его первой строке).
fn form_control_has_text_baseline(kind: &FormControlKind) -> bool {
    match kind {
        FormControlKind::Button | FormControlKind::Select { .. } => true,
        FormControlKind::Input { input_type, .. } => matches!(
            input_type,
            lumen_dom::InputType::Text
                | lumen_dom::InputType::Password
                | lumen_dom::InputType::Email
                | lumen_dom::InputType::Tel
                | lumen_dom::InputType::Url
                | lumen_dom::InputType::Number
                | lumen_dom::InputType::Search
                | lumen_dom::InputType::Date
                | lumen_dom::InputType::DateTimeLocal
                | lumen_dom::InputType::Time
                | lumen_dom::InputType::Month
                | lumen_dom::InputType::Week
                | lumen_dom::InputType::Submit
                | lumen_dom::InputType::Reset
                | lumen_dom::InputType::Button
        ),
        FormControlKind::Textarea { .. }
        | FormControlKind::Range { .. }
        | FormControlKind::Progress { .. }
        | FormControlKind::Meter { .. } => false,
    }
}

/// Базовая линия последнего потомка `b`, находящегося в нормальном потоке
/// (CSS 2.1 §10.8.1 — «базовая линия последнего line box в нормальном потоке»),
/// в координатах border box самого `b`.
fn last_in_flow_baseline(b: &LayoutBox, measurer: Option<&dyn TextMeasurer>) -> Option<f32> {
    for c in b.children.iter().rev() {
        if c.style.float_side != FloatSide::None
            || matches!(c.style.position, Position::Absolute | Position::Fixed)
        {
            continue;
        }
        if matches!(
            c.kind,
            BoxKind::Skip | BoxKind::InlineSpace | BoxKind::Marker { .. }
        ) {
            continue;
        }
        if let Some(bl) = inline_baseline(c, measurer) {
            return Some(c.rect.y - b.rect.y + bl);
        }
    }
    None
}

/// `vertical-align` бокса как участника inline-ряда. Анонимный прогон текста
/// всегда выравнивается по базовой линии: свойство не наследуется, а `anon_style`
/// клонирует стиль блока-родителя целиком.
pub(crate) fn inline_v_align(b: &LayoutBox) -> VerticalAlign {
    if matches!(b.kind, BoxKind::InlineRun { .. }) {
        VerticalAlign::Baseline
    } else {
        b.style.vertical_align
    }
}

/// Разрывает ли бокс анонимный inline-ряд: блочно-уровневый потомок, всплывший
/// из inline-элемента, не может делить line box с текстом (CSS 2.1 §9.2.1.1).
/// Анонимные прогоны и пробелы (`BoxRole::AnonymousInlineRun`) наследуют
/// `display` блока-родителя, поэтому по стилю их отличить нельзя — только по роли.
pub(crate) fn breaks_inline_row(b: &LayoutBox) -> bool {
    !matches!(b.origin.role, BoxRole::AnonymousInlineRun)
        && !matches!(
            b.style.display,
            Display::Inline | Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        )
}

pub(crate) fn anon_inline_block_row(node: NodeId, parent: &ComputedStyle, items: Vec<LayoutBox>) -> LayoutBox {
    LayoutBox {
        node,
        rect: Rect::ZERO,
        style: Arc::new(anon_style(parent)),
        kind: BoxKind::InlineBlockRow,
        children: items,
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(node), role: BoxRole::AnonymousInlineRun },
    }
}

/// Inline-сегменты для текста, у которого нет DOM-узла-источника: значение
/// `<textarea>`, набранное пользователем или присвоенное скриптом (BUG-441).
///
/// Повторяет ветки [`collect_inline_segments`] для текстового узла, но берёт
/// строку не из DOM: при `white-space`, сохраняющем переводы строки (UA-стиль
/// `<textarea>` — `pre-wrap`), строка режется по `\n` на сегменты с
/// `forced_break` между ними; иначе отдаётся одним сегментом. `source_node` —
/// сам контрол: собственного текстового узла у значения нет.
pub(crate) fn control_value_segments(
    node: NodeId,
    value_text: &str,
    style: &ComputedStyle,
) -> Vec<InlineSegment> {
    let mut out = Vec::new();
    let mut push = |text: String, forced_break: bool, byte_offset: u32| {
        out.push(InlineSegment {
            text,
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_is_lazy: false,
            img_width: 0.0,
            forced_break,
            pseudo_kind: PseudoKind::None,
            source_node: node,
            source_char_offset: byte_offset,
            bidi_level: 0,
        });
    };
    if !style.white_space.preserves_newlines() {
        let text = style.text_transform.apply(&strip_invisible_controls(value_text));
        if !text.is_empty() {
            push(text, false, 0);
        }
        return out;
    }
    let mut byte_offset: u32 = 0;
    for (i, line) in value_text.split('\n').enumerate() {
        if i > 0 {
            push(String::new(), true, byte_offset);
            byte_offset += 1; // the \n character
        }
        // BUG-120: invisible controls must not occupy advance width.
        let text = style.text_transform.apply(&strip_invisible_controls(line));
        if !text.is_empty() {
            push(text, false, byte_offset);
        }
        byte_offset += line.len() as u32;
    }
    out
}

/// Потомок inline-элемента, который нельзя уплотнить в [`InlineSegment`].
///
/// CSS 2.1 §9.2.1.1: блочно-уровневый потомок разрезает окружающий inline-бокс,
/// а replaced-элемент (`<img>`, form control) обязан сохранить собственную
/// высоту. У сегмента высоты нет вовсе — до [BUG-728] такой потомок уплощался
/// вместе с текстом и схлопывался в высоту строки. Вместо этого
/// [`collect_inline_segments`] откладывает узел сюда, а строитель блочного
/// контейнера собирает ему настоящий бокс и вставляет на то же место потока.
#[derive(Debug, Clone)]
pub(crate) struct InlineEscape {
    /// Сколько сегментов уже собрано к моменту встречи узла: бокс встаёт
    /// ровно после них и перед всеми последующими.
    at: usize,
    /// DOM-узел, которому нужен собственный `LayoutBox`.
    node: NodeId,
    /// Стиль родительского inline-элемента — то, от чего узел наследует.
    /// Блочный контейнер строит бокс далеко от места находки, и его
    /// собственный стиль здесь не подходит: цвет/шрифт `<span>`-а между ними
    /// был бы потерян.
    inherited: ComputedStyle,
}

/// Рекурсивно собирает `InlineSegment`-ы из поддерева inline-контента.
///
/// `need_first_letter` — starts `true` for the first call on a block container; set to `false`
/// once the first non-whitespace text character is split into a `PseudoKind::FirstLetter` segment.
/// Callers must initialize to `true` and pass through all recursive calls within the same run.
/// After collection, `apply_first_letter_pseudo` overrides the `PseudoKind::FirstLetter`
/// segment's style via `compute_pseudo_element_style(node, "first-letter")`.
///
/// `escapes` собирает узлы, которым нужен собственный бокс — см. [`InlineEscape`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_inline_segments(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    out: &mut Vec<InlineSegment>,
    escapes: &mut Vec<InlineEscape>,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    need_first_letter: &mut bool,
    dark_mode: bool,
) {
    match &doc.get(id).data {
        NodeData::Text(s) if inherited.white_space.preserves_whitespace() => {
            // CSS Text L3 §4.1: white-space: pre/pre-wrap — preserve tabs and
            // newlines. Split on \n to produce forced-break segments.
            let style = inherited.clone();
            let mut byte_offset: u32 = 0;
            for (i, line) in s.split('\n').enumerate() {
                if i > 0 {
                    out.push(InlineSegment {
                        text: String::new(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_is_lazy: false,
                        img_width: 0.0,
                        forced_break: true,
                        pseudo_kind: PseudoKind::None,
                        source_node: id,
                        source_char_offset: byte_offset,
                        bidi_level: 0,
                    });
                    byte_offset += 1; // the \n character
                }
                // BUG-120: drop invisible controls (Cc except tab) — they must
                // not occupy advance width even in white-space: pre.
                let text = strip_invisible_controls(line);
                if !text.is_empty() {
                    out.push(InlineSegment {
                        text: text.into_owned(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_is_lazy: false,
                        img_width: 0.0,
                        forced_break: false,
                        pseudo_kind: PseudoKind::None,
                        source_node: id,
                        source_char_offset: byte_offset,
                        bidi_level: 0,
                    });
                }
                byte_offset += line.len() as u32;
            }
        }
        NodeData::Text(s)
            if inherited.white_space.preserves_newlines() && s.contains('\n') =>
        {
            // CSS Text L4 §3.1 preserve-breaks (white-space: pre-line):
            // segment breaks сохраняются как forced line breaks, остальной
            // whitespace схлопывается как в normal (word-split в
            // wrap_inline_run). Сюда попадает только PreLine — режимы с
            // preserves_whitespace() перехвачены веткой выше.
            let style = inherited.clone();
            let mut byte_offset: u32 = 0;
            for (i, line) in s.split('\n').enumerate() {
                if i > 0 {
                    out.push(InlineSegment {
                        text: String::new(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_is_lazy: false,
                        img_width: 0.0,
                        forced_break: true,
                        pseudo_kind: PseudoKind::None,
                        source_node: id,
                        source_char_offset: byte_offset,
                        bidi_level: 0,
                    });
                    byte_offset += 1; // the \n character
                }
                let stripped = strip_invisible_controls(line);
                if !stripped.chars().all(|c| c.is_whitespace()) {
                    let text = inherited.text_transform.apply(&stripped);
                    let kind = if *need_first_letter && !text.trim().is_empty() {
                        *need_first_letter = false;
                        PseudoKind::FirstLetter
                    } else {
                        PseudoKind::None
                    };
                    out.push(InlineSegment {
                        text,
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_is_lazy: false,
                        img_width: 0.0,
                        forced_break: false,
                        pseudo_kind: kind,
                        source_node: id,
                        source_char_offset: byte_offset,
                        bidi_level: 0,
                    });
                }
                byte_offset += line.len() as u32;
            }
        }
        NodeData::Text(s) if !s.chars().all(|c| c.is_whitespace() || is_invisible_control(c)) => {
            // BUG-120: strip invisible controls before transform/measure — Edge
            // renders them zero-advance, they must not contribute glyphs.
            let s = strip_invisible_controls(s);
            // text-transform применяется здесь, до wrapping и paint —
            // measurer считает ширину уже после преобразования.
            let text = inherited.text_transform.apply(&s);
            // CSS Pseudo-elements L4 §5.1: the first text segment in this inline run
            // is the candidate for ::first-letter. Mark the whole first non-whitespace
            // segment; `apply_first_letter_pseudo` later looks up the ::first-letter rule
            // and splits at the character boundary, restyling only the first letter.
            let kind = if *need_first_letter && !text.trim().is_empty() {
                *need_first_letter = false;
                PseudoKind::FirstLetter
            } else {
                PseudoKind::None
            };
            out.push(InlineSegment {
                text,
                style: inherited.clone(),
                pre_space: 0.0,
                post_space: 0.0,
                is_element_box: false,
                img_src: None,
                img_is_lazy: false,
                img_width: 0.0,
                forced_break: false,
                pseudo_kind: kind,
                source_node: id,
                source_char_offset: 0,
                bidi_level: 0,
            });
        }
        NodeData::Text(_) => {
            // CSS Text L3 §4.1.1 — a collapsing whitespace-only text node between
            // inline-level boxes collapses to a single space. We don't emit a
            // segment for it (it would split to zero words); instead we record the
            // collapsible space on the preceding segment by giving its text a
            // trailing space, so `wrap_inline_run` inserts exactly one inter-word
            // gap at that boundary. Without this, adjacent segments would be joined
            // tightly even when source whitespace separated them. Leading
            // whitespace (no preceding segment) collapses away entirely.
            if let Some(last) = out.last_mut()
                && !last.forced_break
                && !last.style.white_space.preserves_whitespace()
                && !last.text.ends_with(|c: char| c.is_whitespace())
            {
                last.text.push(' ');
            }
        }
        NodeData::Element { .. } => {
            let s = compute_style(doc, id, sheet, inherited, viewport, dark_mode);
            if s.display == Display::None {
                return;
            }
            // BUG-728: всё, что не порождает сегментов — блочно-уровневый
            // потомок (CSS 2.1 §9.2.1.1 разрезает вокруг него inline-бокс),
            // `<img>`, form control — уходит вызывающему за собственным боксом.
            // Уплощение в сегмент стоило бы такому потомку высоты: у сегмента
            // её нет, вертикальный размер строки считается по метрикам шрифта,
            // и `<img width=50 height=50>` внутри `<a>` рисовался 50×16.8.
            if !produces_inline_segments_nested(doc, id, s.display) {
                escapes.push(InlineEscape { at: out.len(), node: id, inherited: inherited.clone() });
                return;
            }
            // Compute horizontal inline box model: margin + border + padding.
            // Use em=font_size, cb=0 (% padding on inline elements is uncommon).
            let em = s.font_size;
            let pre = s.margin_left.resolve_or_zero(em, 0.0, viewport)
                + s.border_left_width
                + s.padding_left.resolve_or_zero(em, 0.0, viewport);
            let post = s.padding_right.resolve_or_zero(em, 0.0, viewport)
                + s.border_right_width
                + s.margin_right.resolve_or_zero(em, 0.0, viewport);
            let start = out.len();
            // CSS Pseudo-elements L4 §4 — ::before in inline formatting context.
            // Block pseudo-elements inside inline context are skipped (Phase 0).
            if let Some(ps) =
                compute_pseudo_element_style(doc, id, "before", sheet, &s, viewport, dark_mode)
                && matches!(
                    ps.display,
                    Display::Inline
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::InlineBlock
                )
            {
                push_pseudo_inline_segs(&ps, doc, id, QuoteSlot::Before, viewport, counters, registry, out);
            }
            let children: Vec<NodeId> = flat.children_of(doc, id).to_vec();
            for child_id in children {
                collect_inline_segments(doc, sheet, child_id, &s, viewport, out, escapes, flat, counters, registry, need_first_letter, dark_mode);
            }
            // CSS Pseudo-elements L4 §4 — ::after in inline formatting context.
            if let Some(ps) =
                compute_pseudo_element_style(doc, id, "after", sheet, &s, viewport, dark_mode)
                && matches!(
                    ps.display,
                    Display::Inline
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::InlineBlock
                )
            {
                push_pseudo_inline_segs(&ps, doc, id, QuoteSlot::After, viewport, counters, registry, out);
            }
            let added = out.len() - start;
            // Mark all segments from this element (including pseudo-element content)
            // as element boxes so the painter draws their background/border.
            for seg in &mut out[start..start + added] {
                seg.is_element_box = true;
            }
            if added > 0 && (pre > 0.0 || post > 0.0) {
                out[start].pre_space += pre;
                out[start + added - 1].post_space += post;
            }
        }
        _ => {}
    }
}

/// Injects a pseudo-element box (::before or ::after) into the children list.
///
/// `is_before = true` → prepend; `false` → append.
/// Inline pseudo-elements are merged into the adjacent InlineRun when possible.
/// Block pseudo-elements are inserted as separate Block boxes.
///
/// `blockify = true` forces every pseudo-element into its own block-level box,
/// regardless of its computed `display`. Used for flex/grid containers: CSS
/// Flexbox §4 / Grid §6 blockify all in-flow children (including generated
/// `::before`/`::after`) into individual items, so they must not be merged into
/// an adjacent InlineRun.
#[allow(clippy::too_many_arguments)]
pub(crate) fn inject_pseudo(
    parent_id: NodeId,
    children: &mut Vec<LayoutBox>,
    ps: Option<ComputedStyle>,
    is_before: bool,
    doc: &Document,
    viewport: Size,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    blockify: bool,
) {
    let Some(ps) = ps else { return };
    let slot = if is_before { QuoteSlot::Before } else { QuoteSlot::After };
    match ps.display {
        Display::Inline
        | Display::InlineFlex
        | Display::InlineGrid
        | Display::InlineBlock
            if !blockify =>
        {
            let segs = content_to_inline_segments(&ps, doc, parent_id, slot, viewport, counters, registry);
            if segs.is_empty() {
                return;
            }
            if is_before {
                match children.first_mut() {
                    Some(LayoutBox { kind: BoxKind::InlineRun { segments, .. }, .. }) => {
                        let mut new_segs = segs;
                        new_segs.extend(std::mem::take(segments));
                        *segments = new_segs;
                    }
                    _ => children.insert(
                        0,
                        anon_inline_run(parent_id, &ps, segs, BoxRole::Pseudo(PseudoKind::Before)),
                    ),
                }
            } else {
                match children.last_mut() {
                    Some(LayoutBox { kind: BoxKind::InlineRun { segments, .. }, .. }) => {
                        segments.extend(segs);
                    }
                    _ => children.push(anon_inline_run(
                        parent_id,
                        &ps,
                        segs,
                        BoxRole::Pseudo(PseudoKind::After),
                    )),
                }
            }
        }
        _ => {
            // Block-level pseudo-element.
            let pseudo_kind = if is_before { PseudoKind::Before } else { PseudoKind::After };
            let inner_segs = content_to_inline_segments(&ps, doc, parent_id, slot, viewport, counters, registry);
            let inner = if inner_segs.is_empty() {
                vec![]
            } else {
                vec![anon_inline_run(parent_id, &ps, inner_segs, BoxRole::Pseudo(pseudo_kind))]
            };
            let b = LayoutBox {
                node: parent_id,
                rect: Rect::ZERO,
                style: Arc::new(ps),
                kind: BoxKind::Block,
                children: inner,
                col_span: 1,
                row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(parent_id), role: BoxRole::Pseudo(pseudo_kind) },
            };
            if is_before {
                children.insert(0, b);
            } else {
                children.push(b);
            }
        }
    }
}

/// Extracts text from `Content::Items` and returns it as a single `InlineSegment`.
///
/// Resolves `ContentItem::String`, `ContentItem::Counter`, `ContentItem::Counters`,
/// `ContentItem::Attr` and `open-quote`/`close-quote` using the per-element
/// `CounterMap` snapshot and DOM lookup. `owner_id` is the element whose
/// `::before`/`::after` pseudo-element we're generating; `slot` selects which
/// precomputed quote-depth list to consume (CSS Generated Content L3 §3.2).
/// Custom `@counter-style` names are resolved via `registry`.
fn content_to_inline_segments(
    style: &ComputedStyle,
    doc: &Document,
    owner_id: NodeId,
    slot: QuoteSlot,
    viewport: Size,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) -> Vec<InlineSegment> {
    let Content::Items(items) = &style.content else {
        return vec![];
    };
    let snap = counters.counters(owner_id);
    let qdepths = counters.quote_depths(owner_id, slot);
    let mut qi = 0usize;
    let mut out: Vec<InlineSegment> = Vec::new();
    // Text-producing items concatenate into a single run; a `url()` item flushes
    // the pending run and emits its own inline-replaced image segment.
    let mut text = String::new();

    for item in items {
        // CSS Generated Content L3 §2.1 — a `url()` value is an inline-replaced
        // image. It interrupts the surrounding text run and becomes its own image
        // segment (mirrors the inline-`<img>` path in `collect_inline_segments`).
        if let ContentItem::Url(url) = item {
            if !text.is_empty() {
                out.push(make_content_text_segment(style, owner_id, std::mem::take(&mut text)));
            }
            if !url.is_empty() {
                let em = style.font_size;
                // No intrinsic size is known before the image is fetched, so honour
                // an explicit `width` and otherwise fall back to `2em` — the same
                // placeholder the inline-`<img>` path uses for undecoded images.
                let w = style
                    .width
                    .as_ref()
                    .and_then(|l| l.resolve(em, None, viewport))
                    .unwrap_or(em * 2.0);
                out.push(make_content_image_segment(style, url.clone(), w));
            }
            continue;
        }
        let piece = match item {
            ContentItem::String(s) => Some(s.clone()),
            ContentItem::Counter { name, style: list_style } => {
                let val = snap
                    .and_then(|s| s.get(name))
                    .and_then(|v| v.last())
                    .copied()
                    .unwrap_or(0);
                let sname = list_style.as_deref().unwrap_or("decimal");
                Some(format_counter_with_registry(val, sname, registry))
            }
            ContentItem::Counters { name, separator, style: list_style } => {
                let vals = snap
                    .and_then(|s| s.get(name))
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let sname = list_style.as_deref().unwrap_or("decimal");
                let formatted: Vec<String> = vals
                    .iter()
                    .map(|&v| format_counter_with_registry(v, sname, registry))
                    .collect();
                Some(formatted.join(separator.as_str()))
            }
            ContentItem::Attr(attr) => {
                doc.get(owner_id).get_attr(attr).map(|s| s.to_string())
            }
            // CSS Generated Content L3 §3.2 — open-quote / close-quote pick a
            // (open, close) pair from `quotes` at the precomputed nesting depth.
            ContentItem::OpenQuote => {
                let depth = qdepths.get(qi).copied().unwrap_or(0);
                qi += 1;
                style.quotes.pair_for_depth(depth).map(|(o, _)| o.to_string())
            }
            ContentItem::CloseQuote => {
                let depth = qdepths.get(qi).copied().unwrap_or(0);
                qi += 1;
                style.quotes.pair_for_depth(depth).map(|(_, c)| c.to_string())
            }
            // url() is handled above; no-open-quote / no-close-quote only advance
            // depth (handled in the precompute pass) and emit nothing.
            _ => None,
        };
        if let Some(piece) = piece {
            text.push_str(&piece);
        }
    }
    if !text.is_empty() {
        out.push(make_content_text_segment(style, owner_id, text));
    }
    out
}

/// Builds a plain-text `InlineSegment` for generated (`content`) text.
/// `source_node` is the owning element so Selection/Range can map back to it.
fn make_content_text_segment(
    style: &ComputedStyle,
    owner_id: NodeId,
    text: String,
) -> InlineSegment {
    InlineSegment {
        text,
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: owner_id,
        source_char_offset: 0,
        bidi_level: 0,
    }
}

/// Builds an inline-replaced image `InlineSegment` for a `content: url(...)` item.
/// `source_node` is `NodeId::from_index(0)` ("no DOM origin"): a generated image is
/// not a selectable text node, and `collect_background_image_requests` keys on this
/// sentinel to recognise generated-content images that still need fetching +
/// registering (real inline `<img>` frags carry their element's own `NodeId`).
fn make_content_image_segment(
    style: &ComputedStyle,
    url: String,
    width: f32,
) -> InlineSegment {
    InlineSegment {
        text: String::new(),
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: true,
        img_src: Some(url),
        img_is_lazy: false,
        img_width: width,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    }
}

/// Builds inline segments for a pseudo-element and applies its own box model
/// spacing (margin + border + padding) as `pre_space` / `post_space`.
/// Used by `collect_inline_segments` to inject `::before` / `::after` content.
#[allow(clippy::too_many_arguments)]
fn push_pseudo_inline_segs(
    ps: &ComputedStyle,
    doc: &Document,
    owner_id: NodeId,
    slot: QuoteSlot,
    viewport: Size,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    out: &mut Vec<InlineSegment>,
) {
    let mut segs = content_to_inline_segments(ps, doc, owner_id, slot, viewport, counters, registry);
    if segs.is_empty() {
        return;
    }
    let em = ps.font_size;
    let pre = ps.margin_left.resolve_or_zero(em, 0.0, viewport)
        + ps.border_left_width
        + ps.padding_left.resolve_or_zero(em, 0.0, viewport);
    let post = ps.padding_right.resolve_or_zero(em, 0.0, viewport)
        + ps.border_right_width
        + ps.margin_right.resolve_or_zero(em, 0.0, viewport);
    if pre > 0.0 {
        segs[0].pre_space += pre;
    }
    if post > 0.0 {
        let last = segs.len() - 1;
        segs[last].post_space += post;
    }
    out.extend(segs);
}

/// CSS Lists L3 §2.1 — ordinal of a `<li>` among its element siblings (1-based).
pub(crate) fn li_ordinal(doc: &Document, id: NodeId) -> u32 {
    let Some(parent_id) = doc.get(id).parent else { return 1 };
    let mut n = 0u32;
    for &sib in &doc.get(parent_id).children.clone() {
        if matches!(&doc.get(sib).data, NodeData::Element { name, .. } if name.local.as_str() == "li") {
            n += 1;
            if sib == id {
                return n;
            }
        }
    }
    1
}

/// CSS Lists L3 §2.1 — creates `BoxKind::Marker` and prepends to children.
/// Calls `compute_pseudo_element_style("marker")` so CSS `::marker` rules (color,
/// font, content) override the defaults. `content: none` on `::marker` suppresses
/// the marker entirely; `content: <string>` / `counter()` replaces the default text.
#[allow(clippy::too_many_arguments)]
pub(crate) fn inject_marker(
    parent_id: NodeId,
    children: &mut Vec<LayoutBox>,
    style: &ComputedStyle,
    ordinal: u32,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) {
    // CSS Lists L3 §2.3: an explicit `list-style-image` shows even when
    // `list-style-type: none` — the image takes precedence over the type, so a
    // marker is still generated. Only suppress when both are absent.
    if matches!(style.list_style_type, ListStyleType::None) && style.list_style_image.is_none() {
        return;
    }
    // CSS Pseudo-elements L4 §14.2 — compute ::marker style.
    // Returns None only when `content: none` is set, which suppresses the marker.
    let Some(mut ms) = compute_pseudo_element_style(
        doc, parent_id, "marker", sheet, style, viewport, dark_mode,
    ) else {
        return;
    };
    // CSS: list-style-image — P4 wires image markers.
    let text = match &ms.content {
        Content::Items(items) => marker_content_text(items, doc, parent_id, counters, registry),
        // CSS: list-style-type (custom counter-style) — build_list_marker_text consults registry.
        _ => build_list_marker_text(style.list_style_type.clone(), ordinal, registry),
    };
    ms.display = Display::Inline;
    children.insert(0, LayoutBox {
        node:     parent_id,
        rect:     Rect::ZERO,
        style:    Arc::new(ms),
        kind:     BoxKind::Marker {
            text,
            position:        style.list_style_position,
            list_style_type: style.list_style_type.clone(),
            image:           style.list_style_image.clone(),
        },
        children: vec![],
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
        origin: BoxOrigin { node: Some(parent_id), role: BoxRole::ListMarker },
    });
}

/// Extracts a plain-text string from `::marker { content: <items> }`.
/// Supports String literals, `attr()`, `counter()`, `counters()`.
fn marker_content_text(
    items: &[ContentItem],
    doc: &Document,
    owner_id: NodeId,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) -> String {
    let snap = counters.counters(owner_id);
    items.iter().filter_map(|item| match item {
        ContentItem::String(s) => Some(s.clone()),
        ContentItem::Counter { name, style: list_style } => {
            let val = snap
                .and_then(|s| s.get(name))
                .and_then(|v| v.last())
                .copied()
                .unwrap_or(0);
            let sname = list_style.as_deref().unwrap_or("decimal");
            Some(format_counter_with_registry(val, sname, registry))
        }
        ContentItem::Counters { name, separator, style: list_style } => {
            let vals = snap
                .and_then(|s| s.get(name))
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let sname = list_style.as_deref().unwrap_or("decimal");
            let parts: Vec<String> = vals.iter()
                .map(|&v| format_counter_with_registry(v, sname, registry))
                .collect();
            Some(parts.join(separator.as_str()))
        }
        ContentItem::Attr(attr) => {
            doc.get(owner_id).get_attr(attr).map(str::to_string)
        }
        _ => None,
    }).collect()
}

/// CSS Display L3 §7.2 — replaces each `BoxKind::Contents` child with its own
/// children in-place. Grandchildren are already flattened (recursive `build_box`
/// calls run `flatten_contents` on inner levels first).
pub(crate) fn flatten_contents(children: &mut Vec<LayoutBox>) {
    let mut i = 0;
    while i < children.len() {
        if matches!(children[i].kind, BoxKind::Contents) {
            let grandchildren = std::mem::take(&mut children[i].children);
            let gc_len = grandchildren.len();
            children.remove(i);
            for (j, gc) in grandchildren.into_iter().enumerate() {
                children.insert(i + j, gc);
            }
            // Don't advance i — a grandchild might itself be Contents (edge case
            // if the inner build_box somehow produced an un-flattened Contents).
            // Advancing by gc_len skips them all safely since they were already
            // flattened at their own build level.
            i += gc_len;
        } else {
            i += 1;
        }
    }
}
