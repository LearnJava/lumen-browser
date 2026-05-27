//! Accessible name and description computation.
//!
//! Implements the WAI-ARIA Accessible Name and Description Computation algorithm
//! (ACCNAME-1.2 §4). Priority order for name:
//!   1. `aria-labelledby` — text content of referenced elements
//!   2. `aria-label`      — literal string
//!   3. Element-specific: `alt` (img), `value` (input[type=button/submit/reset]),
//!      `placeholder` (input text), `title`
//!   4. Text content (recursive, innerText-like)
//!
//! Description priority:
//!   1. `aria-describedby` — text of referenced elements
//!   2. `title` attribute (if not already used as name)

use lumen_dom::{Document, NodeData, NodeId};

/// Compute the accessible name for a DOM node (ACCNAME-1.2 §4.3).
pub fn compute_name(doc: &Document, node_id: NodeId) -> String {
    let node = doc.get(node_id);

    // 1. aria-labelledby — text of referenced elements by ID.
    if let Some(ids) = node.get_attr("aria-labelledby") {
        let text = collect_referenced_text(doc, ids);
        if !text.is_empty() {
            return text;
        }
    }

    // 2. aria-label — literal string.
    if let Some(label) = node.get_attr("aria-label") {
        let trimmed = label.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }

    // 3. Element-specific sources.
    if let Some(tag) = node.element_name().map(|n| n.local.as_str()) {
        match tag {
            "img" | "area" => {
                if let Some(alt) = node.get_attr("alt") {
                    return alt.to_owned();
                }
            }
            "input" => {
                // For form controls, check <label> association first (HTML-AAM §5.1).
                if let Some(label_text) = find_associated_label_text(doc, node_id) {
                    return label_text;
                }

                // value attribute for button-like inputs.
                if let Some(t) = node.input_type() {
                    use lumen_dom::InputType;
                    match t {
                        InputType::Button | InputType::Submit | InputType::Reset => {
                            if let Some(val) = node.get_attr("value") {
                                return val.to_owned();
                            }
                        }
                        InputType::Image => {
                            // <input type="image"> uses alt, like <img>
                            if let Some(alt) = node.get_attr("alt") {
                                return alt.to_owned();
                            }
                        }
                        _ => {}
                    }
                }
                // placeholder as last resort for text inputs.
                if let Some(ph) = node.get_attr("placeholder") {
                    return ph.to_owned();
                }
            }
            "textarea" => {
                // <textarea> checks label first.
                if let Some(label_text) = find_associated_label_text(doc, node_id) {
                    return label_text;
                }
                // placeholder as fallback.
                if let Some(ph) = node.get_attr("placeholder") {
                    return ph.to_owned();
                }
            }
            "select" => {
                // <select> checks label first.
                if let Some(label_text) = find_associated_label_text(doc, node_id) {
                    return label_text;
                }
            }
            "fieldset" => {
                // First <legend> child text.
                if let Some(legend_text) = first_child_text(doc, node_id, "legend") {
                    return legend_text;
                }
            }
            "table" => {
                // First <caption> child text.
                if let Some(cap_text) = first_child_text(doc, node_id, "caption") {
                    return cap_text;
                }
            }
            "figure" => {
                // First <figcaption> child text.
                if let Some(fc_text) = first_child_text(doc, node_id, "figcaption") {
                    return fc_text;
                }
            }
            "a" if node.get_attr("href").is_some() => {
                // Link text is the text content if href is present.
                // Links without href are not accessible links.
                let text_content = collect_text_content(doc, node_id);
                if !text_content.is_empty() {
                    return text_content;
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                // Heading text is the text content recursively.
                let text_content = collect_text_content(doc, node_id);
                if !text_content.is_empty() {
                    return text_content;
                }
            }
            "button" => {
                // Button text comes from text content recursively.
                // Special case: if button contains only an <img>, use img's alt.
                let children = &doc.get(node_id).children;
                if children.len() == 1 {
                    let child = doc.get(children[0]);
                    if child.element_name().is_some_and(|n| n.local.eq_ignore_ascii_case("img"))
                        && let Some(alt) = child.get_attr("alt")
                        && !alt.is_empty()
                    {
                        return alt.to_owned();
                    }
                }
                // Otherwise, use text content.
                let text_content = collect_text_content(doc, node_id);
                if !text_content.is_empty() {
                    return text_content;
                }
            }
            _ => {}
        }
    }

    // 4. Recursive text content (innerText-like). For most elements, this is the fallback.
    // For elements like <button>, this is the primary source of the name.
    let text_content = collect_text_content(doc, node_id);
    if !text_content.is_empty() {
        return text_content;
    }

    // 5. title attribute fallback.
    if let Some(title) = node.get_attr("title") {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }

    String::new()
}

/// Compute the accessible description for a DOM node (ACCNAME-1.2 §4.3.2).
///
/// Description is secondary to name: if title is already the name source,
/// it should not also be the description. This implements "naming takes
/// precedence" from the spec.
pub fn compute_description(doc: &Document, node_id: NodeId) -> String {
    let node = doc.get(node_id);

    // 1. aria-describedby — text of referenced elements.
    if let Some(ids) = node.get_attr("aria-describedby") {
        let text = collect_referenced_text(doc, ids);
        if !text.is_empty() {
            return text;
        }
    }

    // 2. title attribute — but only if it's not already the name source.
    // To avoid duplication, check whether title was used as the name.
    let name = compute_name(doc, node_id);
    if let Some(title) = node.get_attr("title") {
        let trimmed = title.trim();
        if !trimmed.is_empty() && trimmed != name {
            return trimmed.to_owned();
        }
    }

    String::new()
}

/// Collect text from elements referenced by a space-separated list of IDs.
/// Used for `aria-labelledby` and `aria-describedby`.
fn collect_referenced_text(doc: &Document, id_list: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for id in id_list.split_ascii_whitespace() {
        if let Some(node_id) = find_element_by_id(doc, id) {
            let text = collect_text_content(doc, node_id);
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join(" ")
}

/// Find a DOM element by its `id` attribute. Linear scan — O(n) but correct.
/// Phase 4 can add an ID-keyed HashMap to Document for O(1) lookup.
fn find_element_by_id(doc: &Document, target_id: &str) -> Option<NodeId> {
    // Pre-order traversal from root.
    let mut stack = vec![doc.root()];
    while let Some(node_id) = stack.pop() {
        let node = doc.get(node_id);
        if let NodeData::Element { .. } = &node.data
            && node.get_attr("id").is_some_and(|v| v == target_id)
        {
            return Some(node_id);
        }
        // Children in reverse so pop() yields forward order.
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    None
}

/// Return innerText equivalent: concatenate all Text nodes under `node_id`,
/// collapsing whitespace runs to a single space.
fn collect_text_content(doc: &Document, node_id: NodeId) -> String {
    let mut buf = String::new();
    collect_text_recursive(doc, node_id, &mut buf);
    // Collapse runs of whitespace into a single space, trim edges.
    let collapsed: String = buf
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    collapsed
}

fn collect_text_recursive(doc: &Document, node_id: NodeId, buf: &mut String) {
    let node = doc.get(node_id);
    match &node.data {
        NodeData::Text(t) => {
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(t);
        }
        NodeData::Element { .. } => {
            for &child in &node.children {
                collect_text_recursive(doc, child, buf);
            }
        }
        _ => {}
    }
}

/// Return the text content of the first direct child element with the given tag name.
fn first_child_text(doc: &Document, parent_id: NodeId, child_tag: &str) -> Option<String> {
    let parent = doc.get(parent_id);
    for &child_id in &parent.children {
        let child = doc.get(child_id);
        if child
            .element_name()
            .is_some_and(|n| n.local.eq_ignore_ascii_case(child_tag))
        {
            let text = collect_text_content(doc, child_id);
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

/// Find a `<label>` associated with a form control (input, select, textarea, etc.).
/// Returns the label's text content if found.
///
/// Association can be:
/// 1. Explicit: `<label for="input-id">Text</label>` — via `for` attribute
/// 2. Implicit: `<label><input></label>` — input is a descendant of label
fn find_associated_label_text(doc: &Document, control_id: NodeId) -> Option<String> {
    let control = doc.get(control_id);

    // 1. Explicit association via <label for="...">
    if let Some(control_id_str) = control.get_attr("id")
        && let Some(label_id) = find_element_by_tag_and_attr(doc, "label", "for", control_id_str)
    {
        let text = collect_text_content(doc, label_id);
        if !text.is_empty() {
            return Some(text);
        }
    }

    // 2. Implicit association: find a <label> ancestor that contains this control
    if let Some(parent_id) = control.parent {
        let mut ancestor = parent_id;
        loop {
            let ancestor_node = doc.get(ancestor);
            if ancestor_node
                .element_name()
                .is_some_and(|n| n.local.eq_ignore_ascii_case("label"))
            {
                let text = collect_text_content(doc, ancestor);
                if !text.is_empty() {
                    return Some(text);
                }
            }
            if let Some(next_parent) = ancestor_node.parent {
                ancestor = next_parent;
            } else {
                break;
            }
        }
    }

    None
}

/// Find an element by tag name and attribute value. Searches document in pre-order.
fn find_element_by_tag_and_attr(
    doc: &Document,
    tag: &str,
    attr: &str,
    value: &str,
) -> Option<NodeId> {
    let mut stack = vec![doc.root()];
    while let Some(node_id) = stack.pop() {
        let node = doc.get(node_id);
        if node
            .element_name()
            .is_some_and(|n| n.local.eq_ignore_ascii_case(tag))
            && node.get_attr(attr).is_some_and(|v| v == value)
        {
            return Some(node_id);
        }
        // Children in reverse for forward iteration.
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    None
}
