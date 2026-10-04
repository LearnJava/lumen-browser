//! PERF-16, срез 1: `lumen_layout::subtree_paint_eq` — ключ будущего кэша display list по
//! поддереву. Кэш пропустит emit, когда ключ равен снимку, поэтому единственное, что важно
//! доказать, — **безопасность**: `subtree_paint_eq(a, b) ⇒ emit(a) == emit(b)` (команды и
//! provenance-спаны). Обратное неверно и не нужно: лишний промах — только лишний emit.
//!
//! Доказательство — дифференциальное, на реальных страницах: каждая страница
//! `graphic_tests/*.html` раскладывается дважды, во второй раз с CSS-возмущением (одно paint-
//! свойство на узком наборе селекторов), и для каждой пары одноимённых поддеревьев без
//! stacking context внутри (ровно та единица, которую кэшировал бы срез 2) проверяется
//! импликация. Тест не пуст: он считает пары, у которых emit разошёлся, и пары, где ключ
//! признал равенство — оба числа обязаны быть заметными.
//!
//! Вторая часть — чувствительность: на ручной странице каждое поле, которое читает emit,
//! по очереди меняется, и ключ обязан заметить (`dirty` — единственное, что он обязан не замечать).

use super::*;
use lumen_dom::{Document, NodeData, NodeId};
use lumen_layout::subtree_paint_eq;
use std::sync::Arc;

use super::text_and_images::Fixed8;

/// Страницы крупнее — не берём: тест раскладывает каждую десятки раз.
const MAX_PAGE_BYTES: usize = 120 * 1024;

/// Содержимое всех `<style>` документа.
fn style_text(doc: &Document) -> String {
    fn walk(doc: &Document, id: NodeId, out: &mut String) {
        let node = doc.get(id);
        if let NodeData::Element { name, .. } = &node.data
            && name.local == "style"
        {
            for &child in &node.children {
                if let NodeData::Text(s) = &doc.get(child).data {
                    out.push_str(s);
                    out.push('\n');
                }
            }
            return;
        }
        for &child in &node.children {
            walk(doc, child, out);
        }
    }
    let mut out = String::new();
    walk(doc, doc.root(), &mut out);
    out
}

pub(super) fn layout_page(html: &str, extra_css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let css = format!("{}\n{extra_css}", style_text(&doc));
    let sheet = lumen_css_parser::parse(&css);
    lumen_layout::layout_measured(&doc, &sheet, Size::new(1024.0, 720.0), &Fixed8)
}

pub(super) fn corpus() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../graphic_tests");
    let mut pages = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("graphic_tests/ рядом с крейтом") {
        let path = entry.expect("запись каталога").path();
        if path.extension().is_none_or(|e| e != "html") {
            continue;
        }
        let Ok(html) = std::fs::read_to_string(&path) else { continue };
        if html.len() > MAX_PAGE_BYTES {
            continue;
        }
        pages.push((path.file_name().unwrap().to_string_lossy().into_owned(), html));
    }
    pages.sort();
    pages
}

/// Бокс сам — владелец stacking context (его emit уходит в чужой бакет).
fn owns_sc(b: &LayoutBox) -> bool {
    owns_paint_layer(b)
}

/// Поддерево можно эмитить в один бакет: ни сам корень, ни потомок не создают SC.
fn single_bucket(root: &LayoutBox) -> bool {
    let mut stack = vec![root];
    while let Some(b) = stack.pop() {
        if owns_sc(b) {
            return false;
        }
        stack.extend(b.children.iter());
    }
    true
}

/// Emit одного non-SC поддерева так, как его делает `continue_fill` для non-SC ребёнка:
/// `is_sc_root = false`, бакет текущего SC. Возвращает команды и спаны в виде строк
/// (`Debug` — тот же приём, что у `assert_dl_eq`).
fn emit_subtree(b: &LayoutBox) -> (Vec<String>, Vec<String>) {
    let mut buckets = vec![ScBucket::default()];
    let mut next_sc_id = 1;
    let mut split = SplitTracker {
        enabled: false,
        animated_scs: Vec::new(),
        content_spans: Vec::new(),
        invalid: false,
        sc_entries: 0,
    };
    let mut spans = Vec::new();
    fill_buckets(
        b,
        StackingContextId::ROOT,
        &mut next_sc_id,
        &mut buckets,
        false,
        None,
        1.0,
        &[],
        &mut split,
        &mut spans,
    );
    assert_eq!(next_sc_id, 1, "single_bucket-поддерево не должно заводить stacking context");
    let bucket = &buckets[0];
    assert!(bucket.pre.is_empty() && bucket.root_bg.is_empty() && bucket.post.is_empty());
    let cmds = bucket.contents.iter().map(|c| format!("{c:?}")).collect();
    let spans = spans
        .iter()
        .map(|s| format!("{}/{:?}/{:?}/{:?}/{}", s.sc, s.field, s.range, s.origin, s.fragment))
        .collect();
    (cmds, spans)
}

#[derive(Default)]
struct Tally {
    pairs: usize,
    key_equal: usize,
    emit_differs: usize,
    /// Ключ сказал «равны», а emit разошёлся — то, чего не должно быть никогда.
    unsound: Vec<String>,
    /// Ключ сказал «не равны», а emit совпал — допустимый лишний emit.
    false_miss: usize,
}

/// Обходит пару деревьев параллельно, пока структура совпадает, и проверяет импликацию на
/// каждой паре подходящих поддеревьев.
fn compare_trees(label: &str, a: &LayoutBox, b: &LayoutBox, tally: &mut Tally) {
    // Вьюпорт fixed-фонов — часть предусловия (владелец кэша держит его в ключе): оба emit-а
    // идут при одном и том же.
    let _vp = FixedBgViewportGuard::install(a);
    let mut stack = vec![(a, b)];
    while let Some((x, y)) = stack.pop() {
        if x.node != y.node || x.origin != y.origin {
            continue;
        }
        if single_bucket(x) && single_bucket(y) {
            tally.pairs += 1;
            let key = subtree_paint_eq(x, y);
            let same = emit_subtree(x) == emit_subtree(y);
            tally.key_equal += usize::from(key);
            tally.emit_differs += usize::from(!same);
            if key && !same {
                tally.unsound.push(format!("{label}: node {:?} origin {:?}", x.node, x.origin));
            }
            tally.false_miss += usize::from(!key && same);
        }
        if x.children.len() == y.children.len() {
            stack.extend(x.children.iter().zip(y.children.iter()));
        }
    }
}

/// Возмущения: одно paint-свойство (или их связка) на узком наборе селекторов. Список покрывает
/// каждую группу полей `ComputedStyle`, которые читает emit: цвет, фон (цвет/градиент/клип),
/// рамки и радиусы, тени, outline, декорации и тени текста, шрифт, видимость, переполнение,
/// списки, таблицы, replaced-элементы, генерируемое содержимое, режимы письма.
pub(super) const PERTURBATIONS: &[&str] = &[
    "color: #e11 !important",
    "background-color: #0a8 !important",
    "background-image: linear-gradient(#f00, #00f) !important",
    "background-image: radial-gradient(circle, #fa0, #0af) !important",
    "background-clip: padding-box !important; background-color: #d4d !important; border: 4px dashed #333 !important",
    "border: 3px solid #123 !important",
    "border-color: #f0f #0ff #ff0 #000 !important; border-style: solid !important; border-width: 2px !important",
    "border-radius: 9px !important; background-color: #cde !important",
    "box-shadow: 3px 4px 5px #888 !important",
    "box-shadow: inset 0 0 6px #070 !important",
    "outline: 2px dotted #c00 !important; outline-offset: 3px !important",
    "text-decoration: underline wavy #c0c !important",
    "text-decoration: line-through !important",
    "text-shadow: 2px 2px 3px #444 !important",
    "-webkit-text-stroke: 1px #f00 !important",
    "font-weight: 800 !important",
    "font-style: italic !important",
    "font-family: monospace !important",
    "letter-spacing: 2px !important",
    "text-transform: uppercase !important",
    "visibility: hidden !important",
    "overflow: hidden !important",
    "overflow: scroll !important",
    "opacity: 0.4 !important",
    "list-style: square inside !important",
    "list-style-type: decimal !important",
    "border-collapse: collapse !important; border: 1px solid #000 !important",
    "object-fit: cover !important; object-position: 10% 90% !important",
    "image-rendering: pixelated !important",
    "direction: rtl !important",
    "writing-mode: vertical-rl !important",
    "text-align: right !important",
    "padding: 6px !important",
    "margin: 5px !important",
    "width: 77px !important; height: 33px !important",
    "display: none !important",
    "position: relative !important; left: 7px !important; top: 3px !important",
    "caret-color: #f0f !important; accent-color: #0f0 !important",
    "cursor: pointer !important; user-select: none !important",
    "filter: blur(2px) !important",
    "transform: rotate(3deg) !important",
    "clip-path: inset(4px) !important",
    "mix-blend-mode: multiply !important",
    "isolation: isolate !important",
    "background-attachment: fixed !important; background-image: linear-gradient(#0f0, #f0f) !important",
    "text-overflow: ellipsis !important; overflow: hidden !important; white-space: nowrap !important",
    "text-indent: 12px !important; word-spacing: 4px !important",
    "text-emphasis: filled red !important",
    "scrollbar-color: #f00 #00f !important; overflow: auto !important",
    "contain: paint !important",
    "content: 'x' !important",
];

/// Селекторы возмущений: от «всё» до единичных тегов, чтобы часть поддеревьев оставалась нетронутой
/// (иначе нечего сравнивать на равенство).
pub(super) const SELECTORS: &[&str] = &[
    "*",
    "div",
    "p, li, dd, dt",
    "span, a, b, i, em, strong, code, small",
    "h1, h2, h3, h4",
    "img, svg, canvas, video, iframe",
    "table, tr, td, th, caption",
    "input, button, select, textarea",
    "body > :first-child",
    ":nth-child(2n)",
    "p::before, li::before, div::before",
    "ul, ol, section, article",
];

#[test]
fn key_equality_implies_identical_emit_on_the_page_corpus() {
    let pages = corpus();
    assert!(pages.len() > 100, "корпус graphic_tests пуст или не найден: {}", pages.len());
    let mut tally = Tally::default();
    let mut skipped = 0_usize;
    for (page_ix, (name, html)) in pages.iter().enumerate() {
        let base = layout_page(html, "");
        // Тот же вход раскладывается заново — ключ обязан считать деревья равными, а emit совпасть.
        let again = layout_page(html, "");
        compare_trees(&format!("{name} / повтор"), &base, &again, &mut tally);
        for (i, decl) in PERTURBATIONS.iter().enumerate() {
            // Разные страницы получают разные пары «селектор × свойство»: полный перебор
            // 50×12×180 раскладок не нужен — покрытие даёт сдвиг по индексу страницы.
            let sel = SELECTORS[(i + page_ix) % SELECTORS.len()];
            let css = format!("{sel} {{ {decl} }}");
            // В debug-профиле раскладка проверяет геометрические инварианты (`DEVX-8a`) и на части
            // возмущённых страниц паникует — это дефект раскладки, а не ключа. Такая пара
            // пропускается и считается; доля пропусков ограничена ниже.
            let layout = std::panic::catch_unwind(|| layout_page(html, &css));
            let Ok(perturbed) = layout else {
                skipped += 1;
                continue;
            };
            compare_trees(&format!("{name} / {css}"), &base, &perturbed, &mut tally);
        }
    }
    eprintln!(
        "[subtree_paint_eq] pairs={} key_equal={} emit_differs={} false_miss={} unsound={} skipped_layouts={skipped}",
        tally.pairs,
        tally.key_equal,
        tally.emit_differs,
        tally.false_miss,
        tally.unsound.len(),
    );
    assert!(
        tally.unsound.is_empty(),
        "ключ признал равными поддеревья с разным emit ({} шт.), первые: {:#?}",
        tally.unsound.len(),
        &tally.unsound[..tally.unsound.len().min(10)],
    );
    assert!(
        skipped * 50 < pages.len() * PERTURBATIONS.len(),
        "слишком много возмущённых раскладок упало на инвариантах: {skipped}",
    );
    // Не пустой тест: и «равно», и «разошлось» встречаются массово.
    assert!(tally.key_equal > 5_000, "слишком мало пар с равным ключом: {}", tally.key_equal);
    assert!(tally.emit_differs > 5_000, "слишком мало пар с разным emit: {}", tally.emit_differs);
}

// ── чувствительность: каждое читаемое emit-ом поле ───────────────────────────

const HAND_PAGE: &str = r##"
<style>
 body { margin: 0 }
 #box { width: 200px; height: 60px; background: #cde; border: 2px solid #345; padding: 4px }
 #svgbox { width: 40px; height: 40px }
</style>
<div id="box">hello <b>bold</b> world<input value="v"></div>
<svg id="svgbox" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="red"/></svg>
<img src="a.png" alt="alt" width="10" height="10">
"##;

fn find_by_node<'a>(root: &'a LayoutBox, pred: &dyn Fn(&LayoutBox) -> bool) -> Option<&'a LayoutBox> {
    let mut stack = vec![root];
    while let Some(b) = stack.pop() {
        if pred(b) {
            return Some(b);
        }
        stack.extend(b.children.iter());
    }
    None
}

/// Применяет `f` к первому боксу, для которого `pred` истинно, в клоне дерева.
fn mutated(root: &LayoutBox, pred: &dyn Fn(&LayoutBox) -> bool, f: &dyn Fn(&mut LayoutBox)) -> LayoutBox {
    fn go(b: &mut LayoutBox, pred: &dyn Fn(&LayoutBox) -> bool, f: &dyn Fn(&mut LayoutBox)) -> bool {
        if pred(b) {
            f(b);
            return true;
        }
        b.children.iter_mut().any(|c| go(c, pred, f))
    }
    let mut copy = root.clone();
    assert!(go(&mut copy, pred, f), "мутируемый бокс не найден");
    copy
}

fn has_frags(b: &LayoutBox) -> bool {
    matches!(&b.kind, BoxKind::InlineRun { lines, .. } if lines.iter().any(|l| !l.is_empty()))
}

#[test]
fn key_notices_every_field_emit_reads_and_ignores_dirty() {
    let base = layout_page(HAND_PAGE, "");
    assert!(subtree_paint_eq(&base, &base.clone()));
    assert!(subtree_paint_eq(&base, &layout_page(HAND_PAGE, "")), "повторная раскладка того же входа");

    let any = |_: &LayoutBox| true;
    let is_block_child = |b: &LayoutBox| matches!(b.kind, BoxKind::Block) && b.rect.width > 100.0;

    // `dirty` — служебные биты инкрементального layout, emit их не читает.
    let ignored = mutated(&base, &any, &|b| b.dirty = lumen_layout::incremental::DirtyBits::SUBTREE);
    assert!(subtree_paint_eq(&base, &ignored), "dirty не должен влиять на ключ");

    type Mutation = (&'static str, Box<dyn Fn(&mut LayoutBox)>);
    let mutations: Vec<Mutation> = vec![
        ("rect.x", Box::new(|b| b.rect.x += 0.5)),
        ("rect.y", Box::new(|b| b.rect.y += 0.5)),
        ("rect.width", Box::new(|b| b.rect.width += 0.5)),
        ("rect.height", Box::new(|b| b.rect.height += 0.5)),
        ("rect -0.0", Box::new(|b| {
            b.rect.x = 0.0;
            b.rect.y = -0.0;
        })),
        ("used_line_height", Box::new(|b| b.used_line_height += 1.0)),
        ("scroll_x", Box::new(|b| b.scroll_x += 3.0)),
        ("scroll_y", Box::new(|b| b.scroll_y += 3.0)),
        ("col_span", Box::new(|b| b.col_span += 1)),
        ("row_span", Box::new(|b| b.row_span += 1)),
        ("origin.role", Box::new(|b| b.origin.role = BoxRole::GeneratedContent)),
        ("origin.node", Box::new(|b| b.origin.node = Some(NodeId::from_index(9999)))),
        ("node", Box::new(|b| b.node = NodeId::from_index(9999))),
        ("style (background)", Box::new(|b| {
            Arc::make_mut(&mut b.style).background_color = Some(CssColor::Rgba(Color { r: 1, g: 2, b: 3, a: 255 }));
        })),
        ("style (opacity)", Box::new(|b| Arc::make_mut(&mut b.style).opacity = 0.5)),
        ("style (border width)", Box::new(|b| Arc::make_mut(&mut b.style).border_top_width += 1.0)),
        ("drop a child", Box::new(|b| {
            b.children.pop();
        })),
    ];
    for (name, f) in &mutations {
        let changed = mutated(&base, &is_block_child, f);
        // `drop a child` на листе — no-op; ищем бокс с детьми.
        if *name == "drop a child" {
            let changed = mutated(&base, &|b: &LayoutBox| !b.children.is_empty(), f);
            assert!(!subtree_paint_eq(&base, &changed), "ключ не заметил: {name}");
            continue;
        }
        assert!(!subtree_paint_eq(&base, &changed), "ключ не заметил: {name}");
    }

    // Фрагменты строк: позиция, текст, стиль (полный `ComputedStyle` по значению).
    assert!(find_by_node(&base, &has_frags).is_some(), "на ручной странице есть inline-фрагменты");
    let frag_mutations: Vec<Mutation> = vec![
        ("frag.x", Box::new(|b| frag_mut(b, |f| f.x += 0.25))),
        ("frag.width", Box::new(|b| frag_mut(b, |f| f.width += 0.25))),
        ("frag.y_offset", Box::new(|b| frag_mut(b, |f| f.y_offset += 0.25))),
        ("frag.text", Box::new(|b| frag_mut(b, |f| f.text.push('!')))),
        ("frag.style.color", Box::new(|b| {
            frag_mut(b, |f| std::sync::Arc::make_mut(&mut f.style).color = Color { r: 9, g: 8, b: 7, a: 255 });
        })),
        ("frag.padding_left", Box::new(|b| frag_mut(b, |f| f.padding_left += 1.0))),
        ("frag.bidi_level", Box::new(|b| frag_mut(b, |f| f.bidi_level += 1))),
        ("frag.source_char_offset", Box::new(|b| frag_mut(b, |f| f.source_char_offset += 1))),
    ];
    for (name, f) in &frag_mutations {
        let changed = mutated(&base, &has_frags, f);
        assert!(!subtree_paint_eq(&base, &changed), "ключ не заметил: {name}");
    }
}

fn frag_mut(b: &mut LayoutBox, f: impl Fn(&mut InlineFrag)) {
    if let BoxKind::InlineRun { lines, .. } = &mut b.kind {
        let frag = lines
            .iter_mut()
            .flat_map(|l| l.iter_mut())
            .next()
            .expect("has_frags гарантирует фрагмент");
        f(frag);
    }
}

/// Входы layout, которые emit не читает (`segments`, `first_line_*`, `row_continuation_width`),
/// ключ не должен замечать — иначе кэш промахивался бы по полям, не влияющим на пиксели.
#[test]
fn key_ignores_layout_only_inline_run_fields() {
    let base = layout_page(HAND_PAGE, "");
    let changed = mutated(&base, &has_frags, &|b| {
        if let BoxKind::InlineRun { segments, row_continuation_width, first_line_inset, .. } = &mut b.kind {
            segments.clear();
            *row_continuation_width = Some(123.0);
            *first_line_inset += 5.0;
        }
    });
    assert!(subtree_paint_eq(&base, &changed));
}
