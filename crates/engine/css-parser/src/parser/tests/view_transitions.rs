use super::*;

// ── CSS View Transitions Module Level 2 §3 — @view-transition ──

#[test]
fn at_view_transition_navigation_auto() {
    let s = parse("@view-transition { navigation: auto; }");
    assert_eq!(s.view_transition_rules.len(), 1);
    assert_eq!(
        s.view_transition_rules[0].navigation,
        ViewTransitionNavigation::Auto
    );
}

#[test]
fn at_view_transition_navigation_none() {
    let s = parse("@view-transition { navigation: none; }");
    assert_eq!(s.view_transition_rules.len(), 1);
    assert_eq!(
        s.view_transition_rules[0].navigation,
        ViewTransitionNavigation::None
    );
}

#[test]
fn at_view_transition_missing_descriptor_defaults_to_none() {
    // Спек: отсутствие `navigation` — не opt-in, дефолт `none`.
    let s = parse("@view-transition { }");
    assert_eq!(s.view_transition_rules.len(), 1);
    assert_eq!(
        s.view_transition_rules[0].navigation,
        ViewTransitionNavigation::None
    );
}

#[test]
fn at_view_transition_unknown_descriptor_ignored() {
    let s = parse("@view-transition { color: red; navigation: auto; }");
    assert_eq!(s.view_transition_rules.len(), 1);
    assert_eq!(
        s.view_transition_rules[0].navigation,
        ViewTransitionNavigation::Auto
    );
}

#[test]
fn at_view_transition_unknown_value_defaults_to_none() {
    let s = parse("@view-transition { navigation: bogus; }");
    assert_eq!(s.view_transition_rules.len(), 1);
    assert_eq!(
        s.view_transition_rules[0].navigation,
        ViewTransitionNavigation::None
    );
}

#[test]
fn at_view_transition_no_block_is_ignored() {
    // Без блока (аналог кривого @page без `{`) — правило не создаётся,
    // следующий rule должен парситься дальше как обычно.
    let s = parse("@view-transition ; h1 { color: red; }");
    assert_eq!(s.view_transition_rules.len(), 0);
    assert_eq!(s.rules.len(), 1);
}

#[test]
fn at_view_transition_followed_by_other_rules() {
    let s = parse("@view-transition { navigation: auto; } h1 { color: red; }");
    assert_eq!(s.view_transition_rules.len(), 1);
    assert_eq!(s.rules.len(), 1);
}

// ── CSS View Transitions L1 §6 — ::view-transition-* pseudo-tree ──

fn only_pe(sel: &str) -> PseudoElementKind {
    let list = parse_selector_list(sel);
    assert_eq!(list.len(), 1, "{sel}");
    let part = list[0].head.parts.iter().find_map(|p| match p {
        SimpleSelector::PseudoElement(pe) => Some(pe.clone()),
        _ => None,
    });
    part.unwrap_or_else(|| panic!("no pseudo-element in {sel}"))
}

#[test]
fn view_transition_pseudos_parse_with_names() {
    assert_eq!(only_pe("::view-transition"), PseudoElementKind::ViewTransition);
    assert_eq!(
        only_pe("::view-transition-group(root)"),
        PseudoElementKind::ViewTransitionGroup("root".into())
    );
    assert_eq!(
        only_pe("::view-transition-image-pair(hero)"),
        PseudoElementKind::ViewTransitionImagePair("hero".into())
    );
    assert_eq!(
        only_pe("::view-transition-old(*)"),
        PseudoElementKind::ViewTransitionOld("*".into())
    );
    // Names are case-sensitive <custom-ident>s; the function name is not.
    assert_eq!(
        only_pe("::VIEW-TRANSITION-NEW(Hero)"),
        PseudoElementKind::ViewTransitionNew("Hero".into())
    );
    assert_eq!(
        only_pe("::view-transition-old( root )"),
        PseudoElementKind::ViewTransitionOld("root".into())
    );
}

#[test]
fn view_transition_pseudos_valid_and_invalid_selectors() {
    for ok in [
        "::view-transition",
        ":root::view-transition",
        "::view-transition-group(root)",
        "html::view-transition-old(*)",
        "::view-transition-image-pair(a-b)",
        "::view-transition-new(x):hover",
    ] {
        assert!(crate::is_valid_selector_list(ok), "expected valid: {ok}");
    }
    for bad in [
        "::view-transition-group",
        "::view-transition-group()",
        "::view-transition-old(none)",
        "::view-transition-old(inherit)",
        "::view-transition-new(a b)",
        "::view-transition-new(a, b)",
        "::view-transition-group(5)",
        "::view-transition-group('root')",
        "::view-transition(root)",
    ] {
        assert!(!crate::is_valid_selector_list(bad), "expected invalid: {bad}");
    }
}

#[test]
fn view_transition_pseudo_round_trips_through_css_text() {
    for sel in [
        "::view-transition",
        "::view-transition-group(root)",
        "::view-transition-image-pair(*)",
        "::view-transition-old(hero)",
        "::view-transition-new(hero)",
    ] {
        let list = parse_selector_list(sel);
        assert_eq!(pe_to_css_str(&only_pe(sel)), sel);
        assert_eq!(list.len(), 1);
    }
}
