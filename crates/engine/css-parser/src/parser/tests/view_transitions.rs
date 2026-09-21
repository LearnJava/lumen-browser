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
