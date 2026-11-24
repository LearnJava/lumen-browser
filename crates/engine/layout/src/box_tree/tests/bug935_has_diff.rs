//! BUG-1800 срез 17 — диф `:has()`-значений в has-reach: корень ставится
//! только на реальный флип значения аргумента с прошлого флаша.
//!
//! Звучность — дифференциал: инкрементальный каскад == полный на каждом шаге
//! (и при флипе, и без него). Сужение — поведение root-set: без флипа has-reach
//! не называет субъектов правила, с флипом — называет (те же point-корни, что
//! давал структурный вопрос). Промах кэша/моста = широкая дорога, как до среза.
//! Каждый сценарий идёт через прогревочный шаг: запись дифа штампуется
//! мутационной эпохой шага, чтение отвечает только на предыдущую эпоху —
//! как в живом флаше (`basis.touch_epoch` → `touched.epoch`).

use super::*;

use lumen_css_parser::Stylesheet;
use lumen_dom::{Attribute, QualName};

/// Кэш дифа и его переключатель — глобальные на процесс: тесты, трогающие их,
/// сериализуются этим локом (параллельные листы затирают ревизию друг друга).
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const HTML: &str = r##"<body>
  <div id="box"><p id="p1">one</p><div class="card">c1</div></div>
</body>"##;

/// `#box:has(.card) p` — субъект `p` не локальный: флип рестайлит его точечно.
const CSS: &str = r#"
p { color: black }
#box:has(.card) p { color: red }
"#;

const CSS_ATTR: &str = r#"
p { color: black }
#box:has([data-x]) p { color: red }
"#;

struct Rig {
    doc: lumen_dom::Document,
    sheet: std::sync::Arc<Stylesheet>,
    prev: crate::counters::CounterMap,
    vp: Size,
    flat: lumen_dom::FlatTree,
    steps: u32,
}

fn rig(css: &str) -> Rig {
    let doc = lumen_html_parser::parse(HTML);
    let sheet = std::sync::Arc::new(lumen_css_parser::parse(css));
    let vp = Size::new(800.0, 600.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let prev = crate::counters::precompute_counters(&doc, &sheet, vp, &flat, false);
    Rig { doc, sheet, prev, vp, flat, steps: 0 }
}

impl Rig {
    fn by_id(&self, id: &str) -> lumen_dom::NodeId {
        self.doc.find_by_id(id).unwrap_or_else(|| panic!("fixture id {id}"))
    }

    fn set_attr(&mut self, node: lumen_dom::NodeId, name: &str, value: &str) {
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut self.doc.get_mut(node).data {
            match attrs.iter_mut().find(|a| a.name.local == name) {
                Some(attr) => attr.value = value.to_string(),
                None => attrs.push(Attribute { name: QualName::html(name), value: value.to_string() }),
            }
        }
    }

    fn new_card(&mut self, text: &str) -> lumen_dom::NodeId {
        let card = self.doc.create_element(QualName::html("div"));
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut self.doc.get_mut(card).data {
            attrs.push(Attribute { name: QualName::html("class"), value: "card".to_string() });
        }
        let t = self.doc.create_text(text);
        self.doc.append_child(card, t);
        card
    }

    /// Прогревочный шаг: ChildList-правка на контейнере (change_can_flip_has
    /// всегда true) — диф оценивает аргументы и пишет их в кэш этой эпохи.
    fn warm(&mut self, node: lumen_dom::NodeId) -> crate::style::RestyleRoots {
        let tmp = self.doc.create_element(QualName::html("div"));
        self.doc.append_child(node, tmp);
        let change = crate::style::NodeChange::ChildListEnds { front: false, back: true, edits: 1 };
        self.step(node, change, true)
    }

    /// Один шаг: мутация уже применена вызывающим; инкрементальный каскад от
    /// прошлого прохода против полного. Обновляет `self.prev`. Эпохи шага —
    /// `(номер прошлого шага, номер этого)`, как `basis.touch_epoch` →
    /// `touched.epoch` в живом флаше.
    fn step(
        &mut self,
        node: lumen_dom::NodeId,
        change: crate::style::NodeChange<'_>,
        shared_index: bool,
    ) -> crate::style::RestyleRoots {
        use crate::counters::{
            incremental_precompute_counters, set_incremental_restyle, take_cascade_stats, ContentDirty, RestyleDelta,
        };
        use crate::style::{restyle_node_index, restyle_node_index_shared, restyle_roots_for_node_changes_with_basis};

        let epochs = Some((u64::from(self.steps), u64::from(self.steps) + 1));
        self.steps += 1;
        let prev = std::mem::take(&mut self.prev);
        let has_style = |n: lumen_dom::NodeId| prev.styles().contains_key(&n);
        let change_set = [(node, change)];
        let roots = if shared_index {
            let index = restyle_node_index_shared(&self.doc, &self.sheet);
            restyle_roots_for_node_changes_with_basis(&self.doc, change_set, &index, &has_style, epochs)
        } else {
            let index = restyle_node_index(&self.doc, &self.sheet);
            restyle_roots_for_node_changes_with_basis(&self.doc, change_set, &index, &has_style, epochs)
        };
        let mut content = std::collections::HashSet::new();
        content.insert(node);
        let delta = RestyleDelta {
            invalidation: None, attr_changed: None,
            prev_styles: prev.styles().clone(),
            dirty_roots: roots.deep.clone(),
            shallow_roots: roots.shallow.clone(),
            point_roots: roots.point.clone(),
            content_dirty: ContentDirty::Nodes(&content),
        };
        let _ = take_cascade_stats();
        set_incremental_restyle(true);
        let incr = incremental_precompute_counters(&self.doc, &self.sheet, self.vp, &self.flat, false, delta);
        set_incremental_restyle(false);
        let full = crate::counters::precompute_counters(&self.doc, &self.sheet, self.vp, &self.flat, false);
        assert_eq!(
            incr.styles(),
            full.styles(),
            "the has-diff cascade must reproduce the full cascade exactly"
        );
        self.prev = full;
        roots
    }
}

/// Без флипа (второй `.card`, `:has()` уже истинно) has-reach не называет
/// субъектов; с дифом выключенным — называет (структурный could-flip).
fn second_card_names_no_subject(shared_index: bool) {
    let _g = TEST_LOCK.lock().unwrap();
    crate::style::has_cache::set_diff_enabled_for_test(true);
    let mut r = rig(CSS);
    let box_ = r.by_id("box");
    r.warm(box_);
    let card2 = r.new_card("c2");
    r.doc.append_child(box_, card2);
    let change = crate::style::NodeChange::ChildListEnds { front: false, back: true, edits: 1 };
    let roots = r.step(box_, change, shared_index);
    assert!(
        !roots.point.contains(&r.by_id("p1")) && roots.deep.is_empty(),
        "no flip: has-reach must name nothing, got deep={:?} point={:?}",
        roots.deep,
        roots.point
    );

    crate::style::has_cache::set_diff_enabled_for_test(false);
    let mut r = rig(CSS);
    let box_ = r.by_id("box");
    r.warm(box_);
    let card2 = r.new_card("c2");
    r.doc.append_child(box_, card2);
    let change = crate::style::NodeChange::ChildListEnds { front: false, back: true, edits: 1 };
    let roots = r.step(box_, change, shared_index);
    assert!(
        roots.point.contains(&r.by_id("p1")) || roots.deep.contains(&box_),
        "diff off: the structural question must name the subject, got deep={:?} point={:?}",
        roots.deep,
        roots.point
    );
    crate::style::has_cache::set_diff_enabled_for_test(true);
}

#[test]
fn a_second_matching_insert_flips_nothing_and_names_no_subject() {
    second_card_names_no_subject(false);
}

#[test]
fn the_shared_index_bridges_cloned_compounds_to_the_sheet() {
    second_card_names_no_subject(true);
}

#[test]
fn removing_the_only_matching_card_flips_the_subject() {
    let _g = TEST_LOCK.lock().unwrap();
    let mut r = rig(CSS);
    let box_ = r.by_id("box");
    r.warm(box_);
    let card = nth_child_element(&r.doc, box_, 1);
    assert!(r.doc.get(card).get_attr("class").is_some_and(|c| c.contains("card")), "fixture: the second child is the card");
    r.doc.detach(card);
    let change = crate::style::NodeChange::ChildListEnds { front: false, back: true, edits: 1 };
    let roots = r.step(box_, change, false);
    assert!(
        roots.point.contains(&r.by_id("p1")) || roots.deep.contains(&box_),
        "a real flip must name the subject, got deep={:?} point={:?}",
        roots.deep,
        roots.point
    );
}

#[test]
fn an_argument_attr_write_flips_the_subject() {
    let _g = TEST_LOCK.lock().unwrap();
    let mut r = rig(CSS_ATTR);
    let box_ = r.by_id("box");
    let child = nth_child_element(&r.doc, box_, 0);
    r.warm(box_);
    r.set_attr(child, "data-x", "1");
    let change = crate::style::NodeChange::AttrFrom { name: "data-x", old: "" };
    let roots = r.step(child, change, true);
    assert!(
        roots.point.contains(&r.by_id("p1")) || roots.deep.contains(&box_),
        "an attr the argument reads must flip on first write, got deep={:?} point={:?}",
        roots.deep,
        roots.point
    );
}

#[test]
fn an_argument_attr_write_that_changes_nothing_names_no_subject() {
    let _g = TEST_LOCK.lock().unwrap();
    let mut r = rig(CSS_ATTR);
    let box_ = r.by_id("box");
    let child = nth_child_element(&r.doc, box_, 0);
    r.warm(box_);
    r.set_attr(child, "data-x", "1");
    let change = crate::style::NodeChange::AttrFrom { name: "data-x", old: "" };
    r.step(child, change, true);
    // перезапись тем же значением — флипа нет
    r.set_attr(child, "data-x", "1");
    let change = crate::style::NodeChange::AttrFrom { name: "data-x", old: "1" };
    let roots = r.step(child, change, true);
    assert!(
        !roots.point.contains(&r.by_id("p1")) && roots.deep.is_empty(),
        "no flip on a same-value rewrite, got deep={:?} point={:?}",
        roots.deep,
        roots.point
    );
}

fn nth_child_element(doc: &lumen_dom::Document, parent: lumen_dom::NodeId, n: usize) -> lumen_dom::NodeId {
    doc.get(parent)
        .children
        .iter()
        .copied()
        .filter(|&c| matches!(doc.get(c).data, lumen_dom::NodeData::Element { .. }))
        .nth(n)
        .expect("fixture element")
}
