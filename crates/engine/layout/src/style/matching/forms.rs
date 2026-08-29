//! Динамические псевдо-классы: диспетчер [`matches_pseudo_class`] (структурные,
//! form-состояние, `:lang`/`:dir`/`:target`/`:has`, интерактивные `:hover`/
//! `:focus`/`:active` через thread-local контекст прохода, `:host`) и все
//! предикаты, которые он вызывает — form-состояние (`:disabled`/`:checked`/
//! `:required`/…), позиция в дереве (`:nth-child`/`:first-of-type`/…) и общие
//! DOM-traversal хелперы (`is_element`/`previous_element_sibling`/…).
//!
//! Перенесено батчем SPLIT-ST12 из `crates/engine/layout/src/style.rs`
//! (анкер `fn matches_pseudo_class`) без правок тел: изменена только
//! видимость (`matches_pseudo_class`/`matches_defined`/`is_element`/
//! `previous_element_sibling` нужны родителю `style::matching` и/или тестам)
//! и путь импортов. `is_self_or_ancestor` перенесена сюда же не по адресу, а
//! по теме — её единственный вызыватель это `matches_pseudo_class`
//! (`:hover`/`:active`/`:focus-within`), оставленный ST-10 в доноре именно
//! для этого батча.

use std::cell::Cell;

use lumen_css_parser::{Combinator, ComplexSelector, DirArg, PseudoClass};
use lumen_dom::{Document, NodeData, NodeId};

use crate::style::env::{ACTIVE_NID, FOCUS_NID, HOVER_NID};
use crate::style::SHADOW_HOST_SCOPE;

use super::matches_complex;

pub(in crate::style) fn matches_pseudo_class(p: &PseudoClass, doc: &Document, node: NodeId) -> bool {
    match p {
        PseudoClass::FirstChild => is_first_element_child(doc, node),
        PseudoClass::LastChild => is_last_element_child(doc, node),
        PseudoClass::OnlyChild => {
            is_first_element_child(doc, node) && is_last_element_child(doc, node)
        }
        PseudoClass::Empty => is_empty_element(doc, node),
        PseudoClass::Root => is_root_element(doc, node),
        PseudoClass::FirstOfType => is_first_of_type(doc, node),
        PseudoClass::LastOfType => is_last_of_type(doc, node),
        PseudoClass::OnlyOfType => is_first_of_type(doc, node) && is_last_of_type(doc, node),
        PseudoClass::NthChild(spec, of) => {
            match element_index_filtered(doc, node, false, of.as_deref()) {
                Some(i) => spec.matches(i),
                None => false,
            }
        }
        PseudoClass::NthLastChild(spec, of) => {
            match element_index_filtered(doc, node, true, of.as_deref()) {
                Some(i) => spec.matches(i),
                None => false,
            }
        }
        PseudoClass::NthOfType(spec) => match element_index_of_type(doc, node, false) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::NthLastOfType(spec) => match element_index_of_type(doc, node, true) {
            Some(i) => spec.matches(i),
            None => false,
        },
        PseudoClass::Not(list) => {
            // CSS Selectors L4 §5.4: матчит, если ни один селектор из списка
            // элементу не подходит. Внутри допустимы complex-селекторы и
            // nested `:not` — рекурсия идёт через `matches_complex`.
            !list.iter().any(|s| matches_complex(s, doc, node))
        }
        PseudoClass::Is(list) | PseudoClass::Where(list) => {
            // CSS4 §17: матчит, если матчит хоть один селектор из списка.
            // `:where(...)` отличается только тем, что contributes 0 specificity —
            // matching identical с `:is`.
            list.iter().any(|s| matches_complex(s, doc, node))
        }
        PseudoClass::Has(list) => {
            // CSS Selectors L4 §17.2: матчит элемент E, если хоть один из
            // relative selectors удовлетворён каким-то элементом в его
            // поддереве (для combinator None или Child) или sibling-цепочке
            // (для NextSibling / LaterSibling). Внутри matches_complex —
            // тот же recursive matcher с back-tracking, относительно
            // кандидата (а не E); кандидаты ищутся согласно combinator-у.
            list.iter().any(|rs| matches_relative(rs, doc, node))
        }
        PseudoClass::PlaceholderShown => matches_placeholder_shown(doc, node),
        PseudoClass::Required => matches_required(doc, node, true),
        PseudoClass::Optional => matches_required(doc, node, false),
        PseudoClass::ReadOnly => matches_read_only(doc, node),
        PseudoClass::ReadWrite => matches_read_write(doc, node),
        PseudoClass::Disabled => matches_disabled(doc, node, true),
        PseudoClass::Enabled => matches_disabled(doc, node, false),
        PseudoClass::Checked => matches_checked(doc, node),
        PseudoClass::Indeterminate => matches_indeterminate(doc, node),
        PseudoClass::Default => matches_default(doc, node),
        PseudoClass::Lang(tags) => matches_lang(doc, node, tags),
        PseudoClass::Dir(arg) => matches_dir(doc, node, *arg),
        PseudoClass::Link => matches_any_link(doc, node),
        // CSS Selectors L4 §6.2.3: `:visited` требует history-runtime
        // (`lumen-storage::History` + safe-history-API с privacy-ограничениями).
        // Phase 0 без runtime — всегда false; никакая ссылка не считается
        // посещённой. Это безопасный default (соответствует privacy-by-default
        // принципу проекта №1: ничего не утекает через стилизацию).
        PseudoClass::Visited => false,
        PseudoClass::AnyLink => matches_any_link(doc, node),
        // CSS Selectors L4 §4.2: `:scope` matches the document's root element
        // в author-CSS context (без runtime querySelector). Эквивалент `:root`.
        // Реальная разница появится при integration с DOM querySelector API
        // (P3 + JS-runtime) — пока что в layout-cascade оба ведут себя
        // одинаково.
        PseudoClass::Scope => is_root_element(doc, node),
        // CSS Selectors L4 §9.6: `:target` matches element с id равным
        // URL fragment-у (case-sensitive — HTML LS §3.2.6 делает `id`
        // case-sensitive, поэтому matcher не lowercase'ит). Без fragment-а
        // (`Document::target() == None`) — никакой element не матчит.
        // Phase 0: значение target_id выставляет shell-интеграция (P3) при
        // навигации; до её появления matcher всегда возвращает false.
        PseudoClass::Target => matches_target(doc, node),
        // CSS Selectors L4 §9.7: `:target-within` — element сам :target или
        // у него в поддереве есть :target-element. Short-circuit при
        // `Document::target() == None` — на странице без fragment-а никто
        // не матчит, walk поддерева не нужен.
        PseudoClass::TargetWithin => matches_target_within(doc, node),
        // CSS Selectors L4 §6.4.1, HTML LS §4.13.5 — `:defined` матчит
        // built-in HTML/SVG/MathML элементы и зарегистрированные custom
        // elements. Custom-element-имена по HTML LS §4.13.2 обязаны иметь
        // ASCII `-`; без registry в Phase 0 matcher использует это правило
        // как аппроксимацию: имя без `-` → built-in (defined); имя с `-` →
        // un-registered custom element (undefined). Когда P3 поднимет
        // registry, проверка станет `built-in || registry.has(name)`.
        PseudoClass::Defined => matches_defined(doc, node),
        // Fullscreen API §4.2 `:fullscreen` — runtime-only: top-layer
        // элементов, поднятых через `Element.requestFullscreen()`. JS API
        // реализован (p1-fullscreen-api); sentinel — `data-lumen-fullscreen`.
        // CSS: :fullscreen — P4: check doc.get_attr(node.id,"data-lumen-fullscreen").is_some()
        PseudoClass::Fullscreen => doc.get(node).get_attr("data-lumen-fullscreen").is_some(),
        // CSS Selectors L4 §16.5.2 `:modal` — `<dialog>` opened via
        // `showModal()`. JS sets `data-lumen-modal` sentinel; `show()` / author
        // attribute do not set it, so non-modal dialogs stay unmatched.
        PseudoClass::Modal => doc.get(node).get_attr("data-lumen-modal").is_some(),
        // HTML LS §6.12.2 `:popover-open` — popover в открытом состоянии
        // после `element.showPopover()` / клика по `popovertarget`.
        // Runtime-only: атрибут `popover` декларирует тип, но не открытое
        // состояние. Phase 0 без Popover API runtime — всегда `false`.
        PseudoClass::PopoverOpen => doc.get(node).get_attr("data-lumen-popover-open").is_some(),
        // CSS Selectors L4 §17.4 `:state(name)` — WHATWG HTML §4.13.2
        // `ElementInternals.states` (`CustomStateSet`). Runtime-only, same
        // sentinel-attribute pattern as `:fullscreen`/`:modal`: the JS shim
        // (`CustomStateSet.add`/`delete`/`clear`) reflects each active state
        // into a `data-lumen-state-<name>` attribute on the host element via
        // `_lumen_set_attr`/`_lumen_remove_attr` — layout never calls into
        // the JS engine during matching.
        PseudoClass::State(name) => doc
            .get(node)
            .get_attr(&format!("data-lumen-state-{name}"))
            .is_some(),
        // CSS Selectors L4 §11.4 time-dimensional pseudo-classes —
        // `:current` / `:past` / `:future` matches на active / elapsed /
        // upcoming моменты в timed-text потоке (WebVTT cue rendering при
        // воспроизведении видео/аудио). Runtime-only: нужна синхронизация с
        // media timeline и cue lifecycle. Phase 0 без timed-text runtime
        // все три всегда `false`.
        PseudoClass::Current => false,
        PseudoClass::Past => false,
        PseudoClass::Future => false,
        PseudoClass::InRange => matches_in_range(doc, node) == Some(true),
        PseudoClass::OutOfRange => matches_in_range(doc, node) == Some(false),
        PseudoClass::Valid => form_validity(doc, node) == Some(true),
        PseudoClass::Invalid => form_validity(doc, node) == Some(false),
        // Phase 0: без интерактивного состояния пользователя — всегда false.
        PseudoClass::UserValid | PseudoClass::UserInvalid => false,
        // ── Interactive pseudo-classes ────────────────────────────────────────────
        // State is set thread-locally by `set_interactive_state` before layout.
        // `:hover` — element under pointer, or its ancestors (CSS Selectors L4 §4.3).
        PseudoClass::Hover => {
            let hid = HOVER_NID.with(Cell::get);
            if hid == u32::MAX { return false; }
            is_self_or_ancestor(doc, node, NodeId::from_index(hid as usize))
        }
        // `:focus` — exact keyboard-focused element (no ancestor propagation).
        PseudoClass::Focus => {
            FOCUS_NID.with(Cell::get) == node.index() as u32
        }
        // `:active` — mouse-pressed element and its ancestors (CSS Selectors L4 §4.5).
        PseudoClass::Active => {
            let aid = ACTIVE_NID.with(Cell::get);
            if aid == u32::MAX { return false; }
            is_self_or_ancestor(doc, node, NodeId::from_index(aid as usize))
        }
        // `:focus-within` — element or any descendant has focus (CSS Selectors L4 §4.4.2).
        PseudoClass::FocusWithin => {
            let fid = FOCUS_NID.with(Cell::get);
            if fid == u32::MAX { return false; }
            is_self_or_ancestor(doc, node, NodeId::from_index(fid as usize))
        }
        // `:focus-visible` — Phase 0: identical to `:focus` (no keyboard-vs-mouse distinction yet).
        PseudoClass::FocusVisible => {
            FOCUS_NID.with(Cell::get) == node.index() as u32
        }
        PseudoClass::Unsupported(_) => false,
        // CSS Scoping L1 §6.1: `:host` matches the shadow host element, but ONLY
        // from within that host's own shadow-tree stylesheet. We model scope with
        // the `SHADOW_HOST_SCOPE` thread-local: it equals the host index only while
        // the shadow sheet of `node` is being matched. In document scope (MAX) or
        // when matching a *different* host's shadow sheet, `:host` never matches —
        // so a `:host` rule in the page's own `<style>` is a no-op (spec-correct).
        PseudoClass::Host(opt_list) => {
            if SHADOW_HOST_SCOPE.with(Cell::get) != node.index() as u32 {
                return false;
            }
            if !doc.is_shadow_host(node) {
                return false;
            }
            match opt_list {
                None => true,
                Some(list) => list.iter().any(|s| matches_complex(s, doc, node)),
            }
        }
    }
}

/// `:defined` matcher per CSS Selectors L4 §6.4.1 / HTML LS §4.13.5.
///
/// Текстовые / комментарные ноды псевдо-классам не подвергаются вообще
/// (Selector L4 §3.1 «selectors only apply to elements»), но selector
/// engine приходит сюда только для элементов — на всякий случай делаем
/// fast-fail на не-элемент.
pub(in crate::style) fn matches_defined(doc: &Document, node: NodeId) -> bool {
    let NodeData::Element { name, .. } = &doc.get(node).data else {
        return false;
    };
    // HTML LS §4.13.2 «Valid custom element name»: имя custom-element-а
    // обязано содержать дефис. Это единственная синтаксическая разница
    // между «built-in» и «custom». В Phase 0 без CustomElementRegistry
    // считаем все built-in defined, все custom-имена — undefined.
    !name.local.as_str().contains('-')
}

/// Default-значение `<input type>` — `text` (HTML5 §4.10.5.1.2). Возвращает
/// lower-case значение `type`-атрибута; пустая строка трактуется как `text`.
fn input_type_lower(doc: &Document, node: NodeId) -> String {
    let node_ref = doc.get(node);
    node_ref
        .get_attr("type")
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "text".to_string())
}

/// `<input>`-типы, к которым применимы `:read-only` / `:read-write` per HTML5
/// §4.16.4 «mutable input» — text-like (введение текста).
fn input_is_text_like(input_type: &str) -> bool {
    matches!(
        input_type,
        "text"
            | "search"
            | "url"
            | "tel"
            | "email"
            | "password"
            | "number"
            | "date"
            | "month"
            | "week"
            | "time"
            | "datetime-local"
    )
}

/// `<input>`-типы, к которым применим `required` per HTML5 §4.10.3 — text-like
/// + `checkbox` / `radio` / `file`.
fn input_supports_required(input_type: &str) -> bool {
    input_is_text_like(input_type)
        || matches!(input_type, "checkbox" | "radio" | "file")
}

/// CSS Selectors L4 §15.4 / HTML5 §4.10.3 `:required` / `:optional`.
/// `want_required = true` → `:required`, иначе `:optional`. Возвращает true
/// только для form control-ов, к которым применим атрибут `required`.
///
/// Применимо: `<select>`, `<textarea>`, и `<input>` text-like / checkbox /
/// radio / file. Прочие элементы (`<input type=hidden>`, `<button>`, `<div>`)
/// не матчатся ни одним из двух.
fn matches_required(doc: &Document, node: NodeId, want_required: bool) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    let applies = match tag {
        "select" | "textarea" => true,
        "input" => input_supports_required(&input_type_lower(doc, node)),
        _ => false,
    };
    if !applies {
        return false;
    }
    let has_required = node_ref.get_attr("required").is_some();
    has_required == want_required
}

/// CSS Selectors L4 §15.5 / HTML5 §4.16.4 `:read-write` — «mutable» form
/// control или `contenteditable`-элемент.
///
/// True для:
///   - `<input>` text-like type БЕЗ `readonly` и БЕЗ `disabled`;
///   - `<textarea>` БЕЗ `readonly` и БЕЗ `disabled`;
///   - любого элемента с эффективным `contenteditable="true"` (включая
///     наследование от ancestor — `contenteditable=""` тоже считается true).
///
/// Прочие элементы — false (и матчат `:read-only`).
fn matches_read_write(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    let is_form_mutable = match tag {
        "input" => {
            input_is_text_like(&input_type_lower(doc, node))
                && node_ref.get_attr("readonly").is_none()
                && node_ref.get_attr("disabled").is_none()
        }
        "textarea" => {
            node_ref.get_attr("readonly").is_none()
                && node_ref.get_attr("disabled").is_none()
        }
        _ => false,
    };
    if is_form_mutable {
        return true;
    }
    is_effectively_contenteditable(doc, node)
}

/// CSS Selectors L4 §15.5 / HTML5 §4.16.4 `:read-only` — «not mutable».
///
/// Per spec: «matches all other HTML elements» — то есть все Element-ы, не
/// попадающие под `:read-write`. Не Element-ы (Text / Comment / Document) не
/// матчатся ничем.
fn matches_read_only(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    if !matches!(node_ref.data, NodeData::Element { .. }) {
        return false;
    }
    !matches_read_write(doc, node)
}

/// Эффективное значение `contenteditable` с наследованием от ancestor-ов.
/// `contenteditable="true"` или `contenteditable=""` (пустая строка) → true;
/// `contenteditable="false"` → false (и обрывает наследование); отсутствие
/// атрибута на узле — смотрим выше.
fn is_effectively_contenteditable(doc: &Document, node: NodeId) -> bool {
    let mut cur = Some(node);
    while let Some(n) = cur {
        let node_ref = doc.get(n);
        if let NodeData::Element { .. } = node_ref.data
            && let Some(v) = node_ref.get_attr("contenteditable")
        {
            let lower = v.trim().to_ascii_lowercase();
            if lower.is_empty() || lower == "true" {
                return true;
            }
            if lower == "false" {
                return false;
            }
        }
        cur = node_ref.parent;
    }
    false
}

/// HTML5 §4.10.19.2 «can be disabled»-элементы — `<button>`, `<input>`,
/// `<select>`, `<textarea>`, `<optgroup>`, `<option>`, `<fieldset>`.
fn is_disableable_form_control(tag: &str) -> bool {
    matches!(
        tag,
        "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
    )
}

/// CSS Selectors L4 §14.2 / HTML5 §4.10.19.2 `:disabled` / `:enabled`.
/// `want_disabled = true` → `:disabled`, иначе `:enabled`.
///
/// Элемент считается disabled, если:
///   - применим к `:disabled` per `is_disableable_form_control` И;
///   - либо у него самого есть атрибут `disabled`;
///   - либо у `<option>` ancestor-`<optgroup>` имеет `disabled` (HTML5 §4.10.10);
///   - либо элемент находится внутри `<fieldset disabled>` И НЕ внутри
///     первого `<legend>`-ребёнка этого fieldset (HTML5 §4.10.16).
///     `<fieldset>` сам disabled только по собственному атрибуту, не от
///     ancestor-fieldset.
///
/// Прочие элементы (`<div>`, `<p>`, и т.д.) — не матчат ни `:disabled`, ни
/// `:enabled`.
fn matches_disabled(doc: &Document, node: NodeId, want_disabled: bool) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if !is_disableable_form_control(tag) {
        return false;
    }
    let actually_disabled = is_actually_disabled(doc, node, tag);
    actually_disabled == want_disabled
}

fn is_actually_disabled(doc: &Document, node: NodeId, tag: &str) -> bool {
    let node_ref = doc.get(node);
    if node_ref.get_attr("disabled").is_some() {
        return true;
    }
    // `<option>` наследует disabled от непосредственного `<optgroup>`-родителя
    // (HTML5 §4.10.10): «An option element is disabled if its disabled attribute
    // is set or if it is a child of an optgroup element whose disabled attribute
    // is set».
    if tag == "option"
        && let Some(p) = node_ref.parent
    {
        let p_ref = doc.get(p);
        if let NodeData::Element { name: pname, .. } = &p_ref.data
            && pname.local.as_str() == "optgroup"
            && p_ref.get_attr("disabled").is_some()
        {
            return true;
        }
    }
    // `<fieldset>` сам disabled только по собственному атрибуту; ancestor-walk
    // для него не нужен.
    if tag == "fieldset" {
        return false;
    }
    // Form control внутри `<fieldset disabled>` — disabled, кроме случая, когда
    // он лежит в первом `<legend>`-ребёнке этого fieldset (HTML5 §4.10.16).
    let mut child = node;
    let mut cur = node_ref.parent;
    while let Some(p) = cur {
        let p_ref = doc.get(p);
        if let NodeData::Element { name: pname, .. } = &p_ref.data
            && pname.local.as_str() == "fieldset"
            && p_ref.get_attr("disabled").is_some()
            && !is_descendant_of_first_legend_child(doc, p, child)
        {
            return true;
        }
        child = p;
        cur = p_ref.parent;
    }
    false
}

/// True, если `descendant_chain_start` — это сам first-`<legend>`-ребёнок
/// `fieldset` или лежит в его поддереве. Для проверки достаточно посмотреть на
/// `child` — тот узел, через которого мы дошли до fieldset; если он же —
/// первый element-child `<legend>`, то вся ветка живёт под legend.
fn is_descendant_of_first_legend_child(
    doc: &Document,
    fieldset: NodeId,
    child_on_path: NodeId,
) -> bool {
    let first_legend = doc
        .get(fieldset)
        .children
        .iter()
        .copied()
        .find(|&c| is_element(doc, c))
        .filter(|&c| {
            let c_ref = doc.get(c);
            matches!(&c_ref.data, NodeData::Element { name, .. } if name.local.as_str() == "legend")
        });
    matches!(first_legend, Some(l) if l == child_on_path)
}

/// CSS Selectors L4 §15.1 `:placeholder-shown` — true для form-control,
/// у которого есть непустой `placeholder`-атрибут И пустое текущее значение.
///
/// Текущее значение берётся из [`Document::control_value`]: набранный текст и
/// присвоенный скриптом `el.value` прячут placeholder ровно так же, как
/// author-объявленный `value`-атрибут (BUG-441).
fn matches_placeholder_shown(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if tag != "input" && tag != "textarea" {
        return false;
    }
    let Some(placeholder) = node_ref.get_attr("placeholder") else {
        return false;
    };
    if placeholder.trim().is_empty() {
        return false;
    }
    // Непустое текущее значение → контент уже задан, placeholder скрыт.
    // Набранное/присвоенное значение перекрывает дефолт целиком: пустой dirty
    // value возвращает placeholder даже у `<textarea>` с author-текстом.
    if let Some(dirty) = doc.dirty_value(node) {
        return dirty.is_empty();
    }
    // Дефолтная ветка — прежнее правило: у `<input>` это `value`-атрибут, у
    // `<textarea>` — текстовые дети (whitespace-only контентом не считается).
    if tag == "textarea" {
        return !has_non_whitespace_text(doc, node);
    }
    node_ref.get_attr("value").unwrap_or("").is_empty()
}

/// `:checked` (CSS Selectors L4 §10.1). Pure attribute-based matcher без
/// runtime form-state:
/// - `<input type=checkbox|radio>` с атрибутом `checked` (значение атрибута
///   не имеет значения — спецификация трактует наличие как true);
/// - `<option>` с атрибутом `selected`.
///
/// Динамически переключённый через клик/JS checkbox не отражается в
/// DOM-атрибутах и здесь не учитывается — Phase 0 без form-state runtime.
fn matches_checked(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    match name.local.as_str() {
        "input" => {
            let t = input_type_lower(doc, node);
            if t != "checkbox" && t != "radio" {
                return false;
            }
            node_ref.get_attr("checked").is_some()
        }
        "option" => node_ref.get_attr("selected").is_some(),
        _ => false,
    }
}

/// `:indeterminate` (CSS Selectors L4 §10.2, HTML5 §4.16.3 + §4.10.18.4).
/// Применяется к:
/// - `<input type=checkbox>` с DOM-флагом indeterminate (Phase 0: всегда
///   `false` — флаг существует только через JS `.indeterminate = true`,
///   которого пока нет);
/// - `<input type=radio>` в группе (одинаковый `name` внутри ближайшей
///   form-owner-области) без ни одного checked-радио. Если радио без `name`,
///   группа = только сам элемент — тогда indeterminate ≡ нет `checked`;
/// - `<progress>` без атрибута `value` (indeterminate progress per HTML5).
fn matches_indeterminate(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    match name.local.as_str() {
        "input" => {
            let t = input_type_lower(doc, node);
            if t == "radio" {
                // Найти ближайший <form>-предок; если нет — корень документа.
                let scope = nearest_form_or_root(doc, node);
                let radio_name = node_ref.get_attr("name").map(|s| s.to_string());
                !any_descendant(doc, scope, |n| {
                    if !is_element(doc, n) {
                        return false;
                    }
                    let other = doc.get(n);
                    let NodeData::Element { name: n2, .. } = &other.data else {
                        return false;
                    };
                    if n2.local.as_str() != "input" {
                        return false;
                    }
                    let t2 = input_type_lower(doc, n);
                    if t2 != "radio" {
                        return false;
                    }
                    // Радио считается членом той же группы если name совпадает
                    // (или оба отсутствуют — узкая группа из одного элемента).
                    let n2_name = other.get_attr("name").map(|s| s.to_string());
                    if n2_name != radio_name {
                        return false;
                    }
                    other.get_attr("checked").is_some()
                })
            } else {
                // Phase 0: checkbox indeterminate выставляется только через
                // JS — DOM не выражает этого. Всегда false.
                false
            }
        }
        "progress" => node_ref.get_attr("value").is_none(),
        _ => false,
    }
}

/// `:default` (CSS Selectors L4 §10.4, HTML5 §4.16.3) — «по-умолчанию
/// активный» form control:
/// - `<option>` с атрибутом `selected`;
/// - checkbox/radio с атрибутом `checked`;
/// - default submit-button формы — первая в DOM-порядке формы
///   `<button type=submit>` / `<input type=submit|image>`. `type=submit` —
///   default для `<button>` (HTML5 §4.10.8) и для `<input>` без `type` это
///   `text`, поэтому submit-button обязан иметь `type=submit`.
fn matches_default(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    match tag {
        "option" => node_ref.get_attr("selected").is_some(),
        "input" => {
            let t = input_type_lower(doc, node);
            if (t == "checkbox" || t == "radio") && node_ref.get_attr("checked").is_some() {
                return true;
            }
            if t == "submit" || t == "image" {
                return is_default_submit_button(doc, node);
            }
            false
        }
        "button" => {
            // default-type для <button> = submit (HTML5 §4.10.8).
            let t = node_ref
                .get_attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "submit".to_string());
            if t != "submit" {
                return false;
            }
            is_default_submit_button(doc, node)
        }
        _ => false,
    }
}

/// Default submit-button формы — первая submit-кнопка в DOM-порядке внутри
/// ближайшего `<form>`-предка (HTML5 §4.10.22.3 «implicit submission»).
/// Если предка `<form>` нет, кнопка не form-owner-связана и не считается
/// default.
fn is_default_submit_button(doc: &Document, node: NodeId) -> bool {
    let Some(form) = nearest_form(doc, node) else {
        return false;
    };
    let mut found: Option<NodeId> = None;
    walk_first_submit(doc, form, &mut found);
    found == Some(node)
}

/// Pre-order обход поддерева form в поиске первой submit-кнопки. Сохраняет
/// результат в `found` и останавливается раньше через короткое замыкание
/// `is_some()` на ранних уровнях.
fn walk_first_submit(doc: &Document, scope: NodeId, found: &mut Option<NodeId>) {
    if found.is_some() {
        return;
    }
    for &child in &doc.get(scope).children {
        if found.is_some() {
            return;
        }
        if !is_element(doc, child) {
            continue;
        }
        let NodeData::Element { name, .. } = &doc.get(child).data else {
            continue;
        };
        let tag = name.local.as_str();
        if tag == "input" {
            let t = input_type_lower(doc, child);
            if t == "submit" || t == "image" {
                *found = Some(child);
                return;
            }
        } else if tag == "button" {
            let t = doc
                .get(child)
                .get_attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "submit".to_string());
            if t == "submit" {
                *found = Some(child);
                return;
            }
        }
        walk_first_submit(doc, child, found);
    }
}

/// Ближайший `<form>`-предок (или сам node, если он `<form>`). None — нет.
fn nearest_form(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { name, .. } = &doc.get(n).data
            && name.local.as_str() == "form"
        {
            return Some(n);
        }
        cur = doc.get(n).parent;
    }
    None
}

/// Ближайший `<form>`-предок или корень документа — scope-для-обхода
/// radio-группы. Возвращает корень документа если предка `<form>` нет.
fn nearest_form_or_root(doc: &Document, node: NodeId) -> NodeId {
    nearest_form(doc, node).unwrap_or_else(|| doc.root())
}

/// `:lang(<tag>#)` (CSS Selectors L4 §11). Элемент матчит, если его
/// content-language matches хотя бы один из tag-ов в списке по RFC 4647
/// §3.3.1 «basic filtering»: range matches tag, если range — exact equal
/// или range — proper prefix tag с границей по `-`. То есть `:lang(en)`
/// matches `lang="en"`, `lang="en-US"`, `lang="en-Latn-GB"`, но не
/// `lang="english"` и не `lang="fr-en"` (последний — `fr` + `en` — `en`
/// здесь регион/вариант, не language).
///
/// Content-language определяется через ближайший `lang` или `xml:lang`
/// атрибут вверх по дереву (HTML5 §3.2.6 «inheritance»; xml:lang —
/// исторически из XHTML, до сих пор используется в реальных страницах).
/// Если ни один ancestor не имеет `lang`, элемент не имеет языка и не
/// матчит ни один tag — кроме пустого `*` (Selectors L4 расширение пока
/// не поддерживается).
fn matches_lang(doc: &Document, node: NodeId, tags: &[String]) -> bool {
    let Some(content_lang) = element_lang(doc, node) else {
        return false;
    };
    let content_lc = content_lang.to_ascii_lowercase();
    tags.iter().any(|range| lang_range_matches(range, &content_lc))
}

/// Определяет content-language элемента, walking up ancestors. Сначала
/// `lang`, потом `xml:lang` на том же узле; затем родитель, и так далее.
/// Возвращает None если ни у кого нет атрибута либо найденное значение —
/// пустая строка (HTML5: `lang=""` — «явно неизвестен», не наследует от
/// предков — Phase 0 трактует как «нет языка»).
fn element_lang(doc: &Document, node: NodeId) -> Option<String> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { .. } = &doc.get(n).data {
            let nr = doc.get(n);
            if let Some(v) = nr.get_attr("lang") {
                return if v.is_empty() { None } else { Some(v.to_string()) };
            }
            if let Some(v) = nr.get_attr("xml:lang") {
                return if v.is_empty() { None } else { Some(v.to_string()) };
            }
        }
        cur = doc.get(n).parent;
    }
    None
}

/// RFC 4647 §3.3.1 «basic filtering»: language range matches language tag,
/// если range — case-insensitive prefix tag с границей по `-` или концом
/// строки. Обе стороны уже ожидаются в lowercase.
fn lang_range_matches(range_lc: &str, tag_lc: &str) -> bool {
    if range_lc == tag_lc {
        return true;
    }
    if let Some(rest) = tag_lc.strip_prefix(range_lc) {
        return rest.starts_with('-');
    }
    false
}

/// `:any-link` / `:link` (CSS Selectors L4 §6.2.1 / §6.2.2, HTML5 §4.6).
/// Hyperlinks в HTML: `<a>`, `<area>`, `<link>` элементы с **непустым**
/// `href`-атрибутом (HTML5 §4.6.1 — hyperlink требует non-empty href; пустой
/// href трактуется как ссылка на текущий документ и формально валиден, но
/// все mainstream браузеры считают такой элемент hyperlink-ом — мы тоже).
/// Spec различает hyperlink (`href` присутствует) от non-hyperlink (no href),
/// последний не матчит ни `:link`, ни `:visited`, ни `:any-link`.
fn matches_any_link(doc: &Document, node: NodeId) -> bool {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return false;
    };
    let tag = name.local.as_str();
    if !matches!(tag, "a" | "area" | "link") {
        return false;
    }
    node_ref.get_attr("href").is_some()
}

/// `:target` matcher (CSS Selectors L4 §9.6). Возвращает true, если у элемента
/// есть `id`-атрибут, равный текущему `Document::target()` (URL fragment без
/// `:in-range` / `:out-of-range` (CSS Selectors L4 §14.5, HTML5 §4.10.21.4).
///
/// Возвращает `Some(true)` если value в [min, max], `Some(false)` если вне,
/// `None` если у элемента нет range-limitations или нет displayed value.
/// Phase 0: поддерживаются только `type=number` и `type=range`.
fn matches_in_range(doc: &Document, node: NodeId) -> Option<bool> {
    let node_ref = doc.get(node);
    let NodeData::Element { name, .. } = &node_ref.data else {
        return None;
    };
    if name.local.as_str() != "input" {
        return None;
    }
    let t = input_type_lower(doc, node);
    let supports_numeric = matches!(t.as_str(), "number" | "range");
    if !supports_numeric {
        return None;
    }

    let min_attr = node_ref.get_attr("min").and_then(parse_html_number);
    let max_attr = node_ref.get_attr("max").and_then(parse_html_number);

    let (min, max) = match t.as_str() {
        "range" => (min_attr.unwrap_or(0.0), max_attr.unwrap_or(100.0)),
        _ => {
            if min_attr.is_none() && max_attr.is_none() {
                return None;
            }
            (min_attr.unwrap_or(f64::NEG_INFINITY), max_attr.unwrap_or(f64::INFINITY))
        }
    };

    // Текущее значение контрола, а не `value`-атрибут: набранное/присвоенное
    // число решает, попадает ли поле в диапазон (BUG-441).
    let value = match parse_html_number(doc.control_value(node).as_ref()) {
        Some(v) => v,
        None => {
            if t == "range" {
                // Spec §4.10.5.1.13: default value = min + (max-min)/2, clamped.
                let mid = min + (max - min) / 2.0;
                mid.clamp(min, max)
            } else {
                return None;
            }
        }
    };

    Some(value >= min && value <= max)
}

/// `:valid` / `:invalid` (CSS Selectors L4 §14.1, HTML5 §4.10.21).
///
/// Делегирует в `lumen_dom::element_validity` — единый источник истины для
/// constraint validation. `None` — элемент не является кандидатом.
fn form_validity(doc: &Document, node: NodeId) -> Option<bool> {
    lumen_dom::element_validity(doc, node).map(|vs| vs.valid())
}

/// Парсит HTML5 «valid floating-point number» (§2.5.5).
/// Отбрасывает leading `+`, NaN и ±∞ (не допускаются spec-ом).
fn parse_html_number(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.starts_with('+') {
        return None;
    }
    let v: f64 = trimmed.parse().ok()?;
    if v.is_finite() { Some(v) } else { None }
}

/// `#`). Comparison case-sensitive — HTML id case-sensitive per HTML LS §3.2.6.
/// Текстовые узлы и не-element-узлы не матчат.
fn matches_target(doc: &Document, node: NodeId) -> bool {
    let Some(target) = doc.target() else {
        return false;
    };
    let node_ref = doc.get(node);
    if !matches!(&node_ref.data, NodeData::Element { .. }) {
        return false;
    }
    node_ref.get_attr("id") == Some(target)
}

/// `:target-within` matcher (CSS Selectors L4 §9.7). Element matches if it
/// itself is `:target`, OR has any descendant element matching `:target`.
/// Short-circuits на `Document::target() == None` (нет fragment-а — никто не
/// матчит, сэкономим обход поддерева).
fn matches_target_within(doc: &Document, node: NodeId) -> bool {
    let Some(target) = doc.target() else {
        return false;
    };
    if !is_element(doc, node) {
        return false;
    }
    if doc.get(node).get_attr("id") == Some(target) {
        return true;
    }
    any_descendant(doc, node, |n| doc.get(n).get_attr("id") == Some(target))
}

/// `:dir(ltr|rtl)` (CSS Selectors L4 §13.2). Матчит элемент с
/// соответствующей directionality, определяемой через `dir`-атрибут
/// (с inherited fallback от ближайшего ancestor-а). При отсутствии
/// `dir` нигде в цепочке — default `ltr` (HTML5 §3.2.6.1).
fn matches_dir(doc: &Document, node: NodeId, want: DirArg) -> bool {
    element_directionality(doc, node) == want
}

/// Computes content-directionality элемента по HTML5 §3.2.6.1
/// «directionality»: значение `dir`-атрибута самого элемента, либо
/// унаследовано от ближайшего ancestor с `dir`-атрибутом. Default `ltr`.
///
/// Phase 0 не реализует real auto-direction (UAX #9 first-strong scan по
/// текстовому содержимому для `<bdi>` и `dir="auto"`) — оба трактуются
/// как `ltr`, что соответствует поведению типичных страниц на латинице.
/// Real bidi откладывается до layout-bidi движка (см. lumen-layout `Отложено`).
fn element_directionality(doc: &Document, node: NodeId) -> DirArg {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if let NodeData::Element { .. } = &doc.get(n).data
            && let Some(v) = doc.get(n).get_attr("dir")
        {
            return match v.trim().to_ascii_lowercase().as_str() {
                "ltr" => DirArg::Ltr,
                "rtl" => DirArg::Rtl,
                // `auto` и любое другое значение — Phase 0 fallback to ltr;
                // продолжаем walking up НЕ нужно: spec говорит, что
                // `dir` атрибут на самом элементе финализирует
                // directionality (`auto` тоже считается «явным»).
                _ => DirArg::Ltr,
            };
        }
        cur = doc.get(n).parent;
    }
    DirArg::Ltr
}

/// Проверка: у узла есть хоть один text-ребёнок с непустым содержимым
/// (после whitespace-trim). Нужно для `<textarea>` чьё «значение» — это
/// его текстовый контент в DOM (HTML5 §4.10.11), а не `value`-атрибут.
fn has_non_whitespace_text(doc: &Document, node: NodeId) -> bool {
    for &child in &doc.get(node).children {
        if let NodeData::Text(t) = &doc.get(child).data
            && !t.trim().is_empty()
        {
            return true;
        }
    }
    false
}

/// Проверяет, что хоть один кандидат относительно `scope` (в зависимости от
/// combinator-а) удовлетворяет внутреннему selector-у.
fn matches_relative(rs: &lumen_css_parser::RelativeSelector, doc: &Document, scope: NodeId) -> bool {
    match rs.combinator {
        // Implicit descendant — обходим всё поддерево scope.
        None => any_descendant(doc, scope, |n| matches_complex(&rs.selector, doc, n)),
        Some(Combinator::Child) => {
            // Прямые element-children scope.
            doc.get(scope).children.iter().any(|&c| {
                is_element(doc, c) && matches_complex(&rs.selector, doc, c)
            })
        }
        Some(Combinator::NextSibling) => {
            // Прямой следующий element-sibling.
            next_element_sibling(doc, scope)
                .map(|n| matches_complex(&rs.selector, doc, n))
                .unwrap_or(false)
        }
        Some(Combinator::LaterSibling) => {
            // Любой последующий element-sibling.
            let mut cur = next_element_sibling(doc, scope);
            while let Some(n) = cur {
                if matches_complex(&rs.selector, doc, n) {
                    return true;
                }
                cur = next_element_sibling(doc, n);
            }
            false
        }
        // Descendant как explicit combinator — то же что None.
        Some(Combinator::Descendant) => {
            any_descendant(doc, scope, |n| matches_complex(&rs.selector, doc, n))
        }
    }
}

/// True если хоть один element-descendant `root` удовлетворяет `pred`. Сам
/// `root` не проверяется — только потомки (по spec :has() ищет среди
/// descendants, не включая E).
fn any_descendant<F: Fn(NodeId) -> bool>(doc: &Document, root: NodeId, pred: F) -> bool {
    fn walk<F: Fn(NodeId) -> bool>(doc: &Document, n: NodeId, pred: &F) -> bool {
        for &c in &doc.get(n).children {
            if is_element(doc, c) && pred(c) {
                return true;
            }
            if walk(doc, c, pred) {
                return true;
            }
        }
        false
    }
    walk(doc, root, &pred)
}

fn next_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let idx = siblings.iter().position(|&id| id == node)?;
    siblings[idx + 1..].iter().copied().find(|&id| is_element(doc, id))
}

/// 1-based индекс элемента среди element-sibling-ов. Если `from_end` —
/// считаем с конца. None — если узел не элемент или нет родителя.
fn element_index(doc: &Document, node: NodeId, from_end: bool) -> Option<i32> {
    if !is_element(doc, node) {
        return None;
    }
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        if !is_element(doc, id) {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

/// 1-based индекс элемента среди sibling-ов, удовлетворяющих опциональному
/// `of <selector-list>` фильтру (CSS Selectors L4 §6.6.5.1). При `of=None`
/// эквивалент `element_index` (все element-sibling-ы). При `of=Some(list)`:
/// сначала проверяем, что сам узел матчит хотя бы один из селекторов
/// списка — иначе `:nth-child(... of S)` не применим, возвращаем None;
/// затем считаем index среди siblings, удовлетворяющих тому же list-у.
fn element_index_filtered(
    doc: &Document,
    node: NodeId,
    from_end: bool,
    of: Option<&[ComplexSelector]>,
) -> Option<i32> {
    let Some(list) = of else {
        return element_index(doc, node, from_end);
    };
    if !is_element(doc, node) {
        return None;
    }
    // Сам элемент должен матчить хотя бы один селектор list-а — иначе
    // `:nth-child(an+b of S)` к нему вообще не применяется.
    if !list.iter().any(|s| matches_complex(s, doc, node)) {
        return None;
    }
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        if !is_element(doc, id) {
            continue;
        }
        if !list.iter().any(|s| matches_complex(s, doc, id)) {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

/// 1-based индекс элемента среди sibling-ов **того же тега**.
fn element_index_of_type(doc: &Document, node: NodeId, from_end: bool) -> Option<i32> {
    let self_name = match &doc.get(node).data {
        NodeData::Element { name, .. } => name,
        _ => return None,
    };
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let mut index: i32 = 0;
    let iter: Box<dyn Iterator<Item = &NodeId>> = if from_end {
        Box::new(siblings.iter().rev())
    } else {
        Box::new(siblings.iter())
    };
    for &id in iter {
        let same_type = matches!(
            &doc.get(id).data,
            NodeData::Element { name, .. } if name == self_name
        );
        if !same_type {
            continue;
        }
        index += 1;
        if id == node {
            return Some(index);
        }
    }
    None
}

fn is_first_of_type(doc: &Document, node: NodeId) -> bool {
    element_index_of_type(doc, node, false) == Some(1)
}

fn is_last_of_type(doc: &Document, node: NodeId) -> bool {
    element_index_of_type(doc, node, true) == Some(1)
}

// ──────────────── DOM-traversal хелперы ────────────────

pub(in crate::style) fn is_element(doc: &Document, node: NodeId) -> bool {
    matches!(doc.get(node).data, NodeData::Element { .. })
}

pub(in crate::style) fn previous_element_sibling(doc: &Document, node: NodeId) -> Option<NodeId> {
    let parent = doc.get(node).parent?;
    let siblings = &doc.get(parent).children;
    let idx = siblings.iter().position(|&id| id == node)?;
    siblings[..idx]
        .iter()
        .rev()
        .copied()
        .find(|&id| is_element(doc, id))
}

fn is_first_element_child(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    let siblings = &doc.get(parent).children;
    siblings
        .iter()
        .copied()
        .find(|&id| is_element(doc, id))
        == Some(node)
}

fn is_last_element_child(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    let siblings = &doc.get(parent).children;
    siblings
        .iter()
        .rev()
        .copied()
        .find(|&id| is_element(doc, id))
        == Some(node)
}

fn is_empty_element(doc: &Document, node: NodeId) -> bool {
    // `:empty` — нет ни элементов-детей, ни текстовых узлов с непустым контентом.
    doc.get(node).children.iter().all(|&cid| {
        matches!(
            doc.get(cid).data,
            NodeData::Comment(_) | NodeData::Doctype { .. }
        ) || matches!(&doc.get(cid).data, NodeData::Text(t) if t.is_empty())
    })
}

fn is_root_element(doc: &Document, node: NodeId) -> bool {
    let Some(parent) = doc.get(node).parent else {
        return false;
    };
    matches!(doc.get(parent).data, NodeData::Document)
}

/// Returns `true` if `ancestor` is `node` itself, or a proper ancestor of `node` in the tree.
fn is_self_or_ancestor(doc: &Document, ancestor: NodeId, node: NodeId) -> bool {
    if ancestor == node { return true; }
    let mut cur = doc.get(node).parent;
    while let Some(parent_id) = cur {
        if parent_id == ancestor { return true; }
        if parent_id == doc.root() { break; }
        cur = doc.get(parent_id).parent;
    }
    false
}
