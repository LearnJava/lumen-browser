//! BUG-534 — CSS Custom Highlight API: `Highlight` as a real Setlike
//! interface and `CSS.highlights` as a real `HighlightRegistry` (Maplike)
//! instance, over the actual V8 shim. Transcribes the vendored
//! `css/css-highlight-api/Highlight-setlike.html`,
//! `Highlight-type-attribute.tentative.html`,
//! `HighlightRegistry-maplike.html`, `HighlightRegistry-iteration.html`
//! (subset) and the two tampered-prototype files directly.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

fn str_eval(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::String(s)) => s,
        Ok(other) => panic!("expected string from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

// ── Highlight-setlike.html ────────────────────────────────────────────────

#[test]
fn highlight_starts_empty_with_default_priority() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var h = new Highlight();
h.priority === 0 && h.size === 0
"#
    ));
}

#[test]
fn highlight_add_has_and_size_track_membership() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var r0 = new Range(), r1 = new Range(), r2 = new Range();
var h = new Highlight();
var ok = !h.has(r0) && !h.has(r1) && !h.has(r2);
h.add(r0);
ok = ok && h.has(r0) && !h.has(r1) && h.size === 1;
h.add(r0);
ok = ok && h.size === 1;
h.add(r1);
ok = ok && h.has(r0) && h.has(r1) && !h.has(r2) && h.size === 2;
ok
"#
    ));
}

#[test]
fn highlight_add_is_chainable() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var r0 = new Range(), r1 = new Range();
var h = new Highlight();
h.add(r0).add(r1);
h.has(r0) && h.has(r1) && h.size === 2
"#
    ));
}

#[test]
fn highlight_delete_returns_whether_it_removed_something() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var r0 = new Range(), r1 = new Range(), r2 = new Range();
var h = new Highlight(r0, r1);
var ok = h.delete(r2) === false;
ok = ok && h.delete(r1) === true;
ok = ok && h.delete(r1) === false;
ok = ok && h.delete(r0) === true;
ok = ok && h.delete(r0) === false;
ok
"#
    ));
}

#[test]
fn highlight_constructor_deduplicates_like_a_set() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var r0 = new Range(), r1 = new Range();
var h = new Highlight(r0, r0);
var ok = h.has(r0) && h.size === 1;
h = new Highlight(r0, r1, r0, r1);
ok = ok && h.has(r0) && h.has(r1) && h.size === 2;
ok
"#
    ));
}

#[test]
fn highlight_clear_empties_it() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var h = new Highlight(new Range(), new Range());
var ok = h.size === 2;
h.clear();
ok = ok && h.size === 0;
h.clear();
ok = ok && h.size === 0;
ok
"#
    ));
}

#[test]
fn highlight_accepts_static_ranges_too() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var _c = document.createElement('div');
var sr = new StaticRange({startContainer: _c, startOffset: 0, endContainer: _c, endOffset: 0});
var h = new Highlight(sr);
h.has(sr) && h.size === 1
"#
    ));
}

// ── Highlight-type-attribute.tentative.html ───────────────────────────────

#[test]
fn highlight_type_defaults_to_highlight_and_rejects_out_of_enum_values() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var h = new Highlight();
var ok = h.type === 'highlight';
h.type = 'type-not-listed-in-HighlightType-enum';
ok = ok && h.type === 'highlight';
h.type = 'spelling-error';
ok = ok && h.type === 'spelling-error';
h.type = 'grammar-error';
ok = ok && h.type === 'grammar-error';
ok
"#
    ));
}

// ── HighlightRegistry-maplike.html ────────────────────────────────────────

#[test]
fn highlight_registry_global_exists_with_a_throwing_constructor() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var threw = false;
try { new HighlightRegistry(); } catch (e) { threw = e instanceof TypeError; }
window.HighlightRegistry !== undefined
    && typeof HighlightRegistry === 'function'
    && threw
    && CSS.highlights !== undefined
    && Object.getPrototypeOf(CSS.highlights) === HighlightRegistry.prototype
    && CSS.highlights.size === 0
"#
    ));
}

#[test]
fn highlight_registry_set_get_has_delete_size_track_insertion_order() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var h1 = new Highlight(), h2 = new Highlight();
var ok = !CSS.highlights.has('example1');
CSS.highlights.set('example1', h1);
ok = ok && CSS.highlights.has('example1') && CSS.highlights.size === 1;
ok = ok && CSS.highlights.delete('example2') === false && CSS.highlights.size === 1;
CSS.highlights.set('example2', h2);
ok = ok && CSS.highlights.get('example1') === h1 && CSS.highlights.get('example2') === h2 && CSS.highlights.size === 2;
ok = ok && CSS.highlights.delete('example2') === true;
ok = ok && CSS.highlights.has('example1') && !CSS.highlights.has('example2');
ok = ok && CSS.highlights.get('example2') === undefined && CSS.highlights.size === 1;
CSS.highlights.set('example1', h2);
ok = ok && CSS.highlights.size === 1 && CSS.highlights.get('example1') === h2;
CSS.highlights.clear();
ok = ok && CSS.highlights.size === 0;
CSS.highlights.set('example1', h1).set('example2', h2);
ok = ok && CSS.highlights.size === 2;
CSS.highlights.clear();
ok
"#
    ));
}

// ── HighlightRegistry-iteration.html (subset: keys/values/entries/forEach) ─

#[test]
fn highlight_registry_iterates_keys_values_entries_and_for_each_in_insertion_order() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
CSS.highlights.clear();
var h1 = new Highlight(), h2 = new Highlight();
CSS.highlights.set('example1', h1);
CSS.highlights.set('example2', h2);

var keys = [...CSS.highlights.keys()];
var values = [...CSS.highlights.values()];
var entries = [...CSS.highlights.entries()];
var viaSymbol = [...CSS.highlights];
var seen = [];
CSS.highlights.forEach(function (h) { seen.push(h); });

var ok = keys.length === 2 && keys[0] === 'example1' && keys[1] === 'example2';
ok = ok && values.length === 2 && values[0] === h1 && values[1] === h2;
ok = ok && entries.length === 2
    && entries[0][0] === 'example1' && entries[0][1] === h1
    && entries[1][0] === 'example2' && entries[1][1] === h2;
ok = ok && viaSymbol.length === 2 && viaSymbol[0][0] === 'example1' && viaSymbol[1][1] === h2;
ok = ok && seen.length === 2 && seen[0] === h1 && seen[1] === h2;
CSS.highlights.clear();
ok
"#
    ));
}

#[test]
fn highlight_iterates_setlike_keys_values_entries() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var r0 = new Range(), r1 = new Range();
var h = new Highlight(r0, r1);
var keys = [...h.keys()];
var values = [...h.values()];
var entries = [...h.entries()];
var viaSymbol = [...h];
keys.length === 2 && keys[0] === r0 && keys[1] === r1
    && values.length === 2 && values[0] === r0 && values[1] === r1
    && entries.length === 2 && entries[0][0] === r0 && entries[0][1] === r0
    && viaSymbol.length === 2 && viaSymbol[0] === r0 && viaSymbol[1] === r1
"#
    ));
}

// ── Highlight-setlike-tampered-Set-prototype.html ─────────────────────────

#[test]
fn highlight_setlike_survives_a_tampered_set_prototype() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
delete Set.prototype.size;
Set.prototype.entries = null;
Set.prototype.forEach = undefined;
Set.prototype.has = "foo";
Set.prototype.keys = 0;
Set.prototype.values = Symbol();
Set.prototype[Symbol.iterator] = 1;
Set.prototype.add = true;
Set.prototype.clear = "";
Set.prototype.delete = -1.5;
Object.freeze(Set.prototype);

var _c = document.createElement('div');
var sr = new StaticRange({startContainer: _c, endContainer: _c, startOffset: 0, endOffset: 0});
var h = new Highlight(sr);
var ok = h.size === 1 && h.has(sr) && [...h.entries()][0][0] === sr;
h.clear();
ok = ok && h.size === 0;
h.add(sr);
ok = ok && h.size === 1;
h.delete(sr);
ok = ok && h.size === 0 && !h.has(sr);
h.add(sr);
ok = ok && [...h.keys()][0] === sr && [...h.values()][0] === sr;
var called = false;
h.forEach(function () { called = true; });
ok && called
"#
    ));
}

// ── HighlightRegistry-maplike-tampered-Map-prototype.html ─────────────────

#[test]
fn highlight_registry_maplike_survives_a_tampered_map_prototype() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
CSS.highlights.clear();
// Container created before the tamper, matching the real WPT test's use of
// the already-existing `document.body` — `document.createElement` itself
// leans on an internal `Map` for its own prototype cache, unrelated to this
// shim, so it must not run after `Map.prototype` is frozen below.
var _c = document.createElement('div');
var h = new Highlight(new StaticRange({startContainer: _c, endContainer: _c, startOffset: 0, endOffset: 0}));

delete Map.prototype.size;
Map.prototype.entries = null;
Map.prototype.forEach = undefined;
Map.prototype.get = "foo";
Map.prototype.has = 0;
Map.prototype.keys = Symbol();
Map.prototype.values = 1;
Map.prototype[Symbol.iterator] = true;
Map.prototype.clear = false;
Map.prototype.delete = "";
Map.prototype.set = 3.14;
Object.freeze(Map.prototype);

var reg = CSS.highlights;
var ok = reg.size === 0;
reg.set("foo", h);
ok = ok && reg.size === 1 && reg.has("foo") && [...reg.entries()][0][0] === "foo";
reg.clear();
ok = ok && reg.size === 0 && reg.get("foo") === undefined;
reg.set("bar", h);
ok = ok && reg.get("bar") === h && [...reg][0][1] === h;
reg.delete("bar");
ok = ok && reg.size === 0 && !reg.has("bar");
reg.set("baz", h);
ok = ok && [...reg.keys()][0] === "baz" && [...reg.values()][0] === h;
var called = false;
reg.forEach(function () { called = true; });
ok = ok && called;
reg.clear();
ok
"#
    ));
}

// ── HighlightRegistry-iteration-with-modifications.html ───────────────────

#[test]
fn highlight_registry_iteration_sees_insertions_made_after_it_started() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
CSS.highlights.clear();
var h1 = new Highlight();
var it = CSS.highlights[Symbol.iterator]();
CSS.highlights.set('example1', h1);
var e = it.next();
var ok = e.done === false && e.value[0] === 'example1' && e.value[1] === h1;
e = it.next();
ok = ok && e.done === true && e.value === undefined;
CSS.highlights.clear();
ok
"#
    ));
}

#[test]
fn highlight_registry_iteration_skips_an_entry_deleted_before_being_visited() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
CSS.highlights.clear();
var h1 = new Highlight(), h2 = new Highlight();
CSS.highlights.set('example1', h1);
CSS.highlights.set('example2', h2);
var it = CSS.highlights[Symbol.iterator]();
CSS.highlights.delete('example2');
var e = it.next();
var ok = e.done === false && e.value[0] === 'example1' && e.value[1] === h1;
e = it.next();
ok = ok && e.done === true && e.value === undefined;
CSS.highlights.clear();
ok
"#
    ));
}

#[test]
fn highlight_registry_iteration_unaffected_by_deleting_an_already_visited_entry() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
CSS.highlights.clear();
var h1 = new Highlight(), h2 = new Highlight();
CSS.highlights.set('example1', h1);
CSS.highlights.set('example2', h2);
var it = CSS.highlights[Symbol.iterator]();
var e = it.next();
var ok = e.done === false && e.value[0] === 'example1';
CSS.highlights.delete('example1');
e = it.next();
ok = ok && e.done === false && e.value[0] === 'example2' && e.value[1] === h2;
e = it.next();
ok = ok && e.done === true;
CSS.highlights.clear();
ok
"#
    ));
}

#[test]
fn highlight_registry_iteration_ends_when_cleared_before_being_visited() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
CSS.highlights.clear();
CSS.highlights.set('example1', new Highlight());
var it = CSS.highlights[Symbol.iterator]();
CSS.highlights.clear();
var e = it.next();
e.done === true && e.value === undefined
"#
    ));
}

#[test]
fn highlight_iteration_sees_a_range_added_after_it_started() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var r0 = new Range();
var h = new Highlight();
var it = h[Symbol.iterator]();
h.add(r0);
var e = it.next();
var ok = e.done === false && e.value === r0;
e = it.next();
ok && e.done === true && e.value === undefined
"#
    ));
}

#[test]
fn highlight_iteration_skips_a_range_deleted_before_being_visited() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var r0 = new Range();
var h = new Highlight(r0);
var it = h[Symbol.iterator]();
h.delete(r0);
var e = it.next();
e.done === true && e.value === undefined
"#
    ));
}

// ── HighlightRegistry-highlightsFromPoint.html (argument validation only —
// real hit-testing is not implemented, see crates/js/src/highlight_api.rs) ─

#[test]
fn highlights_from_point_validates_its_arguments() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
function throwsTypeError(fn) {
  try { fn(); return false; } catch (e) { return e instanceof TypeError; }
}
throwsTypeError(function () { CSS.highlights.highlightsFromPoint("asdf", 10); })
    && throwsTypeError(function () { CSS.highlights.highlightsFromPoint(10); })
    && throwsTypeError(function () { CSS.highlights.highlightsFromPoint(); })
    && throwsTypeError(function () { CSS.highlights.highlightsFromPoint(10, 10, "asdf"); })
"#
    ));
}

#[test]
fn highlights_from_point_returns_empty_array_for_valid_arguments() {
    let rt = make_rt();
    let out = str_eval(
        &rt,
        r#"
JSON.stringify(CSS.highlights.highlightsFromPoint(-1, -1))
"#,
    );
    assert_eq!(out, "[]", "no paint hit-testing is wired up yet — see BUG-534's remaining scope");
}

