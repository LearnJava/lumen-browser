// CSS parse-gate + codegen-identifier helpers.
//
// Shared verbatim between the crate (`mod gate;`, for unit tests) and
// `build.rs` (`include!("src/gate.rs")`, for the compile-time gate itself) —
// see CC-3 (docs/tasks/p1-css-chrome.md). Keep this file self-contained (own
// `use`s only): `build.rs` textually inlines it at crate-root scope alongside
// its own top-level `use`s, so a duplicate import here would be a compile
// error there. Plain `//` comments only — `include!` splices this mid-file,
// where a leading `//!` inner doc comment is a syntax error (E0753).

use lumen_css_parser::{
    ComplexSelector, CompoundSelector, Declaration, PseudoClass, PseudoElementKind, Rule,
    SimpleSelector, Stylesheet, SUPPORTED_PROPERTIES,
};

/// CSS properties the reference uses that `apply_declaration` deliberately
/// parses-and-ignores rather than implementing (CC-CSS-1: antialiasing is
/// already on by default, so `-webkit-font-smoothing` has no effect to wire).
pub(crate) const ALLOWED_UNKNOWN_PROPERTIES: &[&str] = &["-webkit-font-smoothing"];

/// Identifiers that are valid HTML `id`/`data-action` values but reserved in
/// Rust — build.rs fails loudly if a future asset id collides with one of
/// these instead of silently generating uncompilable code.
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
    "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true",
    "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
];

/// True if `field` (a codegen'd snake_case identifier) collides with a Rust keyword.
pub(crate) fn is_reserved_identifier(field: &str) -> bool {
    RUST_KEYWORDS.contains(&field)
}

/// Returns a human-readable violation string per unsupported CSS construct
/// found in `sheet` (empty = the parse-gate passes).
pub(crate) fn validate_css(sheet: &Stylesheet) -> Vec<String> {
    let mut violations = Vec::new();
    for rule in &sheet.rules {
        validate_rule(rule, &mut violations);
    }
    violations
}

fn validate_rule(rule: &Rule, violations: &mut Vec<String>) {
    for selector in &rule.selectors {
        for part in selector_violations(selector) {
            violations.push(format!("selector `{}`: unsupported {part}", selector.to_css_str()));
        }
    }
    for decl in &rule.declarations {
        if let Some(prop) = declaration_violation(decl) {
            let ctx = rule
                .selectors
                .first()
                .map(|s| s.to_css_str())
                .unwrap_or_default();
            violations.push(format!("unknown property `{prop}` in rule `{ctx}`"));
        }
    }
}

fn declaration_violation(decl: &Declaration) -> Option<&str> {
    let prop = decl.property.as_str();
    if prop.starts_with("--")
        || SUPPORTED_PROPERTIES.contains(&prop)
        || ALLOWED_UNKNOWN_PROPERTIES.contains(&prop)
    {
        return None;
    }
    Some(prop)
}

/// Mirrors `ComplexSelector::is_supported`'s private recursive walk (css-parser
/// keeps that predicate `pub` but opaque — it only returns a bool). Re-walking
/// here lets the build-gate report *which* part is unsupported and grant the
/// `::-webkit-scrollbar*` exception (CC-CSS-1) instead of hard-failing on it.
fn selector_violations(selector: &ComplexSelector) -> Vec<String> {
    if selector.is_supported() {
        return Vec::new();
    }
    let mut out = Vec::new();
    compound_violations(&selector.head, &mut out);
    for (_, compound) in &selector.tail {
        compound_violations(compound, &mut out);
    }
    out
}

fn compound_violations(compound: &CompoundSelector, out: &mut Vec<String>) {
    for part in &compound.parts {
        simple_violations(part, out);
    }
}

fn simple_violations(part: &SimpleSelector, out: &mut Vec<String>) {
    match part {
        SimpleSelector::PseudoClass(pc) => pseudo_class_violations(pc, out),
        SimpleSelector::PseudoElement(pe) => pseudo_element_violations(pe, out),
        SimpleSelector::Type(_)
        | SimpleSelector::Class(_)
        | SimpleSelector::Id(_)
        | SimpleSelector::Universal
        | SimpleSelector::Attribute(_) => {}
    }
}

fn pseudo_class_violations(pc: &PseudoClass, out: &mut Vec<String>) {
    match pc {
        PseudoClass::Unsupported(name) => out.push(format!("pseudo-class `:{name}`")),
        PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
            for s in list {
                out.extend(selector_violations(s));
            }
        }
        PseudoClass::NthChild(_, Some(list)) | PseudoClass::NthLastChild(_, Some(list)) => {
            for s in list {
                out.extend(selector_violations(s));
            }
        }
        PseudoClass::Has(rels) => {
            for r in rels {
                out.extend(selector_violations(&r.selector));
            }
        }
        PseudoClass::Host(Some(list)) => {
            for s in list {
                out.extend(selector_violations(s));
            }
        }
        _ => {}
    }
}

fn pseudo_element_violations(pe: &PseudoElementKind, out: &mut Vec<String>) {
    match pe {
        PseudoElementKind::Unknown(name) => {
            // CC-CSS-1: `::-webkit-scrollbar`/`-thumb`/`-track` are mapped to
            // `scrollbar-width`/`scrollbar-color` in layout::style — allowed.
            if !name.to_ascii_lowercase().starts_with("-webkit-scrollbar") {
                out.push(format!("pseudo-element `::{name}`"));
            }
        }
        PseudoElementKind::Slotted(Some(list)) => {
            for s in list {
                out.extend(selector_violations(s));
            }
        }
        _ => {}
    }
}

/// `avatarBtn` / `dt-console` → `avatar_btn` / `dt_console`.
pub(crate) fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower_or_digit = false;
    for c in s.chars() {
        if c == '-' || c == '_' {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            prev_lower_or_digit = false;
            continue;
        }
        if c.is_ascii_uppercase() {
            if prev_lower_or_digit {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower_or_digit = false;
        } else {
            out.push(c);
            prev_lower_or_digit = c.is_ascii_lowercase() || c.is_ascii_digit();
        }
    }
    out
}

/// `set-devtools-tab` → `SetDevtoolsTab`.
pub(crate) fn to_pascal_case(s: &str) -> String {
    s.split(['-', '_'])
        .filter(|p| !p.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_snake_case_converts_camel_and_kebab() {
        assert_eq!(to_snake_case("avatarBtn"), "avatar_btn");
        assert_eq!(to_snake_case("dt-console"), "dt_console");
        assert_eq!(to_snake_case("i-back"), "i_back");
        assert_eq!(to_snake_case("appFrame"), "app_frame");
        assert_eq!(to_snake_case("sidebar"), "sidebar");
    }

    #[test]
    fn is_reserved_identifier_flags_rust_keywords() {
        assert!(is_reserved_identifier("type"));
        assert!(is_reserved_identifier("self"));
        assert!(!is_reserved_identifier("avatar_btn"));
    }

    #[test]
    fn to_pascal_case_converts_kebab() {
        assert_eq!(to_pascal_case("toggle-find"), "ToggleFind");
        assert_eq!(to_pascal_case("set-devtools-tab"), "SetDevtoolsTab");
        assert_eq!(to_pascal_case("omni-go"), "OmniGo");
    }

    #[test]
    fn validate_css_accepts_supported_property_and_selector() {
        let sheet = lumen_css_parser::parse(".tab-row:hover { color: red; }");
        assert_eq!(validate_css(&sheet), Vec::<String>::new());
    }

    #[test]
    fn validate_css_allows_webkit_scrollbar_pseudo_elements() {
        let sheet = lumen_css_parser::parse(
            "::-webkit-scrollbar { width: 8px; } ::-webkit-scrollbar-thumb { background: red; }",
        );
        assert_eq!(validate_css(&sheet), Vec::<String>::new());
    }

    #[test]
    fn validate_css_allows_font_smoothing() {
        let sheet = lumen_css_parser::parse("body { -webkit-font-smoothing: antialiased; }");
        assert_eq!(validate_css(&sheet), Vec::<String>::new());
    }

    #[test]
    fn validate_css_rejects_unknown_property() {
        let sheet = lumen_css_parser::parse(".btn { made-up-property: 1px; }");
        let violations = validate_css(&sheet);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("made-up-property"), "{violations:?}");
    }

    #[test]
    fn validate_css_rejects_unknown_pseudo_element() {
        let sheet = lumen_css_parser::parse("::made-up-pseudo { color: red; }");
        let violations = validate_css(&sheet);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("made-up-pseudo"), "{violations:?}");
    }
}
