//! BUG-1800 срез 5 — ancestor-bloom для descendant-обхода матчинга.
//!
//! Срез 2 ([BUG-1800](../../bugs/BUG-1800-OPEN.md)) насчитал ~345k prefix-fail
//! на флаш: селекторы проходят subject-compound и перебирают ВСЕХ предков в
//! поисках префикса (`Combinator::Descendant`), хотя ни один предк документа
//! компаунд-префикс не удовлетворяет. Здесь — счётный bloom фильтр ключей
//! текущего пути каскадного `walk` (теги/классы/id), по которому весь обход
//! предков отменяется одним запросом, если ни один предк не может матчить
//! компаунд.
//!
//! Звук по построению, без стражей на ~30 местах вызова `matches_complex`:
//! консультация отвечает «отказ» только когда одновременно
//! 1. активна — верх кадра стека совпадает с узлом входа (`walk` кладёт кадр
//!    на каждый элемент и снимает его за пределами рекурсии; любой вызов
//!    вне walk-региона — selector_query/`:has()`/container/rule_roots — видит
//!    чужой или пустой стек и получает «не могу отказать»);
//! 2. путь не отравлен — кадр помечается отравленным, если composed-родитель
//!    (`walk` ходит по [`lumen_dom::FlatTree`]) не равен DOM-родителю: у
//!    slotted/теневых узлов DOM-предки не лежат на пройденном пути, и их
//!    ключей в bloom нет. Отравление наследуется вниз по пути;
//! 3. компаунд выразим ключами — только `Type`/`Class`/`Id`/`Universal`;
//!    селекторы атрибутов и псевдо-классы bloom не выражает («не могу
//!    отказать»);
//! 4. требуемый ключ отсутствует — bloom хранит ОБЪЕДИНЕНИЕ по всем предкам,
//!    его отсутствие опровергает каждого кандидата сразу. Ложные срабатывания
//!    «есть» только снижают экономию, ложных «нет» нет по построению.
//!
//! Семантика ключей снята с фактического матчера (`matches_simple`): тег —
//! побайтово с `name.local`, класс — токен `class`, id — значение `id`.

use lumen_css_parser::{CompoundSelector, SimpleSelector};
use lumen_dom::{Document, NodeData, NodeId};
use std::cell::RefCell;

/// 2^13 счётчиков u16 — 16 КиБ на поток, коллизии ложноположительны, но не
/// ложноотрицательны.
const BLOOM_BITS: usize = 13;
const BLOOM_MASK: usize = (1 << BLOOM_BITS) - 1;

/// Порода ключа в хэше — тег, класс и id не смешиваются.
const KIND_TAG: u8 = 0;
const KIND_CLASS: u8 = 1;
const KIND_ID: u8 = 2;

struct Frame {
    node: NodeId,
    /// Composed-путь разошёлся с DOM-путём (slotted/теневой узел) — консультации
    /// по этому поддереву и всем вложенным запрещены.
    poisoned: bool,
    /// Метка в [`FilterState::journal`] на момент push — откат вставок за O(вставок).
    journal_mark: usize,
}

#[derive(Default)]
struct FilterState {
    frames: Vec<Frame>,
    /// Позиции bloom, поднятые вставками текущего пути; pop уменьшает счётчики.
    journal: Vec<(usize, usize)>,
    counters: Vec<u16>,
}

thread_local! {
    static STATE: RefCell<FilterState> = RefCell::new(FilterState {
        counters: vec![0u16; 1 << BLOOM_BITS],
        ..FilterState::default()
    });
}

/// FNV-1a с байтом породы — один и тот же текст в теге и классе не смешивается.
fn key_hash(kind: u8, key: &str) -> u32 {
    let mut h = 0x811c9dc5_u32 ^ (kind as u32).wrapping_mul(0x01000193);
    for b in key.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x01000193);
    }
    h
}

fn positions(kind: u8, key: &str) -> (usize, usize) {
    let h = key_hash(kind, key);
    let h2 = h.wrapping_mul(0x9e37_79b9).rotate_left(15);
    (h as usize & BLOOM_MASK, h2 as usize & BLOOM_MASK)
}

fn contains(st: &FilterState, kind: u8, key: &str) -> bool {
    let (p0, p1) = positions(kind, key);
    st.counters[p0] > 0 && st.counters[p1] > 0
}

fn insert(st: &mut FilterState, kind: u8, key: &str) {
    let (p0, p1) = positions(kind, key);
    st.counters[p0] = st.counters[p0].saturating_add(1);
    st.counters[p1] = st.counters[p1].saturating_add(1);
    st.journal.push((p0, p1));
}

/// RAII-кадр одного элемента пути [`crate::counters::walk`]: клave узла в bloom
/// на всю рекурсию в поддерево (потомки матчят префиксы против него как предка),
/// при `Drop` — откат вставок и кадра.
pub(crate) struct FrameGuard(());

impl Drop for FrameGuard {
    #[allow(clippy::expect_used)] // инвариант: метка кадра ограничивает журнал, docs/lint-policy.md §10
    fn drop(&mut self) {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            let Some(top) = st.frames.pop() else { return };
            while st.journal.len() > top.journal_mark {
                let (p0, p1) = st.journal.pop().expect("mark bounds the journal");
                st.counters[p0] = st.counters[p0].saturating_sub(1);
                st.counters[p1] = st.counters[p1].saturating_sub(1);
            }
        });
    }
}

/// Кладёт кадр элемента `node`, зашедшего в `walk` под composed-родителем
/// `composed_parent` ([`lumen_dom::FlatTree`]): ключи тега/классов/id — в bloom,
/// отравление наследуется от родительского кадра и ставится при расхождении
/// composed- и DOM-пути.
pub(crate) fn push_frame(doc: &Document, node: NodeId, composed_parent: NodeId) -> FrameGuard {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        let poisoned =
            st.frames.last().is_some_and(|f| f.poisoned) || doc.get(node).parent != Some(composed_parent);
        let journal_mark = st.journal.len();
        if let NodeData::Element { name, attrs } = &doc.get(node).data {
            insert(&mut st, KIND_TAG, &name.local);
            if let Some(id) = attrs.iter().find(|a| a.name.local == "id") {
                insert(&mut st, KIND_ID, &id.value);
            }
            if let Some(class) = attrs.iter().find(|a| a.name.local == "class") {
                for token in class.value.split_whitespace() {
                    insert(&mut st, KIND_CLASS, token);
                }
            }
        }
        st.frames.push(Frame { node, poisoned, journal_mark });
        FrameGuard(())
    })
}

/// Может ли хоть один DOM-предк `node` удовлетворить `compound`?
///
/// `false` — только когда фильтр активен, путь не отравлен, компаунд выразим
/// ключами и требуемого ключа нет ни у одного предка пути: тогда весь
/// candidate-обход [`Combinator::Descendant`] в `matches_chain` отменяется.
/// Во всех остальных случаях — `true` («фильтр не может отказать»).
pub(crate) fn prefix_may_match_descendant(compound: &CompoundSelector, node: NodeId) -> bool {
    STATE.with(|s| {
        let st = s.borrow();
        let Some(top) = st.frames.last() else { return true };
        if top.node != node || top.poisoned {
            return true;
        }
        for part in &compound.parts {
            match part {
                SimpleSelector::Universal => {}
                SimpleSelector::Type(t) => {
                    if !contains(&st, KIND_TAG, t) {
                        note_refuted();
                        return false;
                    }
                }
                SimpleSelector::Class(c) => {
                    if !contains(&st, KIND_CLASS, c) {
                        note_refuted();
                        return false;
                    }
                }
                SimpleSelector::Id(i) => {
                    if !contains(&st, KIND_ID, i) {
                        note_refuted();
                        return false;
                    }
                }
                // Селекторы атрибутов и псевдо-классы/элементы bloom не выражает.
                _ => return true,
            }
        }
        true
    })
}

fn note_refuted() {
    crate::counters::note_prefix_refuted();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_compound(sel: &str) -> CompoundSelector {
        lumen_css_parser::parse_selector_list(sel).first().expect("one selector").head.clone()
    }

    /// Неотравленный кадр: composed-родитель = DOM-родитель, как у walk на
    /// документе без теневого дерева.
    #[allow(clippy::expect_used)] // тестовая фикстура, docs/lint-policy.md §10
    fn frame(doc: &Document, node: NodeId) -> FrameGuard {
        push_frame(doc, node, doc.get(node).parent.expect("fixture node has a parent"))
    }

    fn fixture(html: &str) -> (Document, NodeId, NodeId, NodeId) {
        let doc = lumen_html_parser::parse(html);
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let p = doc.get(div).children[0];
        (doc, body, div, p)
    }

    /// Компаунд, не выражаемый ни одним предком пути, отказывает; выражаемый — нет;
    /// атрибуты и псевдоклассы фильтр не выражает.
    #[test]
    fn refutes_keys_absent_from_the_walked_path() {
        let (doc, body, div, p) = fixture("<div class=\"wrap\"><p id=\"x\">t</p></div>");
        let _fb = frame(&doc, body);
        let _fd = frame(&doc, div);
        let _fp = frame(&doc, p);
        assert!(!prefix_may_match_descendant(&parse_compound(".a"), p));
        assert!(!prefix_may_match_descendant(&parse_compound("span"), p));
        assert!(!prefix_may_match_descendant(&parse_compound("#other"), p));
        assert!(prefix_may_match_descendant(&parse_compound(".wrap"), p));
        assert!(prefix_may_match_descendant(&parse_compound("div.wrap"), p));
        assert!(prefix_may_match_descendant(&parse_compound("#x"), p));
        assert!(prefix_may_match_descendant(&parse_compound("p"), p));
        assert!(!prefix_may_match_descendant(&parse_compound(".wrap.a"), p));
        assert!(prefix_may_match_descendant(&parse_compound("[data-x]"), p));
        assert!(prefix_may_match_descendant(&parse_compound(":hover"), p));
    }

    /// Чужой узел входа (не верх кадра) — консультация не отказывает: так видит
    /// фильтр любой вызов `matches_complex` вне walk-региона или на другой узел.
    #[test]
    fn foreign_subject_node_is_never_refuted() {
        let (doc, body, div, p) = fixture("<div><p>t</p></div>");
        let _fb = frame(&doc, body);
        let _fd = frame(&doc, div);
        assert!(prefix_may_match_descendant(&parse_compound(".a"), p));
    }

    /// Расхождение composed- и DOM-пути (slotted/теневой узел) отравляет кадр,
    /// и отравление наследуется потомками — отказ запрещён.
    #[test]
    fn composed_parent_mismatch_poisons_the_frame() {
        let (doc, body, div, p) = fixture("<div><p><span>s</span></p></div>");
        let span = doc.get(p).children[0];
        let _fb = frame(&doc, body);
        let _fd = frame(&doc, div);
        // Заявленный composed-родитель p — body, DOM-родитель — div: отравлен.
        let _fp = push_frame(&doc, p, body);
        assert!(prefix_may_match_descendant(&parse_compound(".nope"), p));
        let _fs = frame(&doc, span);
        assert!(prefix_may_match_descendant(&parse_compound(".nope"), span));
    }

    /// Откат кадра снимает ровно его вставки — в обе стороны (нет ни
    /// недоотката, ни переотката), включая двойную вставку одного ключа.
    #[test]
    fn pop_restores_the_previous_path() {
        let (doc, body, div, p) = fixture("<div class=\"wrap\"><p>t</p></div>");
        let _fb = frame(&doc, body);
        {
            let _fp = frame(&doc, p);
            assert!(!prefix_may_match_descendant(&parse_compound(".wrap"), p));
        }
        {
            let _fd = frame(&doc, div);
            let _fp = frame(&doc, p);
            assert!(prefix_may_match_descendant(&parse_compound(".wrap"), p));
        }
        {
            let _fp = frame(&doc, p);
            assert!(!prefix_may_match_descendant(&parse_compound(".wrap"), p));
        }

        let doc2 = lumen_html_parser::parse("<div class=\"wrap\"><section class=\"wrap\"><p>t</p></section></div>");
        let body2 = doc2.body().expect("body");
        let div2 = doc2.get(body2).children[0];
        let section2 = doc2.get(div2).children[0];
        let p2 = doc2.get(section2).children[0];
        let _fb = frame(&doc2, body2);
        {
            let _fd = frame(&doc2, div2);
            let _fs = frame(&doc2, section2);
            let _fp = frame(&doc2, p2);
            assert!(prefix_may_match_descendant(&parse_compound(".wrap"), p2));
        }
        {
            let _fp = frame(&doc2, p2);
            assert!(!prefix_may_match_descendant(&parse_compound(".wrap"), p2));
        }
    }
}
