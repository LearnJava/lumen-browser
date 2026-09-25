//! BUG-639 — the Navigation API shim was missing its spec types:
//! `NavigationCurrentEntryChangeEvent`, `updateCurrentEntry()`,
//! `NavigationDestination`, and a `NavigationHistoryEntry` that is an
//! `EventTarget` with stable identity and a `dispose` event.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn runtime() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(
        make_doc(),
        "https://example.com/page.html",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();
    rt
}

fn bool_of(rt: &V8JsRuntime, code: &str) -> bool {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::Bool(b) => b,
        other => panic!("{code}: expected a bool, got {other:?}"),
    }
}

fn str_of(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

/// Publish shell state the way `commit_nav_state` does.
fn set_state(rt: &V8JsRuntime, entries: &[(&str, &str)], index: usize) {
    let list: Vec<String> = entries
        .iter()
        .map(|(url, key)| {
            format!(
                r#"{{"url":"{url}","key":"{key}","id":"id-{}","state":null}}"#,
                key.trim_start_matches("nav-")
            )
        })
        .collect();
    let json = format!(r#"{{"entries":[{}],"index":{index}}}"#, list.join(","));
    let quoted = serde_json::to_string(&json).unwrap();
    rt.eval(&format!("_lumen_navigation_set_state({quoted}); true")).unwrap();
}

fn two_entries(rt: &V8JsRuntime) {
    set_state(
        rt,
        &[("https://example.com/a", "nav-1"), ("https://example.com/page.html", "nav-2")],
        1,
    );
}

// ── NavigationCurrentEntryChangeEvent ───────────────────────────────────

/// WPT `currententrychange-event/constructor.html`.
#[test]
fn current_entry_change_event_constructor_requires_from() {
    let rt = runtime();
    two_entries(&rt);
    assert!(bool_of(&rt, "typeof NavigationCurrentEntryChangeEvent === 'function'"));
    assert!(bool_of(
        &rt,
        "try { new NavigationCurrentEntryChangeEvent('currententrychange'); false } \
         catch (e) { e instanceof TypeError }"
    ));
    assert!(bool_of(
        &rt,
        "try { new NavigationCurrentEntryChangeEvent('currententrychange', \
               { navigationType: 'push' }); false } \
         catch (e) { e instanceof TypeError }"
    ));
    assert!(bool_of(
        &rt,
        "(() => { const e = new NavigationCurrentEntryChangeEvent('currententrychange', \
                    { navigationType: 'replace', from: navigation.currentEntry }); \
                  return e.navigationType === 'replace' && e.from === navigation.currentEntry \
                    && e instanceof Event; })()"
    ));
    assert!(bool_of(
        &rt,
        "new NavigationCurrentEntryChangeEvent('x', { from: navigation.currentEntry }) \
           .navigationType === null"
    ));
}

/// The shell-fired event is the real type, trusted, with `from` = the entry
/// that was current before the change.
#[test]
fn shell_fired_currententrychange_carries_from_and_type() {
    let rt = runtime();
    two_entries(&rt);
    rt.eval(
        "globalThis.__before = navigation.currentEntry; \
         navigation.oncurrententrychange = e => { globalThis.__ev = e; }; true",
    )
    .unwrap();
    set_state(
        &rt,
        &[
            ("https://example.com/a", "nav-1"),
            ("https://example.com/page.html", "nav-2"),
            ("https://example.com/page.html#x", "nav-3"),
        ],
        2,
    );
    rt.eval("_lumen_fire_currententrychange(); true").unwrap();
    assert!(bool_of(&rt, "__ev.constructor === NavigationCurrentEntryChangeEvent"));
    assert!(bool_of(&rt, "__ev.isTrusted && !__ev.bubbles && !__ev.cancelable"));
    assert!(bool_of(&rt, "__ev.from === __before"));
    assert_eq!(str_of(&rt, "__ev.navigationType"), "push");
    // Back to nav-2 — an entry already in the list: a traversal.
    set_state(
        &rt,
        &[
            ("https://example.com/a", "nav-1"),
            ("https://example.com/page.html", "nav-2"),
            ("https://example.com/page.html#x", "nav-3"),
        ],
        1,
    );
    rt.eval("_lumen_fire_currententrychange(); true").unwrap();
    assert_eq!(str_of(&rt, "__ev.navigationType"), "traverse");
    assert!(bool_of(&rt, "__ev.from.key === 'nav-3'"));
}

// ── updateCurrentEntry ──────────────────────────────────────────────────

/// WPT `updateCurrentEntry-method/basic.html` + `no-args.html` +
/// `currententrychange-event/navigation-updateCurrentEntry.html`.
#[test]
fn update_current_entry_sets_state_and_fires_currententrychange() {
    let rt = runtime();
    two_entries(&rt);
    assert!(bool_of(&rt, "navigation.currentEntry.getState() === undefined"));
    assert!(bool_of(
        &rt,
        "try { navigation.updateCurrentEntry(); false } catch (e) { e instanceof TypeError }"
    ));
    assert!(bool_of(
        &rt,
        "try { navigation.updateCurrentEntry({}); false } catch (e) { e instanceof TypeError }"
    ));
    rt.eval(
        "globalThis.__n = 0; globalThis.__navs = 0; \
         navigation.onnavigate = () => { __navs++; }; \
         navigation.oncurrententrychange = e => { \
           __n++; globalThis.__ok = e.from === navigation.currentEntry \
             && e.navigationType === null \
             && navigation.currentEntry.getState().key === 'value'; }; \
         globalThis.__s = { key: 'value' }; \
         navigation.updateCurrentEntry({ state: __s }); true",
    )
    .unwrap();
    assert!(bool_of(&rt, "__n === 1 && __ok && __navs === 0"));
    // A fresh clone per read, never the caller's object.
    assert!(bool_of(&rt, "navigation.currentEntry.getState() !== __s"));
    assert!(bool_of(
        &rt,
        "navigation.currentEntry.getState() !== navigation.currentEntry.getState()"
    ));
    // Kept separate from history.state.
    assert!(bool_of(&rt, "history.state === null"));
    rt.eval("navigation.updateCurrentEntry({ state: navigation.currentEntry.getState() }); true")
        .unwrap();
    assert!(bool_of(&rt, "__n === 2"));
}

#[test]
fn update_current_entry_rejects_unserializable_state_without_change() {
    let rt = runtime();
    two_entries(&rt);
    assert!(bool_of(
        &rt,
        "try { navigation.updateCurrentEntry({ state: () => 1 }); false } \
         catch (e) { e.name === 'DataCloneError' }"
    ));
    assert!(bool_of(&rt, "navigation.currentEntry.getState() === undefined"));
}

// ── NavigationHistoryEntry ──────────────────────────────────────────────

#[test]
fn history_entry_is_event_target_with_stable_identity() {
    let rt = runtime();
    two_entries(&rt);
    assert!(bool_of(&rt, "navigation.currentEntry instanceof EventTarget"));
    assert!(bool_of(&rt, "navigation.currentEntry === navigation.currentEntry"));
    assert!(bool_of(&rt, "navigation.entries()[1] === navigation.currentEntry"));
    assert!(bool_of(&rt, "'ondispose' in navigation.currentEntry"));
    assert!(bool_of(
        &rt,
        "try { new NavigationHistoryEntry(); false } catch (e) { e instanceof TypeError }"
    ));
    assert!(bool_of(&rt, "navigation.currentEntry.sameDocument === true"));
}

/// Pruning the forward list disposes the pruned entry: `index` → -1 and a
/// trusted `dispose` event fires as soon as the shell publishes the change.
#[test]
fn entry_dropped_from_the_stacks_is_disposed() {
    let rt = runtime();
    set_state(
        &rt,
        &[
            ("https://example.com/a", "nav-1"),
            ("https://example.com/page.html", "nav-2"),
            ("https://example.com/c", "nav-3"),
        ],
        1,
    );
    rt.eval(
        "globalThis.__fwd = navigation.entries()[2]; globalThis.__d = null; \
         __fwd.addEventListener('dispose', e => { __d = e; }); true",
    )
    .unwrap();
    assert!(bool_of(&rt, "__fwd.index === 2"));
    // A push from index 1 truncates the forward entry.
    set_state(
        &rt,
        &[
            ("https://example.com/a", "nav-1"),
            ("https://example.com/page.html", "nav-2"),
            ("https://example.com/page.html#n", "nav-4"),
        ],
        2,
    );
    assert!(bool_of(&rt, "__d !== null && __d.isTrusted && __d.target === __fwd"));
    assert!(bool_of(&rt, "__fwd.index === -1"));
    assert!(bool_of(&rt, "navigation.entries().indexOf(__fwd) === -1"));
}

// ── NavigationDestination ───────────────────────────────────────────────

#[test]
fn navigate_event_destination_is_navigation_destination() {
    let rt = runtime();
    two_entries(&rt);
    rt.eval(
        "navigation.onnavigate = e => { globalThis.__dest = e.destination; }; \
         _lumen_dispatch_navigate('push', 'https://example.com/other', true, false); true",
    )
    .unwrap();
    assert!(bool_of(&rt, "__dest instanceof NavigationDestination"));
    assert_eq!(str_of(&rt, "__dest.url"), "https://example.com/other");
    assert!(bool_of(
        &rt,
        "__dest.key === '' && __dest.id === '' && __dest.index === -1 \
         && __dest.sameDocument === false && __dest.getState() === undefined"
    ));
    // A fragment navigation is same-document; a relative URL resolves.
    rt.eval("_lumen_dispatch_navigate('push', '#frag', true, true); true").unwrap();
    assert_eq!(str_of(&rt, "__dest.url"), "https://example.com/page.html#frag");
    assert!(bool_of(&rt, "__dest.sameDocument === true"));
}

#[test]
fn traverse_destination_describes_the_target_entry() {
    let rt = runtime();
    two_entries(&rt);
    rt.eval(
        "navigation.onnavigate = e => { globalThis.__dest = e.destination; }; \
         _lumen_dispatch_navigate('traverse', '', true, false, 'nav-1'); true",
    )
    .unwrap();
    assert!(bool_of(
        &rt,
        "__dest.key === 'nav-1' && __dest.id === 'id-1' && __dest.index === 0 \
         && __dest.url === 'https://example.com/a'"
    ));
}

/// `navigate(url, {state})` state lands on the entry the shell commits, and
/// `history: 'replace'` is read as a replace request.
#[test]
fn navigate_state_is_attached_to_the_committed_entry() {
    let rt = runtime();
    two_entries(&rt);
    rt.eval("navigation.navigate('#1', { state: { v: 7 }, history: 'replace' }); true").unwrap();
    let q = rt.take_nav_updates();
    assert!(q.iter().any(|(a, url, _, _)| matches!(a, NavAction::Replace) && url == "#1"));
    set_state(
        &rt,
        &[("https://example.com/a", "nav-1"), ("https://example.com/page.html#1", "nav-3")],
        1,
    );
    assert!(bool_of(&rt, "navigation.currentEntry.getState().v === 7"));
    assert!(bool_of(
        &rt,
        "navigation.entries()[0].getState() === undefined"
    ));
}

// ── NavigationResult / initial entry ────────────────────────────────────

/// `navigate()` returns `{committed, finished}` synchronously, not a promise
/// of it — WPT reads `navigation.navigate(u).committed` directly.
#[test]
fn navigate_returns_navigation_result_synchronously() {
    let rt = runtime();
    two_entries(&rt);
    assert!(bool_of(
        &rt,
        "(() => { const r = navigation.navigate('#a'); \
                  return !(r instanceof Promise) && r.committed instanceof Promise \
                    && r.finished instanceof Promise; })()"
    ));
    // Unknown key: rejected result, nothing queued.
    rt.take_nav_updates();
    assert!(bool_of(
        &rt,
        "navigation.traverseTo('nope').committed instanceof Promise"
    ));
    assert!(rt.take_nav_updates().is_empty());
    // Unserializable state throws synchronously, before anything is queued.
    assert!(bool_of(
        &rt,
        "try { navigation.navigate('#b', { state: () => 1 }); false } \
         catch (e) { e.name === 'DataCloneError' }"
    ));
    assert!(rt.take_nav_updates().is_empty());
}

/// Before the shell's first publish (parse-time scripts, `onload`) the
/// document still has a current entry, and the first publish adopts that very
/// object — state set on it survives.
#[test]
fn current_entry_exists_before_the_first_shell_publish() {
    let rt = runtime();
    assert!(bool_of(&rt, "navigation.currentEntry !== null"));
    assert_eq!(
        str_of(&rt, "navigation.currentEntry.url"),
        "https://example.com/page.html"
    );
    assert!(bool_of(
        &rt,
        "navigation.currentEntry.index === 0 && navigation.entries().length === 1"
    ));
    rt.eval(
        "globalThis.__early = navigation.currentEntry; \
         navigation.updateCurrentEntry({ state: 5 }); true",
    )
    .unwrap();
    two_entries(&rt);
    assert!(bool_of(&rt, "navigation.currentEntry === __early"));
    assert!(bool_of(
        &rt,
        "__early.key === 'nav-2' && __early.index === 1 && __early.getState() === 5"
    ));
}

// ── transition / activation exist (null, not undefined) ─────────────────────

#[test]
fn transition_and_activation_are_null_not_undefined() {
    let rt = runtime();
    assert!(bool_of(
        &rt,
        "navigation.transition === null && navigation.activation === null \
         && 'transition' in navigation"
    ));
}

// ── canceled navigate aborts the signal; navigateerror rejects the result ────

#[test]
fn prevent_default_aborts_signal_and_navigateerror_rejects_result() {
    let rt = runtime();
    rt.eval(
        "globalThis.__ev = []; \
         navigation.onnavigate = e => { globalThis.__sig = e.signal; \
           e.signal.onabort = () => __ev.push('abort'); e.preventDefault(); }; \
         navigation.onnavigateerror = () => __ev.push('navigateerror'); \
         globalThis.__r = navigation.navigate('?1'); \
         __r.committed.catch(e => { globalThis.__err = e.name; }); \
         globalThis._lumen_navigation_report_intercept = () => {}; \
         _lumen_dispatch_navigate('push', 'http://example.test/?1', true, false); \
         _lumen_fire_navigate_error(); true",
    )
    .unwrap();
    assert!(bool_of(&rt, "__sig.aborted === true"));
    assert!(bool_of(&rt, "__ev.join(',') === 'abort,navigateerror'"));
    assert!(bool_of(&rt, "__err === 'AbortError'"));
}

// ── intercept() makes navigation.transition non-null until the outcome ──────

#[test]
fn intercept_sets_transition_until_navigatesuccess() {
    let rt = runtime();
    rt.eval(
        "globalThis._lumen_navigation_report_intercept = () => {}; \
         navigation.onnavigate = e => e.intercept(); \
         _lumen_dispatch_navigate('push', 'http://example.test/#a', true, true); \
         globalThis.__tr = navigation.transition; \
         globalThis.__done = false; __tr.finished.then(() => { __done = true; }); \
         _lumen_fire_navigate_success(); true",
    )
    .unwrap();
    assert!(bool_of(
        &rt,
        "__tr instanceof NavigationTransition && __tr.navigationType === 'push' \
         && __tr.to.url === 'http://example.test/#a'"
    ));
    assert!(bool_of(&rt, "navigation.transition === null && __done === true"));
}
