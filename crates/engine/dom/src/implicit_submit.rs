//! Implicit form submission (HTML LS §4.10.21.2): what pressing Enter in a
//! text field of a form does.
//!
//! Pure DOM question, answered here so the page path and the frame path of the
//! shell ask the same thing: given the field that took the key, which form is
//! submitted and through which button.

use super::{find_ancestor_form, Document, NodeId};

/// Outcome of Enter pressed in a form field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImplicitSubmission {
    /// The form to submit.
    pub form: NodeId,
    /// The form's default button, activated as if clicked; `None` when the form
    /// has no submit button and the field itself is the only one that blocks
    /// implicit submission (the form is submitted with no submitter).
    pub submitter: Option<NodeId>,
}

/// Input types whose Enter key triggers implicit submission — and, for a form
/// without a submit button, the types counted as "fields that block implicit
/// submission" (§4.10.21.2). A missing or unknown `type` is the Text state.
fn blocks_implicit_submission(doc: &Document, node: NodeId) -> bool {
    let n = doc.get(node);
    if !n.element_name().is_some_and(|q| q.local.eq_ignore_ascii_case("input")) {
        return false;
    }
    let ty = n.get_attr("type").unwrap_or("").to_ascii_lowercase();
    !matches!(
        ty.as_str(),
        "hidden" | "submit" | "image" | "reset" | "button" | "checkbox" | "radio" | "file"
            | "color" | "range"
    )
}

/// `true` for `<button>` of type submit (the default) and `<input type=submit|image>`.
fn is_submit_button(doc: &Document, node: NodeId) -> bool {
    let n = doc.get(node);
    let Some(q) = n.element_name() else { return false };
    let ty = n.get_attr("type").unwrap_or("").to_ascii_lowercase();
    if q.local.eq_ignore_ascii_case("button") {
        !matches!(ty.as_str(), "button" | "reset")
    } else if q.local.eq_ignore_ascii_case("input") {
        matches!(ty.as_str(), "submit" | "image")
    } else {
        false
    }
}

/// Walk `node`'s subtree in tree order, collecting the first submit button and
/// counting the fields that block implicit submission.
fn scan(doc: &Document, node: NodeId, button: &mut Option<NodeId>, blockers: &mut usize) {
    for &child in &doc.get(node).children {
        if button.is_none() && is_submit_button(doc, child) {
            *button = Some(child);
        }
        if blocks_implicit_submission(doc, child) {
            *blockers += 1;
        }
        scan(doc, child, button, blockers);
    }
}

/// What Enter pressed in `field` does (HTML LS §4.10.21.2), or `None` when it
/// submits nothing: `field` is not a text-like `<input>`, it has no form, the
/// form's default button is disabled, or the form has no submit button and
/// more than one field that blocks implicit submission.
///
/// The form owner is the nearest `<form>` ancestor, as everywhere else in the
/// engine (the `form` content attribute is not honoured yet).
pub fn implicit_submission(doc: &Document, field: NodeId) -> Option<ImplicitSubmission> {
    if !blocks_implicit_submission(doc, field) {
        return None;
    }
    let form = find_ancestor_form(doc, field)?;
    let mut button = None;
    let mut blockers = 0;
    scan(doc, form, &mut button, &mut blockers);
    match button {
        // The default button is the first one in tree order; a disabled one
        // cancels the submission instead of handing over to the next button.
        Some(b) if doc.get(b).get_attr("disabled").is_some() => None,
        Some(b) => Some(ImplicitSubmission { form, submitter: Some(b) }),
        None if blockers > 1 => None,
        None => Some(ImplicitSubmission { form, submitter: None }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attribute, NodeData, QualName};

    fn el(doc: &mut Document, parent: NodeId, tag: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = doc.create_element(QualName::html(tag));
        if let NodeData::Element { attrs: list, .. } = &mut doc.get_mut(id).data {
            for (k, v) in attrs {
                list.push(Attribute { name: QualName::html(*k), value: (*v).into() });
            }
        }
        doc.append_child(parent, id);
        id
    }

    fn form_doc() -> (Document, NodeId) {
        let mut doc = Document::new();
        let root = doc.root();
        let form = el(&mut doc, root, "form", &[]);
        (doc, form)
    }

    #[test]
    fn single_field_without_button_submits_form_itself() {
        let (mut doc, form) = form_doc();
        let q = el(&mut doc, form, "input", &[("name", "q")]);
        assert_eq!(
            implicit_submission(&doc, q),
            Some(ImplicitSubmission { form, submitter: None })
        );
    }

    #[test]
    fn two_text_fields_without_button_do_not_submit() {
        let (mut doc, form) = form_doc();
        let a = el(&mut doc, form, "input", &[("type", "text")]);
        el(&mut doc, form, "input", &[("type", "password")]);
        assert_eq!(implicit_submission(&doc, a), None);
    }

    #[test]
    fn non_blocking_inputs_do_not_count_as_second_field() {
        let (mut doc, form) = form_doc();
        let q = el(&mut doc, form, "input", &[("type", "search")]);
        el(&mut doc, form, "input", &[("type", "hidden")]);
        el(&mut doc, form, "input", &[("type", "checkbox")]);
        el(&mut doc, form, "textarea", &[]);
        assert_eq!(
            implicit_submission(&doc, q),
            Some(ImplicitSubmission { form, submitter: None })
        );
    }

    #[test]
    fn first_submit_button_is_the_submitter() {
        let (mut doc, form) = form_doc();
        let a = el(&mut doc, form, "input", &[]);
        el(&mut doc, form, "input", &[("type", "email")]);
        el(&mut doc, form, "button", &[("type", "button")]);
        let go = el(&mut doc, form, "button", &[]);
        el(&mut doc, form, "input", &[("type", "submit")]);
        assert_eq!(
            implicit_submission(&doc, a),
            Some(ImplicitSubmission { form, submitter: Some(go) })
        );
    }

    #[test]
    fn button_nested_in_a_wrapper_is_found() {
        let (mut doc, form) = form_doc();
        let a = el(&mut doc, form, "input", &[]);
        let div = el(&mut doc, form, "div", &[]);
        let go = el(&mut doc, div, "input", &[("type", "image")]);
        assert_eq!(
            implicit_submission(&doc, a),
            Some(ImplicitSubmission { form, submitter: Some(go) })
        );
    }

    #[test]
    fn disabled_default_button_cancels_submission() {
        let (mut doc, form) = form_doc();
        let a = el(&mut doc, form, "input", &[]);
        el(&mut doc, form, "button", &[("disabled", "")]);
        el(&mut doc, form, "button", &[]);
        assert_eq!(implicit_submission(&doc, a), None);
    }

    #[test]
    fn field_outside_a_form_or_of_other_type_submits_nothing() {
        let (mut doc, form) = form_doc();
        let root = doc.root();
        let loose = el(&mut doc, root, "input", &[]);
        let check = el(&mut doc, form, "input", &[("type", "checkbox")]);
        let area = el(&mut doc, form, "textarea", &[]);
        assert_eq!(implicit_submission(&doc, loose), None);
        assert_eq!(implicit_submission(&doc, check), None);
        assert_eq!(implicit_submission(&doc, area), None);
    }
}
